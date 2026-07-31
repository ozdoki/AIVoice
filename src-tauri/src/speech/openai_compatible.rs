use std::fs;

use anyhow::Context;
use futures_util::StreamExt;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use tokio::sync::mpsc;

use super::SpeechProvider;
use crate::{audio::CapturedAudio, context::FocusedAppContext, settings::LanguageMode};

const TRANSCRIPTION_TEMPERATURE: &str = "0";

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
        "gpt-transcribe"
            | "gpt-4o-transcribe"
            | "gpt-4o-mini-transcribe"
            | "gpt-4o-transcribe-diarize"
    )
}

fn uses_gpt_transcribe_contract(model: &str) -> bool {
    model.trim() == "gpt-transcribe"
}

fn safe_gpt_transcribe_keywords(dictionary_words: &[String]) -> Vec<String> {
    let mut keywords = Vec::new();
    let mut chars = 0_usize;
    for word in dictionary_words {
        let word = word.trim();
        let word_chars = word.chars().count();
        if word.is_empty()
            || word.contains(['\r', '\n', '<', '>'])
            || keywords.len() >= crate::corrections::MAX_EFFECTIVE_VOCABULARY
            || chars.saturating_add(word_chars) > crate::corrections::MAX_EFFECTIVE_VOCABULARY_CHARS
        {
            continue;
        }
        chars += word_chars;
        keywords.push(word.to_string());
    }
    keywords
}

fn transcription_prompt(
    model: &str,
    dictionary_words: &[String],
    focused_context: Option<&FocusedAppContext>,
    language_mode: LanguageMode,
) -> Option<String> {
    let prompt_words = if uses_gpt_transcribe_contract(model) {
        &[]
    } else {
        dictionary_words
    };
    build_transcription_prompt(prompt_words, focused_context, language_mode)
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
    dictionary_words: &[String],
) -> Vec<(&'static str, String)> {
    let mut fields = vec![
        ("model", model.to_string()),
        ("temperature", TRANSCRIPTION_TEMPERATURE.to_string()),
    ];
    if let Some(language) = language_mode.api_language() {
        let field_name = if uses_gpt_transcribe_contract(model) {
            "languages[]"
        } else {
            "language"
        };
        fields.push((field_name, language.to_string()));
    }
    if uses_gpt_transcribe_contract(model) {
        fields.extend(
            safe_gpt_transcribe_keywords(dictionary_words)
                .into_iter()
                .map(|keyword| ("keywords[]", keyword)),
        );
    }
    if let Some(prompt) = prompt {
        fields.push(("prompt", prompt));
    }
    if streaming {
        fields.push(("stream", "true".to_string()));
        if !uses_gpt_transcribe_contract(model) {
            fields.push(("response_format", "text".to_string()));
        }
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

fn take_complete_sse_lines(buffer: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    for index in 0..buffer.len() {
        if buffer[index] == b'\n' {
            lines.push(
                String::from_utf8_lossy(&buffer[start..index])
                    .trim()
                    .to_string(),
            );
            start = index + 1;
        }
    }
    if start > 0 {
        buffer.drain(..start);
    }
    lines
}

#[async_trait::async_trait]
impl SpeechProvider for OpenAiCompatibleProvider {
    async fn transcribe(&self, audio: &CapturedAudio) -> anyhow::Result<String> {
        let wav = match &audio.wav_path {
            Some(path) => fs::read(path).context("ASR audio file read failed")?,
            None => encode_wav(audio),
        };
        let client = reqwest::Client::new();

        let part = Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;
        let batch_model = batch_transcription_model(&self.model);
        let prompt = transcription_prompt(
            &batch_model,
            &self.dictionary_words,
            self.focused_context.as_ref(),
            self.language_mode,
        );
        let streaming = supports_file_streaming(&batch_model) && self.partial_tx.is_some();
        let mut form = Form::new().part("file", part);
        for (name, value) in transcription_text_fields(
            &batch_model,
            self.language_mode,
            prompt,
            streaming,
            &self.dictionary_words,
        ) {
            form = form.text(name, value);
        }

        let url = format!(
            "{}/audio/transcriptions",
            self.base_url.trim_end_matches('/')
        );

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

        if supports_file_streaming(&batch_model) && self.partial_tx.is_some() {
            let mut stream = resp.bytes_stream();
            let mut buffer = Vec::new();
            let mut final_text = String::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.context("ASR stream read failed")?;
                buffer.extend_from_slice(&chunk);
                for line in take_complete_sse_lines(&mut buffer) {
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
        Ok(result.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let auto = transcription_text_fields(
            "gpt-4o-mini-transcribe",
            LanguageMode::Auto,
            None,
            false,
            &[],
        );
        assert!(!auto.iter().any(|(name, _)| *name == "language"));
        let ja = transcription_text_fields("whisper-1", LanguageMode::Ja, None, false, &[]);
        assert!(ja
            .iter()
            .any(|(name, value)| *name == "language" && value == "ja"));
        let en = transcription_text_fields("whisper-1", LanguageMode::En, None, false, &[]);
        assert!(en
            .iter()
            .any(|(name, value)| *name == "language" && value == "en"));
    }

    #[test]
    fn gpt_transcribe_uses_languages_and_safe_keyword_fields() {
        let words = vec![
            " KoeType ".to_string(),
            String::new(),
            "bad\nword".to_string(),
            "<script>".to_string(),
            "Obsidian".to_string(),
        ];
        let auto = transcription_text_fields(
            "gpt-transcribe",
            LanguageMode::Auto,
            Some("app=notepad.exe".to_string()),
            false,
            &words,
        );
        assert!(!auto
            .iter()
            .any(|(name, _)| *name == "language" || *name == "languages[]"));
        assert_eq!(
            auto.iter()
                .filter(|(name, _)| *name == "keywords[]")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            vec!["KoeType", "Obsidian"]
        );
        assert!(auto
            .iter()
            .any(|(name, value)| *name == "prompt" && value == "app=notepad.exe"));

        let ja = transcription_text_fields("gpt-transcribe", LanguageMode::Ja, None, false, &[]);
        assert!(ja
            .iter()
            .any(|(name, value)| *name == "languages[]" && value == "ja"));
        assert!(!ja.iter().any(|(name, _)| *name == "language"));

        let en = transcription_text_fields("gpt-transcribe", LanguageMode::En, None, false, &[]);
        assert!(en
            .iter()
            .any(|(name, value)| *name == "languages[]" && value == "en"));
    }

    #[test]
    fn gpt_transcribe_keeps_dictionary_words_out_of_prompt() {
        let context = FocusedAppContext {
            process_name: "notepad.exe".to_string(),
            window_title: String::new(),
        };
        let prompt = transcription_prompt(
            "gpt-transcribe",
            &["KoeType".to_string()],
            Some(&context),
            LanguageMode::Auto,
        )
        .unwrap();
        assert!(prompt.contains("app=notepad.exe"));
        assert!(!prompt.contains("KoeType"));
    }

    #[test]
    fn gpt_transcribe_keywords_respect_effective_vocabulary_limit() {
        let words = (0..crate::corrections::MAX_EFFECTIVE_VOCABULARY)
            .map(|index| format!("term-{index}"))
            .chain(std::iter::once("overflow".to_string()))
            .collect::<Vec<_>>();
        let keywords = safe_gpt_transcribe_keywords(&words);
        assert_eq!(keywords.len(), crate::corrections::MAX_EFFECTIVE_VOCABULARY);
        assert!(!keywords.iter().any(|keyword| keyword == "overflow"));
    }

    #[test]
    fn gpt_transcribe_streaming_omits_incompatible_response_format() {
        let fields =
            transcription_text_fields("gpt-transcribe", LanguageMode::Auto, None, true, &[]);
        assert!(fields
            .iter()
            .any(|(name, value)| *name == "stream" && value == "true"));
        assert!(!fields.iter().any(|(name, _)| *name == "response_format"));

        let legacy = transcription_text_fields(
            "gpt-4o-mini-transcribe",
            LanguageMode::Auto,
            None,
            true,
            &[],
        );
        assert!(legacy
            .iter()
            .any(|(name, value)| *name == "response_format" && value == "text"));
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
        assert!(supports_file_streaming("gpt-transcribe"));
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
    fn sse_line_buffer_preserves_utf8_split_across_chunks() {
        let event = "data: {\"type\":\"transcript.text.delta\",\"delta\":\"日本語\"}\n";
        let bytes = event.as_bytes();
        let split = bytes
            .windows("日".len())
            .position(|window| window == "日".as_bytes())
            .unwrap()
            + 1;
        let mut buffer = bytes[..split].to_vec();
        assert!(take_complete_sse_lines(&mut buffer).is_empty());

        buffer.extend_from_slice(&bytes[split..]);
        let lines = take_complete_sse_lines(&mut buffer);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            parse_transcript_event(&lines[0]),
            Some(("transcript.text.delta".to_string(), "日本語".to_string()))
        );
        assert!(buffer.is_empty());
    }
}
