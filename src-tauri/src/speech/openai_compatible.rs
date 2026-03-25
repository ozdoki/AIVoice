use anyhow::Context;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use crate::audio::CapturedAudio;
use super::SpeechProvider;

pub struct OpenAiCompatibleProvider {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
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

#[async_trait::async_trait]
impl SpeechProvider for OpenAiCompatibleProvider {
    async fn transcribe(&self, audio: &CapturedAudio) -> anyhow::Result<String> {
        let wav = encode_wav(audio);
        let client = reqwest::Client::new();

        let part = Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;
        let form = Form::new()
            .part("file", part)
            .text("model", self.model.clone());

        let url = format!("{}/audio/transcriptions", self.base_url.trim_end_matches('/'));

        let resp = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .context("ASR request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("ASR API error {}: {}", status, body);
        }

        let result: TranscriptionResponse =
            resp.json().await.context("ASR response parse failed")?;
        Ok(result.text)
    }
}
