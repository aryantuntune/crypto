use crate::error::{AppError, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

const DESCRIBE_PROMPT: &str = "You are indexing this chart image for later retrieval. Output a single dense paragraph (no bullets, no JSON) describing: symbol/timeframe if visible, the pattern type, key indicator readings, conclusion the user might draw. Be specific and factual.";

#[derive(Deserialize)]
struct AnthropicNonStream {
    content: Vec<Block>,
}
#[derive(Deserialize)]
struct Block { #[allow(dead_code)] r#type: String, text: Option<String> }

pub async fn describe_image(api_key: &str, base_url: &str, model: &str, path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > 5 * 1024 * 1024 {
        return Err(AppError::Invalid(format!("image > 5MB: {}", path.display())));
    }
    let media_type = match path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return Err(AppError::Invalid("unsupported image type".into())),
    };
    let b64 = STANDARD.encode(&bytes);
    let body = json!({
        "model": model,
        "max_tokens": 600,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image", "source": {"type": "base64", "media_type": media_type, "data": b64}},
                {"type": "text", "text": DESCRIBE_PROMPT}
            ]
        }]
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", base_url))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send().await?;
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(AppError::Anthropic(format!("HTTP {}: {}", s, t)));
    }
    let parsed: AnthropicNonStream = resp.json().await?;
    let txt = parsed.content.into_iter().filter_map(|b| b.text).collect::<Vec<_>>().join("\n").trim().to_string();
    if txt.is_empty() { return Err(AppError::Anthropic("empty description".into())); }
    Ok(txt)
}
