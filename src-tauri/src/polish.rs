use crate::{context, context::FocusedAppContext, settings::AppSettings};

const SYSTEM_PROMPT: &str = "\
You are a post-processor for speech-to-text output. \
Your task is to clean up and improve the transcribed text while preserving the original meaning. \
Fix grammar, punctuation, and formatting. \
Remove filler words and false starts. \
Output ONLY the improved text without any explanation or commentary.";

fn build_system_prompt(
    base_prompt: &str,
    custom_instructions: &str,
    dictionary_words: &[String],
    focused_context: Option<&FocusedAppContext>,
) -> String {
    let mut sections = vec![base_prompt.trim().to_string()];
    let custom = custom_instructions.trim();
    if !custom.is_empty() {
        sections.push(format!("User custom instructions:\n{custom}"));
    }
    if !dictionary_words.is_empty() {
        sections.push(format!(
            "Preferred custom words and proper nouns:\n{}",
            dictionary_words.join(", ")
        ));
    }
    let context_prompt = context::prompt_fragment(focused_context);
    if !context_prompt.is_empty() {
        sections.push(format!("Current input context:\n{context_prompt}"));
    }
    sections.push("Never include explanations, annotations, or confirmation text.".to_string());
    sections.join("\n\n")
}

pub async fn polish_text(
    settings: &AppSettings,
    dictionary_words: &[String],
    focused_context: Option<&FocusedAppContext>,
    text: &str,
) -> anyhow::Result<String> {
    if settings.api_key.trim().is_empty() || settings.polish_model.trim().is_empty() {
        return Ok(text.to_string());
    }

    let client = reqwest::Client::new();
    let url = format!(
        "{}/chat/completions",
        settings.api_base_url.trim_end_matches('/')
    );

    let system_prompt = build_system_prompt(
        SYSTEM_PROMPT,
        &settings.custom_polish_instructions,
        dictionary_words,
        focused_context,
    );

    let body = serde_json::json!({
        "model": settings.polish_model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": text }
        ],
        "temperature": 0.3,
        "max_tokens": 1024
    });

    let resp = client
        .post(&url)
        .bearer_auth(&settings.api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let msg = resp.text().await.unwrap_or_default();
        anyhow::bail!("polish API error {status}: {msg}");
    }

    let json: serde_json::Value = resp.json().await?;
    let polished = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(polished)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_custom_dictionary_and_context() {
        let context = FocusedAppContext {
            process_name: "Slack.exe".to_string(),
            window_title: "general".to_string(),
        };
        let prompt = build_system_prompt(
            SYSTEM_PROMPT,
            "短く自然にする",
            &["Obsidian".to_string(), "AIVoice".to_string()],
            Some(&context),
        );
        assert!(prompt.contains("短く自然にする"));
        assert!(prompt.contains("Obsidian"));
        assert!(prompt.contains("Slack.exe"));
        assert!(prompt.contains("Never include explanations"));
    }
}
