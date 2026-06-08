use cognitrade_lib::llm::{client::{LlmClient, AnalyzeArgs}, prompt::PromptInputs};
use tokio::sync::mpsc;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

fn fake_sse_body() -> String {
    let evts = [
        ("message_start", r#"{"type":"message_start","message":{"id":"x","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-6","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":100,"cache_creation_input_tokens":0,"cache_read_input_tokens":50,"output_tokens":0}}}"#),
        ("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#),
        ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Looks bullish.\n\n"}}"#),
        ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"```json\n{\"action\":\"buy\",\"probability_up\":0.7,\"horizon\":\"4h\",\"stop_loss_pct\":2,\"take_profit_pct\":4,\"key_signals\":[\"x\"],\"citations\":[]}\n```"}}"#),
        ("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        ("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":80}}"#),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ];
    let mut s = String::new();
    for (name, data) in evts {
        s.push_str(&format!("event: {}\ndata: {}\n\n", name, data));
    }
    s
}

#[tokio::test]
async fn analyze_streams_text_and_parses_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(fake_sse_body()))
        .mount(&server).await;

    let client = LlmClient::with_base(server.uri());
    let history = vec![];
    let chunks: Vec<String> = vec![];
    let inputs = PromptInputs {
        retrieved_chunks: &chunks,
        history: &history,
        user_text: "analyze this",
        image_b64: None,
    };
    let (tx, mut rx) = mpsc::channel::<String>(32);
    let out = client.analyze("sk-test", AnalyzeArgs { model: "claude-sonnet-4-6", max_tokens: 1500, inputs }, tx).await.unwrap();
    drop(rx.try_recv()); // drain (avoid unused warning by reading at least once)

    assert!(out.full_text.contains("Looks bullish"));
    let a = out.analysis.expect("json tail parsed");
    assert!((a.probability_up - 0.7).abs() < 1e-6);
    assert_eq!(out.cached_input_tokens, 50);
    assert_eq!(out.output_tokens, 80);
}
