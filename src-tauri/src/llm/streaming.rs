use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Text(String),
    Usage { input_tokens: u64, cached_input_tokens: u64, output_tokens: u64 },
    Done,
}

/// Anthropic SSE message_start.message.usage
#[derive(Debug, Deserialize)]
struct MessageStart {
    message: MessageStartInner,
}
#[derive(Debug, Deserialize)]
struct MessageStartInner {
    usage: Option<UsageRaw>,
}

#[derive(Debug, Deserialize, Default)]
struct UsageRaw {
    #[serde(default)] input_tokens: u64,
    #[serde(default)] cache_creation_input_tokens: u64,
    #[serde(default)] cache_read_input_tokens: u64,
    #[serde(default)] output_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct MessageDelta {
    usage: UsageRaw,
}

#[derive(Debug, Deserialize)]
struct ContentBlockDelta { delta: BlockDelta }

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum BlockDelta {
    #[serde(rename = "text_delta")] TextDelta { text: String },
    #[serde(other)] Other,
}

/// Parse a single Anthropic SSE event (the JSON object after `data: `).
/// Returns Some(event) when the line yields a recognized event, None otherwise.
pub fn parse_event(event_name: &str, data_json: &str) -> Option<StreamEvent> {
    match event_name {
        "content_block_delta" => {
            let cbd: ContentBlockDelta = serde_json::from_str(data_json).ok()?;
            match cbd.delta {
                BlockDelta::TextDelta { text } => Some(StreamEvent::Text(text)),
                _ => None,
            }
        }
        "message_start" => {
            let m: MessageStart = serde_json::from_str(data_json).ok()?;
            let u = m.message.usage?;
            // Treat cache_read as cached input; cache_creation also counts as input but is billed separately;
            // we sum cache_creation into input_tokens for a conservative cost estimate.
            Some(StreamEvent::Usage {
                input_tokens: u.input_tokens + u.cache_creation_input_tokens,
                cached_input_tokens: u.cache_read_input_tokens,
                output_tokens: u.output_tokens,
            })
        }
        "message_delta" => {
            let m: MessageDelta = serde_json::from_str(data_json).ok()?;
            Some(StreamEvent::Usage {
                input_tokens: m.usage.input_tokens + m.usage.cache_creation_input_tokens,
                cached_input_tokens: m.usage.cache_read_input_tokens,
                output_tokens: m.usage.output_tokens,
            })
        }
        "message_stop" => Some(StreamEvent::Done),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta() {
        let e = parse_event("content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#).unwrap();
        assert_eq!(e, StreamEvent::Text("hello".into()));
    }

    #[test]
    fn parses_message_start_usage() {
        let e = parse_event("message_start",
            r#"{"type":"message_start","message":{"id":"x","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-6","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":120,"cache_creation_input_tokens":10,"cache_read_input_tokens":50,"output_tokens":0}}}"#).unwrap();
        assert!(matches!(e, StreamEvent::Usage { input_tokens: 130, cached_input_tokens: 50, output_tokens: 0 }));
    }

    #[test]
    fn parses_message_stop() {
        assert_eq!(parse_event("message_stop", r#"{"type":"message_stop"}"#), Some(StreamEvent::Done));
    }

    #[test]
    fn ignores_unknown_event() {
        assert!(parse_event("ping", "{}").is_none());
    }
}
