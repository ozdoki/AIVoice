use crate::settings::AppSettings;

const SYSTEM_PROMPT: &str = "\
You are a post-processor for speech-to-text output. \
Your task is to clean up and improve the transcribed text while preserving the original meaning. \
Fix grammar, punctuation, and formatting. \
Remove filler words and false starts. \
Output ONLY the improved text without any explanation or commentary.";

pub async fn polish_text(settings: &AppSettings, text: &str) -> anyhow::Result<String> {
    if settings.api_key.trim().is_empty() || settings.polish_model.trim().is_empty() {
        return Ok(text.to_string());
    }

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", settings.api_base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": settings.polish_model,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
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
