use super::types::AnalysisJson;

/// Extract the last fenced ```json ...``` block from a text response and parse it.
pub fn extract_json_tail(text: &str) -> Option<AnalysisJson> {
    let needle_open = "```json";
    let last_open = text.rfind(needle_open)?;
    let after_open = &text[last_open + needle_open.len()..];
    let close = after_open.find("```")?;
    let body = after_open[..close].trim();
    let parsed: AnalysisJson = serde_json::from_str(body).ok()?;
    parsed.validate().ok()?;
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::*;

    #[test]
    fn extracts_well_formed_json() {
        let text = r#"
Some prose first.

```json
{
  "action": "buy",
  "probability_up": 0.65,
  "horizon": "4h",
  "stop_loss_pct": 2.5,
  "take_profit_pct": 5.0,
  "key_signals": ["bullish RSI divergence"],
  "citations": [{"doc": "wyckoff_book.pdf", "page": 42}]
}
```
"#;
        let a = extract_json_tail(text).unwrap();
        assert_eq!(a.action, Action::Buy);
        assert!((a.probability_up - 0.65).abs() < 1e-6);
        assert_eq!(a.horizon, Horizon::H4);
        assert_eq!(a.citations.len(), 1);
    }

    #[test]
    fn returns_none_when_missing() {
        assert!(extract_json_tail("just prose, no fence").is_none());
    }

    #[test]
    fn returns_none_on_invalid_probability() {
        let t = r#"```json
{"action":"buy","probability_up":1.5,"horizon":"4h"}
```"#;
        assert!(extract_json_tail(t).is_none());
    }

    #[test]
    fn picks_last_fence_when_multiple() {
        let t = r#"
```json
{"action":"hold","probability_up":0.5,"horizon":"1h"}
```
later
```json
{"action":"sell","probability_up":0.3,"horizon":"4h"}
```
"#;
        let a = extract_json_tail(t).unwrap();
        assert_eq!(a.action, Action::Sell);
    }
}
