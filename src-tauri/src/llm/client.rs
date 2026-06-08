use crate::cost;
use crate::db::Db;
use crate::error::{AppError, Result};
use crate::llm::prompt::{ChatMessage, PromptInputs, build_messages, SYSTEM_PROMPT};
use crate::llm::streaming::{parse_event, StreamEvent};
use crate::llm::parse::extract_json_tail;
use crate::llm::types::AnalysisJson;
use crate::settings;
use futures::StreamExt;
use reqwest::Client;
use serde::Serialize;
use tokio::sync::mpsc;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

#[derive(Debug, Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    stream: bool,
    system: Vec<serde_json::Value>,
    messages: &'a [ChatMessage],
}

#[derive(Clone)]
pub struct LlmClient {
    pub base_url: String,
    pub http: Client,
}

impl LlmClient {
    pub fn new() -> Self {
        Self { base_url: DEFAULT_BASE_URL.into(), http: Client::new() }
    }
    pub fn with_base(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), http: Client::new() }
    }
}

pub struct AnalyzeArgs<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub inputs: PromptInputs<'a>,
}

pub struct AnalyzeOutput {
    pub full_text: String,
    pub analysis: Option<AnalysisJson>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

impl LlmClient {
    /// Streaming analyze. Sends text chunks to `tx` as they arrive.
    /// Final returned struct contains the assembled text + parsed JSON tail + usage.
    pub async fn analyze(
        &self,
        api_key: &str,
        args: AnalyzeArgs<'_>,
        tx: mpsc::Sender<String>,
    ) -> Result<AnalyzeOutput> {
        let messages = build_messages(&args.inputs);
        let req = AnthropicRequest {
            model: args.model,
            max_tokens: args.max_tokens,
            stream: true,
            system: vec![serde_json::json!({
                "type": "text",
                "text": SYSTEM_PROMPT,
                "cache_control": {"type": "ephemeral"}
            })],
            messages: &messages,
        };
        let url = format!("{}/v1/messages", self.base_url);
        let resp = self.http.post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req)
            .send().await?;
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Anthropic(format!("HTTP {}: {}", s, body)));
        }

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut full = String::new();
        let mut input_tokens = 0u64;
        let mut cached = 0u64;
        let mut output_tokens = 0u64;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buf.push_str(&String::from_utf8_lossy(&bytes));
            // Parse line by line, splitting on \n\n event boundaries
            while let Some(boundary) = buf.find("\n\n") {
                let event_block = buf[..boundary].to_string();
                buf.drain(..boundary + 2);
                let mut event_name = String::from("message");
                let mut data = String::new();
                for line in event_block.lines() {
                    if let Some(rest) = line.strip_prefix("event: ") {
                        event_name = rest.trim().to_string();
                    } else if let Some(rest) = line.strip_prefix("data: ") {
                        data.push_str(rest);
                    }
                }
                if data.is_empty() { continue; }
                if let Some(ev) = parse_event(&event_name, &data) {
                    match ev {
                        StreamEvent::Text(t) => {
                            full.push_str(&t);
                            let _ = tx.send(t).await;
                        }
                        StreamEvent::Usage { input_tokens: i, cached_input_tokens: c, output_tokens: o } => {
                            // message_start gives initial counts; message_delta gives final output_tokens
                            if i > 0 { input_tokens = i; }
                            if c > 0 { cached = c; }
                            if o > 0 { output_tokens = o; }
                        }
                        StreamEvent::Done => {}
                    }
                }
            }
        }

        let analysis = extract_json_tail(&full);
        Ok(AnalyzeOutput { full_text: full, analysis, input_tokens, cached_input_tokens: cached, output_tokens })
    }
}

/// High-level: fetch key, run analyze, record cost, enforce cap.
pub async fn analyze_with_cap(
    db: &Db,
    client: &LlmClient,
    model: &str,
    inputs: PromptInputs<'_>,
    tx: mpsc::Sender<String>,
) -> Result<AnalyzeOutput> {
    let s = settings::load(db)?;
    let today = cost::today(db)?;
    if cost::is_over_cap(today.cost_usd, s.daily_cost_cap_usd) {
        return Err(AppError::CostCapReached);
    }
    let key = settings::require_api_key()?;
    let out = client.analyze(&key, AnalyzeArgs { model, max_tokens: 1500, inputs }, tx).await?;
    cost::add_usage(db, model, out.input_tokens, out.cached_input_tokens, out.output_tokens)?;
    Ok(out)
}
