use serde::Serialize;

pub const SYSTEM_PROMPT: &str = r#"You are CogniTrade, a personal crypto trading analyst. The user will share a chart screenshot. Your job:

1. Read the chart: identify symbol, timeframe, current price, key indicators, recent pattern.
2. Use the retrieved knowledge passages below as supporting context. Cite by document name (and page if PDF).
3. Output a brief plain-English explanation, then end your response with a fenced ```json block matching this schema:

{
  "action": "buy" | "sell" | "hold",
  "probability_up": <float 0..1>,
  "horizon": "1h" | "4h" | "1d" | "3d" | "1w",
  "stop_loss_pct": <float, e.g. 2.5>,
  "take_profit_pct": <float, e.g. 5.0>,
  "key_signals": [<short strings>],
  "citations": [{"doc": "<filename>", "page": <int or omit>}]
}

Rules:
- If the chart is unreadable or ambiguous, return action: "hold" and ask the user for a clearer image.
- probability_up MUST be in [0, 1].
- Be calibrated: probabilities should reflect realistic uncertainty, not advocacy.
- Cite at least one retrieved passage when you used it; if no retrieval was relevant, say so.
- Never recommend trades you can't justify with what's on the chart and the citations.
- You are an advisor. The user places all trades themselves.
"#;

#[derive(Debug, Clone, Serialize)]
pub struct ContentBlock<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSource<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageSource<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str, // "base64"
    pub media_type: &'a str, // "image/png"
    pub data: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheControl { #[serde(rename = "type")] pub kind: String }
impl CacheControl { pub fn ephemeral() -> Self { Self { kind: "ephemeral".into() } } }

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
}

pub struct PromptInputs<'a> {
    pub retrieved_chunks: &'a [String],
    pub history: &'a [ChatMessage],
    pub user_text: &'a str,
    pub image_b64: Option<&'a str>, // base64 PNG
}

pub fn build_messages(inputs: &PromptInputs) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    out.extend(inputs.history.iter().cloned());

    // Build the new user turn: retrieved context (cached) + image + text
    let mut blocks: Vec<serde_json::Value> = Vec::new();
    if !inputs.retrieved_chunks.is_empty() {
        let joined = format!("Retrieved knowledge:\n\n{}", inputs.retrieved_chunks.join("\n\n---\n\n"));
        blocks.push(serde_json::json!({
            "type": "text",
            "text": joined,
            "cache_control": {"type": "ephemeral"}
        }));
    }
    if let Some(b64) = inputs.image_b64 {
        blocks.push(serde_json::json!({
            "type": "image",
            "source": {"type": "base64", "media_type": "image/png", "data": b64}
        }));
    }
    blocks.push(serde_json::json!({"type": "text", "text": inputs.user_text}));

    out.push(ChatMessage {
        role: "user".into(),
        content: serde_json::Value::Array(blocks),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_user_turn_with_image_and_chunks() {
        let chunks = vec!["pattern A says X".to_string(), "pattern B says Y".to_string()];
        let history = vec![];
        let inputs = PromptInputs {
            retrieved_chunks: &chunks,
            history: &history,
            user_text: "what should I do?",
            image_b64: Some("ZmFrZQ=="),
        };
        let msgs = build_messages(&inputs);
        assert_eq!(msgs.len(), 1);
        let last = &msgs[0];
        assert_eq!(last.role, "user");
        let arr = last.content.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(arr[1]["type"], "image");
        assert_eq!(arr[2]["type"], "text");
        assert_eq!(arr[2]["text"], "what should I do?");
    }

    #[test]
    fn no_chunks_means_no_cached_block() {
        let inputs = PromptInputs {
            retrieved_chunks: &[],
            history: &[],
            user_text: "hi",
            image_b64: None,
        };
        let msgs = build_messages(&inputs);
        let arr = msgs[0].content.as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn includes_history_first() {
        let history = vec![
            ChatMessage { role: "user".into(), content: serde_json::json!("earlier q") },
            ChatMessage { role: "assistant".into(), content: serde_json::json!("earlier a") },
        ];
        let inputs = PromptInputs { retrieved_chunks: &[], history: &history, user_text: "now", image_b64: None };
        let msgs = build_messages(&inputs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "user");
    }
}
