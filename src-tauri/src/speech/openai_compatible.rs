use std::fs;
use std::future::Future;

use anyhow::Context;
use futures_util::StreamExt;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use super::{long_audio, SpeechProvider};
use crate::{audio::CapturedAudio, context::FocusedAppContext, recovery, settings::LanguageMode};

const TRANSCRIPTION_TEMPERATURE: &str = "0";
const ASR_REQUEST_TIMEOUT_SECS: u64 = 90;
const TRANSIENT_RETRIES: usize = 1;

pub struct OpenAiCompatibleProvider {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub language_mode: LanguageMode,
    pub dictionary_words: Vec<String>,
    pub focused_context: Option<FocusedAppContext>,
    pub partial_tx: Option<mpsc::UnboundedSender<String>>,
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Clone, serde::Serialize)]
struct LongAudioProgressEvent {
    recovery_id: Option<String>,
    current: usize,
    total: usize,
}

/// CapturedAudio を RIFF WAV バイト列に変換する。
/// サンプルレート・チャンネル数は audio から取得する。
fn encode_wav(audio: &CapturedAudio) -> Vec<u8> {
    let samples = &audio.samples;
    let sample_rate = audio.sample_rate;
    let channels = audio.channels as u32;
    let num_samples = samples.len() as u32;
    let data_size = num_samples * 2; // 16-bit = 2 bytes/sample
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity((file_size + 8) as usize);

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&(channels as u16).to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * channels * 2).to_le_bytes()); // ByteRate
    buf.extend_from_slice(&(channels as u16 * 2).to_le_bytes()); // BlockAlign
    buf.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    for s in samples {
        let v = (*s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

fn build_transcription_prompt(
    dictionary_words: &[String],
    focused_context: Option<&FocusedAppContext>,
    language_mode: LanguageMode,
) -> Option<String> {
    let mut sections = Vec::new();
    if !dictionary_words.is_empty() {
        sections.push(match language_mode {
            LanguageMode::Ja => format!(
                "次の専門用語・固有名詞を、音声と一致する場合は優先して認識してください: {}",
                dictionary_words.join(", ")
            ),
            LanguageMode::En => format!(
                "Prefer these terms and proper nouns when they match the audio: {}",
                dictionary_words.join(", ")
            ),
            LanguageMode::Auto => format!("terms={}", dictionary_words.join(", ")),
        });
    }
    if let Some(context) = focused_context {
        let mut lines = Vec::new();
        if !context.process_name.trim().is_empty() {
            let label = match language_mode {
                LanguageMode::Ja => "入力先アプリ",
                LanguageMode::En => "Target application",
                LanguageMode::Auto => "app",
            };
            lines.push(format!("{label}={}", context.process_name.trim()));
        }
        if !context.window_title.trim().is_empty() {
            let label = match language_mode {
                LanguageMode::Ja => "入力先ウィンドウタイトル",
                LanguageMode::En => "Target window title",
                LanguageMode::Auto => "window",
            };
            lines.push(format!("{label}={}", context.window_title.trim()));
        }
        if !lines.is_empty() {
            sections.push(lines.join("\n"));
        }
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n"))
    }
}

fn supports_file_streaming(model: &str) -> bool {
    matches!(
        model.trim(),
        "gpt-4o-transcribe" | "gpt-4o-mini-transcribe" | "gpt-4o-transcribe-diarize"
    )
}

pub fn batch_transcription_model(model: &str) -> String {
    if crate::speech::realtime::supports_realtime_model(model) {
        "gpt-4o-mini-transcribe".to_string()
    } else {
        model.to_string()
    }
}

fn transcription_text_fields(
    model: &str,
    language_mode: LanguageMode,
    prompt: Option<String>,
    streaming: bool,
) -> Vec<(&'static str, String)> {
    let mut fields = vec![
        ("model", model.to_string()),
        ("temperature", TRANSCRIPTION_TEMPERATURE.to_string()),
    ];
    if let Some(language) = language_mode.api_language() {
        fields.push(("language", language.to_string()));
    }
    if let Some(prompt) = prompt {
        fields.push(("prompt", prompt));
    }
    if streaming {
        fields.push(("stream", "true".to_string()));
        fields.push(("response_format", "text".to_string()));
    }
    fields
}

fn parse_transcript_event(line: &str) -> Option<(String, String)> {
    let payload = line.strip_prefix("data:").unwrap_or(line).trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    let event_type = json.get("type")?.as_str()?;
    match event_type {
        "transcript.text.delta" => json
            .get("delta")
            .and_then(|value| value.as_str())
            .map(|delta| (event_type.to_string(), delta.to_string())),
        "transcript.text.done" => json
            .get("text")
            .or_else(|| json.get("transcript"))
            .and_then(|value| value.as_str())
            .map(|text| (event_type.to_string(), text.to_string())),
        _ => None,
    }
}

#[async_trait::async_trait]
impl SpeechProvider for OpenAiCompatibleProvider {
    async fn transcribe(&self, audio: &CapturedAudio) -> anyhow::Result<String> {
        let wav = match &audio.wav_path {
            Some(path) => fs::read(path).context("ASR audio file read failed")?,
            None => encode_wav(audio),
        };
        self.transcribe_wav(wav, true, false).await
    }
}

impl OpenAiCompatibleProvider {
    pub async fn transcribe_dictation(
        &self,
        audio: &CapturedAudio,
        app: Option<&AppHandle>,
        recovery_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let Some(path) = audio.wav_path.as_deref() else {
            return self.transcribe(audio).await;
        };
        if !is_openai_endpoint(&self.base_url)
            || fs::metadata(path)?.len() as usize <= long_audio::OPENAI_SAFE_WAV_LIMIT
        {
            return self.transcribe(audio).await;
        }
        let info = long_audio::read_pcm_wav_info(path)?;
        let parts = long_audio::split_wav_parts(path, &info, long_audio::OPENAI_SAFE_WAV_LIMIT)?;
        let source_fingerprint = long_audio::fingerprint_file(path)?;
        let batch_model = batch_transcription_model(&self.model);
        let prompt = build_transcription_prompt(
            &self.dictionary_words,
            self.focused_context.as_ref(),
            self.language_mode,
        );
        let plan: Vec<(usize, usize)> = parts
            .iter()
            .map(|part| (part.start_frame, part.end_frame))
            .collect();
        let request_fingerprint =
            long_audio::fingerprint_text(&serde_json::to_string(&serde_json::json!({
                "version": 1,
                "endpoint": self.base_url.trim_end_matches('/'),
                "model": batch_model,
                "language": self.language_mode,
                "prompt": prompt,
                "plan": plan,
            }))?);
        let mut completed = match (app, recovery_id) {
            (Some(app), Some(id)) => recovery::load_long_audio_checkpoint(
                app,
                id,
                &source_fingerprint,
                &request_fingerprint,
                parts.len(),
            )?,
            _ => vec![None; parts.len()],
        };
        run_long_parts(
            &mut completed,
            |index| {
                let wav = long_audio::read_wav_part(path, &info, &parts[index]);
                async move { self.transcribe_wav(wav?, false, true).await }
            },
            |index, text| {
                if let (Some(app), Some(id)) = (app, recovery_id) {
                    recovery::save_long_audio_part(
                        app,
                        id,
                        source_fingerprint.clone(),
                        request_fingerprint.clone(),
                        parts.len(),
                        index,
                        text,
                    )
                } else {
                    Ok(())
                }
            },
            |current, total| emit_long_audio_progress(app, recovery_id, current, total),
        )
        .await
    }

    async fn transcribe_wav(
        &self,
        wav: Vec<u8>,
        streaming: bool,
        retry_transient: bool,
    ) -> anyhow::Result<String> {
        let client = if retry_transient {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(ASR_REQUEST_TIMEOUT_SECS))
                .build()?
        } else {
            reqwest::Client::new()
        };

        let batch_model = batch_transcription_model(&self.model);
        let prompt = build_transcription_prompt(
            &self.dictionary_words,
            self.focused_context.as_ref(),
            self.language_mode,
        );
        let streaming =
            streaming && supports_file_streaming(&batch_model) && self.partial_tx.is_some();
        let url = format!(
            "{}/audio/transcriptions",
            self.base_url.trim_end_matches('/')
        );
        let retry_count = if retry_transient {
            TRANSIENT_RETRIES
        } else {
            0
        };
        for attempt in 0..=retry_count {
            let part = Part::bytes(wav.clone())
                .file_name("audio.wav")
                .mime_str("audio/wav")?;
            let mut form = Form::new().part("file", part);
            for (name, value) in transcription_text_fields(
                &batch_model,
                self.language_mode,
                prompt.clone(),
                streaming,
            ) {
                form = form.text(name, value);
            }
            let resp = match client
                .post(&url)
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(error)
                    if attempt < retry_count && (error.is_timeout() || error.is_connect()) =>
                {
                    continue
                }
                Err(error) => return Err(error).context("ASR request failed"),
            };
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                if attempt < retry_count
                    && (status.is_server_error()
                        || (status.as_u16() == 429 && !body.contains("insufficient_quota")))
                {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                }
                anyhow::bail!("ASR API error {}: {}", status, body);
            }

            if streaming {
                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                let mut final_text = String::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.context("ASR stream read failed")?;
                    buffer.push_str(&String::from_utf8_lossy(&chunk));
                    while let Some(index) = buffer.find('\n') {
                        let line = buffer[..index].trim().to_string();
                        buffer = buffer[index + 1..].to_string();
                        let Some((event_type, text)) = parse_transcript_event(&line) else {
                            continue;
                        };
                        if event_type == "transcript.text.delta" {
                            if let Some(tx) = &self.partial_tx {
                                let _ = tx.send(text.clone());
                            }
                            final_text.push_str(&text);
                        } else if event_type == "transcript.text.done" {
                            final_text = text;
                        }
                    }
                }
                return Ok(final_text.trim().to_string());
            }

            let result: TranscriptionResponse =
                resp.json().await.context("ASR response parse failed")?;
            return Ok(result.text);
        }
        unreachable!("ASR retry loop always returns")
    }
}

fn join_transcript_parts(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

async fn run_long_parts<Request, RequestFuture, Save, Progress>(
    completed: &mut [Option<String>],
    mut request: Request,
    mut save: Save,
    mut progress: Progress,
) -> anyhow::Result<String>
where
    Request: FnMut(usize) -> RequestFuture,
    RequestFuture: Future<Output = anyhow::Result<String>>,
    Save: FnMut(usize, String) -> anyhow::Result<()>,
    Progress: FnMut(usize, usize),
{
    for index in 0..completed.len() {
        if completed[index].is_none() {
            let text = request(index).await?;
            save(index, text.clone())?;
            completed[index] = Some(text);
        }
        progress(index + 1, completed.len());
    }
    Ok(join_transcript_parts(completed.iter().flatten().cloned()))
}

fn is_openai_endpoint(base_url: &str) -> bool {
    base_url
        .trim_end_matches('/')
        .eq_ignore_ascii_case("https://api.openai.com/v1")
}

fn emit_long_audio_progress(
    app: Option<&AppHandle>,
    recovery_id: Option<&str>,
    current: usize,
    total: usize,
) {
    if let Some(app) = app {
        let _ = app.emit(
            "session://long-audio-progress",
            LongAudioProgressEvent {
                recovery_id: recovery_id.map(str::to_string),
                current,
                total,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        fs,
        sync::Arc,
    };

    #[test]
    fn transcription_prompt_uses_dictionary_and_context() {
        let context = FocusedAppContext {
            process_name: "notepad.exe".to_string(),
            window_title: "notes".to_string(),
        };
        let prompt =
            build_transcription_prompt(&["Obsidian".to_string()], Some(&context), LanguageMode::Ja)
                .expect("prompt should be generated");
        assert!(prompt.contains("次の専門用語・固有名詞"));
        assert!(prompt.contains("Obsidian"));
        assert!(prompt.contains("入力先アプリ"));
        assert!(prompt.contains("notepad.exe"));
        assert!(prompt.contains("入力先ウィンドウタイトル"));
        assert!(prompt.contains("notes"));
    }

    #[test]
    fn transcription_prompt_is_absent_when_empty() {
        assert!(build_transcription_prompt(&[], None, LanguageMode::Auto).is_none());
    }

    #[test]
    fn language_mode_maps_to_optional_batch_field() {
        assert_eq!(LanguageMode::Auto.api_language(), None);
        assert_eq!(LanguageMode::Ja.api_language(), Some("ja"));
        assert_eq!(LanguageMode::En.api_language(), Some("en"));
    }

    #[test]
    fn batch_request_fields_omit_auto_and_map_explicit_languages() {
        let auto =
            transcription_text_fields("gpt-4o-mini-transcribe", LanguageMode::Auto, None, false);
        assert!(!auto.iter().any(|(name, _)| *name == "language"));
        let ja = transcription_text_fields("whisper-1", LanguageMode::Ja, None, false);
        assert!(ja
            .iter()
            .any(|(name, value)| *name == "language" && value == "ja"));
        let en = transcription_text_fields("whisper-1", LanguageMode::En, None, false);
        assert!(en
            .iter()
            .any(|(name, value)| *name == "language" && value == "en"));
    }

    #[test]
    fn auto_prompt_does_not_force_a_language() {
        let prompt =
            build_transcription_prompt(&["KoeType".to_string()], None, LanguageMode::Auto).unwrap();
        assert!(!prompt.contains("日本語"));
        assert!(!prompt.contains("Japanese"));
    }

    #[test]
    fn english_prompt_has_no_japanese_labels() {
        let context = FocusedAppContext {
            process_name: "notepad.exe".to_string(),
            window_title: "notes".to_string(),
        };
        let prompt =
            build_transcription_prompt(&["KoeType".to_string()], Some(&context), LanguageMode::En)
                .unwrap();
        assert!(prompt.contains("Target application"));
        assert!(!prompt.contains("入力先"));
        assert!(!prompt.contains("日本語"));
    }

    #[test]
    fn detects_file_streaming_models() {
        assert!(supports_file_streaming("gpt-4o-transcribe"));
        assert!(supports_file_streaming("gpt-4o-mini-transcribe"));
        assert!(!supports_file_streaming("whisper-1"));
    }

    #[test]
    fn maps_realtime_model_to_batch_fallback_model() {
        assert_eq!(
            batch_transcription_model("gpt-realtime-whisper"),
            "gpt-4o-mini-transcribe"
        );
        assert_eq!(
            batch_transcription_model("gpt-4o-transcribe"),
            "gpt-4o-transcribe"
        );
    }

    #[test]
    fn parses_transcript_stream_events() {
        assert_eq!(
            parse_transcript_event(r#"data: {"type":"transcript.text.delta","delta":"hello"}"#),
            Some(("transcript.text.delta".to_string(), "hello".to_string()))
        );
        assert_eq!(
            parse_transcript_event(r#"data: {"type":"transcript.text.done","text":"hello world"}"#),
            Some((
                "transcript.text.done".to_string(),
                "hello world".to_string()
            ))
        );
        assert_eq!(parse_transcript_event("data: [DONE]"), None);
    }

    #[test]
    fn long_part_join_preserves_text_boundaries_and_empty_successes() {
        assert_eq!(
            join_transcript_parts(["hello".to_string(), "world".to_string()]),
            "hello\nworld"
        );
        assert_eq!(
            join_transcript_parts(["日本語".to_string(), String::new(), "続き".to_string()]),
            "日本語\n続き"
        );
    }

    #[tokio::test]
    async fn long_loop_persists_before_later_failure_and_resume_skips_saved_parts() {
        let path = std::env::temp_dir().join(format!("long-loop-{}.json", uuid::Uuid::new_v4()));
        let persisted = Arc::new(std::sync::Mutex::new(
            crate::recovery::LongAudioCheckpoint {
                source_fingerprint: "source".into(),
                request_fingerprint: "request".into(),
                total_parts: 3,
                completed_parts: vec![None, None, None],
            },
        ));
        let calls = RefCell::new(Vec::new());
        let fail_second = Cell::new(true);
        let first = run_long_parts(
            &mut [None, None, None],
            |index| {
                calls.borrow_mut().push(index);
                let fail = fail_second.get() && index == 1;
                async move {
                    if fail {
                        anyhow::bail!("permanent part failure")
                    } else {
                        Ok(["one", "", "three"][index].to_string())
                    }
                }
            },
            |index, text| {
                let mut checkpoint = persisted.lock().unwrap();
                checkpoint.completed_parts[index] = Some(text);
                let tmp = path.with_extension("tmp");
                fs::write(&tmp, serde_json::to_vec(&*checkpoint)?)?;
                fs::rename(tmp, &path)?;
                Ok(())
            },
            |_, _| {},
        )
        .await;
        assert!(first.is_err());
        assert_eq!(calls.into_inner(), vec![0, 1]);
        let restored: crate::recovery::LongAudioCheckpoint =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            restored.completed_parts,
            vec![Some("one".into()), None, None]
        );

        let calls = RefCell::new(Vec::new());
        fail_second.set(false);
        let mut resumed = restored.completed_parts;
        let final_text = run_long_parts(
            &mut resumed,
            |index| {
                calls.borrow_mut().push(index);
                async move { Ok(["one", "", "three"][index].to_string()) }
            },
            |index, text| {
                persisted.lock().unwrap().completed_parts[index] = Some(text);
                Ok(())
            },
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(calls.into_inner(), vec![1, 2]);
        assert_eq!(final_text, "one\nthree");

        let calls = RefCell::new(Vec::new());
        run_long_parts(
            &mut resumed,
            |index| {
                calls.borrow_mut().push(index);
                async move { Ok(String::new()) }
            },
            |_, _| Ok(()),
            |_, _| {},
        )
        .await
        .unwrap();
        assert!(calls.into_inner().is_empty());
        let _ = fs::remove_file(path);
    }
}
