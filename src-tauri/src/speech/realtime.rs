use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

use crate::{audio::AudioChunk, settings::LanguageMode};

pub const REALTIME_TRANSCRIPTION_MODEL: &str = "gpt-realtime-whisper";
const REALTIME_SAMPLE_RATE: u32 = 24_000;
const LIVE_COMMIT_INTERVAL_MS: u64 = 800;

struct AbortOnDrop<T> {
    handle: Option<tokio::task::JoinHandle<T>>,
}

impl<T> AbortOnDrop<T> {
    fn new(handle: tokio::task::JoinHandle<T>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .map_or(true, |handle| handle.is_finished())
    }

    async fn join(mut self) -> Result<T, tokio::task::JoinError> {
        self.handle.take().expect("join handle missing").await
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[derive(Clone, Debug)]
pub struct RealtimeStatus {
    pub state: String,
    pub detail: Option<String>,
}

fn live_debug(message: impl AsRef<str>) {
    let path = std::env::temp_dir().join("aivoice-live-transcript-debug.log");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "{now} {}", message.as_ref())
        });
}

fn send_status(
    status_tx: &Option<mpsc::UnboundedSender<RealtimeStatus>>,
    state: impl Into<String>,
    detail: Option<String>,
) {
    if let Some(tx) = status_tx {
        let _ = tx.send(RealtimeStatus {
            state: state.into(),
            detail,
        });
    }
}

pub fn supports_realtime_model(model: &str) -> bool {
    model.trim() == REALTIME_TRANSCRIPTION_MODEL
}

pub fn supports_realtime_language(base_url: &str, model: &str) -> bool {
    supports_realtime_model(model)
        && reqwest::Url::parse(base_url.trim())
            .ok()
            .and_then(|url| {
                url.host_str()
                    .map(|host| host.eq_ignore_ascii_case("api.openai.com"))
            })
            .unwrap_or(false)
}

pub fn supports_realtime_prompt(base_url: &str, model: &str) -> bool {
    supports_realtime_language(base_url, model)
}

pub fn realtime_vocabulary_prompt(
    base_url: &str,
    model: &str,
    dictionary_words: &[String],
) -> Option<String> {
    if !supports_realtime_prompt(base_url, model) {
        return None;
    }
    const PREFIX: &str = "Important vocabulary: ";
    let mut prompt = PREFIX.to_string();
    let mut added = false;
    for word in dictionary_words {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        let separator = if added { ", " } else { "" };
        if prompt
            .chars()
            .count()
            .saturating_add(separator.chars().count())
            .saturating_add(word.chars().count())
            > crate::corrections::MAX_EFFECTIVE_VOCABULARY_CHARS
        {
            break;
        }
        prompt.push_str(separator);
        prompt.push_str(word);
        added = true;
    }
    added.then_some(prompt)
}

fn build_session_update(
    base_url: &str,
    model: &str,
    live_enabled: bool,
    language_mode: LanguageMode,
    prompt: Option<&str>,
) -> serde_json::Value {
    let mut transcription = serde_json::json!({ "model": model });
    if live_enabled {
        transcription["delay"] = serde_json::json!("minimal");
    }
    if supports_realtime_language(base_url, model) {
        if let Some(language) = language_mode.api_language() {
            transcription["language"] = serde_json::json!(language);
        }
        if let Some(prompt) = prompt.filter(|prompt| !prompt.trim().is_empty()) {
            transcription["prompt"] = serde_json::json!(prompt);
        }
    }
    serde_json::json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": { "input": {
                "format": { "type": "audio/pcm", "rate": REALTIME_SAMPLE_RATE },
                "transcription": transcription,
                "turn_detection": null
            }}
        }
    })
}

fn realtime_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let path = "/realtime?intent=transcription";
    if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}{path}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}{path}")
    } else {
        format!("{base}{path}")
    }
}

fn downmix_resample_to_pcm16(chunk: &AudioChunk) -> Vec<u8> {
    if chunk.samples.is_empty() || chunk.sample_rate == 0 || chunk.channels == 0 {
        return Vec::new();
    }

    let channels = chunk.channels as usize;
    let frames = chunk.samples.len() / channels;
    if frames == 0 {
        return Vec::new();
    }

    let mut mono = Vec::with_capacity(frames);
    for frame in chunk.samples.chunks_exact(channels) {
        mono.push(frame.iter().copied().sum::<f32>() / channels as f32);
    }

    let output_frames =
        ((frames as u64 * REALTIME_SAMPLE_RATE as u64) / chunk.sample_rate.max(1) as u64) as usize;
    if output_frames == 0 {
        return Vec::new();
    }

    let mut pcm = Vec::with_capacity(output_frames * 2);
    let ratio = chunk.sample_rate as f64 / REALTIME_SAMPLE_RATE as f64;
    for i in 0..output_frames {
        let src_pos = i as f64 * ratio;
        let left = src_pos.floor() as usize;
        let right = (left + 1).min(mono.len() - 1);
        let frac = (src_pos - left as f64) as f32;
        let sample = mono[left] * (1.0 - frac) + mono[right] * frac;
        let value = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }
    pcm
}

pub async fn transcribe_realtime(
    base_url: String,
    api_key: String,
    model: String,
    language_mode: LanguageMode,
    prompt: Option<String>,
    mut audio_rx: mpsc::UnboundedReceiver<AudioChunk>,
    partial_tx: Option<mpsc::UnboundedSender<String>>,
    status_tx: Option<mpsc::UnboundedSender<RealtimeStatus>>,
) -> Result<String, String> {
    if std::env::var("AIVOICE_FORCE_REALTIME_FAIL").as_deref() == Ok("1") {
        send_status(
            &status_tx,
            "error",
            Some("Realtime ASR forced failure for fallback QA".to_string()),
        );
        return Err("Realtime ASR forced failure for fallback QA".to_string());
    }

    send_status(&status_tx, "connecting", None);
    live_debug(format!(
        "connecting realtime model={model} live={}",
        partial_tx.is_some()
    ));
    let url = realtime_url(&base_url);
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("Realtime request could not be built: {error}"))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", api_key.trim())
            .parse()
            .map_err(|error| format!("Realtime auth header is invalid: {error}"))?,
    );
    let (ws, _) = match connect_async(request).await {
        Ok(result) => result,
        Err(error) => {
            let message = format!("Realtime connection failed: {error}");
            live_debug(&message);
            send_status(&status_tx, "error", Some(message.clone()));
            return Err(message);
        }
    };
    let (mut write, mut read) = ws.split();
    send_status(&status_tx, "connected", None);
    live_debug("connected realtime websocket");

    let live_enabled = partial_tx.is_some();
    tracing::info!(
        model = %model,
        live_enabled,
        "starting realtime transcription"
    );
    let session_update = build_session_update(
        &base_url,
        &model,
        live_enabled,
        language_mode,
        prompt.as_deref(),
    );
    if let Err(error) = write
        .send(Message::Text(session_update.to_string().into()))
        .await
    {
        let message = format!("Realtime session update failed: {error}");
        live_debug(&message);
        send_status(&status_tx, "error", Some(message.clone()));
        return Err(message);
    }
    send_status(&status_tx, "session_ready", None);
    live_debug("sent realtime session.update");

    let commit_count = Arc::new(AtomicUsize::new(0));
    let sender_commit_count = commit_count.clone();
    let sender_status_tx = status_tx.clone();
    let sender = AbortOnDrop::new(tokio::spawn(async move {
        let mut last_commit = Instant::now();
        let mut has_uncommitted_audio = false;
        while let Some(chunk) = audio_rx.recv().await {
            let pcm = downmix_resample_to_pcm16(&chunk);
            if pcm.is_empty() {
                continue;
            }
            let append = serde_json::json!({
                "type": "input_audio_buffer.append",
                "audio": STANDARD.encode(pcm)
            });
            if let Err(error) = write.send(Message::Text(append.to_string().into())).await {
                let message = format!("Realtime audio send failed: {error}");
                live_debug(&message);
                send_status(&sender_status_tx, "error", Some(message.clone()));
                return Err(message);
            }
            has_uncommitted_audio = true;

            if live_enabled
                && last_commit.elapsed() >= Duration::from_millis(LIVE_COMMIT_INTERVAL_MS)
            {
                let commit = serde_json::json!({ "type": "input_audio_buffer.commit" });
                write
                    .send(Message::Text(commit.to_string().into()))
                    .await
                    .map_err(|error| {
                        let message = format!("Realtime live audio commit failed: {error}");
                        live_debug(&message);
                        send_status(&sender_status_tx, "error", Some(message.clone()));
                        message
                    })?;
                let commits = sender_commit_count.fetch_add(1, Ordering::SeqCst) + 1;
                if commits <= 3 || commits % 10 == 0 {
                    live_debug(format!("sent realtime audio commit count={commits}"));
                }
                has_uncommitted_audio = false;
                last_commit = Instant::now();
            }
        }

        if has_uncommitted_audio || !live_enabled {
            let commit = serde_json::json!({ "type": "input_audio_buffer.commit" });
            write
                .send(Message::Text(commit.to_string().into()))
                .await
                .map_err(|error| {
                    let message = format!("Realtime audio commit failed: {error}");
                    live_debug(&message);
                    send_status(&sender_status_tx, "error", Some(message.clone()));
                    message
                })?;
            let commits = sender_commit_count.fetch_add(1, Ordering::SeqCst) + 1;
            live_debug(format!("sent final realtime audio commit count={commits}"));
        }
        Ok(())
    }));

    let mut final_text = String::new();
    let mut completed_segments: Vec<String> = Vec::new();
    let mut current_segment = String::new();
    let mut completed_count = 0_usize;
    let mut delta_count = 0_usize;
    loop {
        let maybe_message = if live_enabled {
            tokio::select! {
                message = read.next() => message,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if sender.is_finished()
                        && completed_count >= commit_count.load(Ordering::SeqCst)
                    {
                        break;
                    }
                    continue;
                }
            }
        } else {
            read.next().await
        };
        let Some(message) = maybe_message else {
            break;
        };
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                let message = format!("Realtime receive failed: {error}");
                live_debug(&message);
                send_status(&status_tx, "error", Some(message.clone()));
                return Err(message);
            }
        };
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let event_type = event.get("type").and_then(|value| value.as_str());
        match event_type {
            Some("conversation.item.input_audio_transcription.delta") => {
                if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                    delta_count += 1;
                    if delta_count <= 3 || delta_count % 10 == 0 {
                        tracing::debug!(
                            delta_count,
                            delta_chars = delta.chars().count(),
                            "received realtime transcription delta"
                        );
                        live_debug(format!(
                            "received realtime delta count={delta_count} chars={}",
                            delta.chars().count()
                        ));
                    }
                    send_status(&status_tx, "delta", None);
                    if live_enabled {
                        current_segment.push_str(delta);
                        let mut preview = completed_segments.join(" ");
                        if !current_segment.trim().is_empty() {
                            if !preview.is_empty() {
                                preview.push(' ');
                            }
                            preview.push_str(current_segment.trim());
                        }
                        if let Some(tx) = &partial_tx {
                            let _ = tx.send(preview);
                        }
                    } else {
                        final_text.push_str(delta);
                    }
                }
            }
            Some("conversation.item.input_audio_transcription.completed") => {
                completed_count += 1;
                tracing::info!(
                    completed_count,
                    delta_count,
                    "received realtime transcription completed"
                );
                live_debug(format!(
                    "received realtime completed count={completed_count} delta_count={delta_count}"
                ));
                send_status(&status_tx, "completed", None);
                if live_enabled {
                    let segment = event
                        .get("transcript")
                        .and_then(|value| value.as_str())
                        .unwrap_or(current_segment.as_str())
                        .trim()
                        .to_string();
                    if !segment.is_empty() {
                        completed_segments.push(segment);
                    }
                    current_segment.clear();
                    final_text = completed_segments.join(" ");
                    if let Some(tx) = &partial_tx {
                        let _ = tx.send(final_text.clone());
                    }
                    if sender.is_finished()
                        && completed_count >= commit_count.load(Ordering::SeqCst)
                    {
                        break;
                    }
                } else {
                    if let Some(transcript) =
                        event.get("transcript").and_then(|value| value.as_str())
                    {
                        final_text = transcript.to_string();
                    }
                    break;
                }
            }
            Some("error") => {
                let message = event
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(|message| message.as_str())
                    .unwrap_or("Realtime API error")
                    .to_string();
                live_debug(format!("received realtime api error: {message}"));
                send_status(&status_tx, "error", Some(message.clone()));
                return Err(message);
            }
            Some(other) => {
                if delta_count == 0 && completed_count == 0 {
                    live_debug(format!(
                        "received realtime event before transcript: {other}"
                    ));
                }
            }
            None => {}
        }
    }

    match sender.join().await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(error) => return Err(format!("Realtime sender task failed: {error}")),
    }

    tracing::info!(
        completed_count,
        delta_count,
        final_chars = final_text.chars().count(),
        "finished realtime transcription"
    );
    live_debug(format!(
        "finished realtime completed_count={completed_count} delta_count={delta_count} final_chars={}",
        final_text.chars().count()
    ));
    Ok(final_text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_url_maps_openai_rest_base_to_transcription_ws() {
        assert_eq!(
            realtime_url("https://api.openai.com/v1"),
            "wss://api.openai.com/v1/realtime?intent=transcription"
        );
    }

    #[test]
    fn realtime_audio_is_mono_pcm16_24khz() {
        let chunk = AudioChunk {
            samples: vec![0.5, -0.5, 0.25, -0.25],
            sample_rate: 48_000,
            channels: 2,
        };
        let pcm = downmix_resample_to_pcm16(&chunk);
        assert_eq!(pcm.len(), 2);
    }

    #[test]
    fn detects_realtime_model() {
        assert!(supports_realtime_model("gpt-realtime-whisper"));
        assert!(!supports_realtime_model("gpt-4o-mini-transcribe"));
    }

    #[test]
    fn realtime_language_is_optional_and_model_gated() {
        let auto = build_session_update(
            "https://api.openai.com/v1",
            REALTIME_TRANSCRIPTION_MODEL,
            false,
            LanguageMode::Auto,
            None,
        );
        assert!(auto["session"]["audio"]["input"]["transcription"]
            .get("language")
            .is_none());
        let en = build_session_update(
            "https://api.openai.com/v1",
            REALTIME_TRANSCRIPTION_MODEL,
            false,
            LanguageMode::En,
            None,
        );
        assert_eq!(
            en["session"]["audio"]["input"]["transcription"]["language"],
            "en"
        );
        let unsupported = build_session_update(
            "https://compatible.example/v1",
            REALTIME_TRANSCRIPTION_MODEL,
            false,
            LanguageMode::Ja,
            None,
        );
        assert!(unsupported["session"]["audio"]["input"]["transcription"]
            .get("language")
            .is_none());
    }

    #[test]
    fn realtime_prompt_is_official_only_and_optional() {
        let words = vec!["KoeType".to_string(), "音声入力".to_string()];
        let prompt = realtime_vocabulary_prompt(
            "https://api.openai.com/v1",
            REALTIME_TRANSCRIPTION_MODEL,
            &words,
        );
        let official = build_session_update(
            "https://api.openai.com/v1",
            REALTIME_TRANSCRIPTION_MODEL,
            false,
            LanguageMode::Auto,
            prompt.as_deref(),
        );
        assert!(
            official["session"]["audio"]["input"]["transcription"]["prompt"]
                .as_str()
                .unwrap()
                .contains("KoeType")
        );
        assert!(realtime_vocabulary_prompt(
            "https://compatible.example/v1",
            REALTIME_TRANSCRIPTION_MODEL,
            &words
        )
        .is_none());
        assert!(realtime_vocabulary_prompt(
            "https://api.openai.com/v1",
            REALTIME_TRANSCRIPTION_MODEL,
            &[]
        )
        .is_none());
    }

    #[tokio::test]
    async fn dropping_parent_guard_aborts_sender_task() {
        struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        let guard = AbortOnDrop::new(tokio::spawn(async move {
            let _signal = DropSignal(Some(tx));
            std::future::pending::<()>().await;
        }));
        tokio::task::yield_now().await;
        drop(guard);
        tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("sender task was not aborted")
            .expect("drop signal was lost");
    }
}
