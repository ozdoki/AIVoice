use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

use crate::audio::AudioChunk;

const REALTIME_SESSION_MODEL: &str = "gpt-realtime-2";
const REALTIME_SAMPLE_RATE: u32 = 24_000;

pub fn supports_realtime_model(model: &str) -> bool {
    matches!(model.trim(), "gpt-realtime-whisper")
}

fn realtime_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let path = format!("/realtime?model={REALTIME_SESSION_MODEL}");
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
    mut audio_rx: mpsc::UnboundedReceiver<AudioChunk>,
) -> Result<String, String> {
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
    let (ws, _) = connect_async(request)
        .await
        .map_err(|error| format!("Realtime connection failed: {error}"))?;
    let (mut write, mut read) = ws.split();

    let session_update = serde_json::json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": {
                "input": {
                    "format": {
                        "type": "audio/pcm",
                        "rate": REALTIME_SAMPLE_RATE
                    },
                    "transcription": {
                        "model": model
                    },
                    "turn_detection": null
                }
            }
        }
    });
    write
        .send(Message::Text(session_update.to_string().into()))
        .await
        .map_err(|error| format!("Realtime session update failed: {error}"))?;

    let sender = tokio::spawn(async move {
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
                return Err(format!("Realtime audio send failed: {error}"));
            }
        }

        let commit = serde_json::json!({ "type": "input_audio_buffer.commit" });
        write
            .send(Message::Text(commit.to_string().into()))
            .await
            .map_err(|error| format!("Realtime audio commit failed: {error}"))
    });

    let mut final_text = String::new();
    while let Some(message) = read.next().await {
        let message = message.map_err(|error| format!("Realtime receive failed: {error}"))?;
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
                    final_text.push_str(delta);
                }
            }
            Some("conversation.item.input_audio_transcription.completed") => {
                if let Some(transcript) = event.get("transcript").and_then(|value| value.as_str()) {
                    final_text = transcript.to_string();
                }
                break;
            }
            Some("error") => {
                return Err(event
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(|message| message.as_str())
                    .unwrap_or("Realtime API error")
                    .to_string());
            }
            _ => {}
        }
    }

    match sender.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(error) => return Err(format!("Realtime sender task failed: {error}")),
    }

    Ok(final_text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_url_maps_openai_rest_base_to_ws() {
        assert_eq!(
            realtime_url("https://api.openai.com/v1"),
            "wss://api.openai.com/v1/realtime?model=gpt-realtime-2"
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
}
