use crate::chat_store::{self, Message, NewMessage};
use crate::cost::{self, DailyCost};
use crate::db::Db;
use crate::error::{AppError, Result};
use crate::ingest::{ingest_pdf, ingest_image};
use crate::lance::{self, LanceStore};
use crate::llm::client::{LlmClient, analyze_with_cap};
use crate::llm::prompt::{ChatMessage, PromptInputs};
use crate::predictions;
use crate::retrieval;
use crate::settings::{self, Settings};
use crate::paths::Paths;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, mpsc};

/// Tiny non-streaming Haiku call: given an image, return the trading symbol.
async fn extract_symbol_from_image(api_key: &str, model: &str, image_b64: &str, media_type: &str) -> Result<String> {
    let body = json!({
        "model": model,
        "max_tokens": 40,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image", "source": {"type": "base64", "media_type": media_type, "data": image_b64}},
                {"type": "text", "text": "What trading symbol is shown? Answer with ONLY the ticker (e.g. BTCUSDT, ETH, SOLUSDT). If unclear, answer UNKNOWN."}
            ]
        }]
    });
    let resp = reqwest::Client::new()
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::Anthropic(format!("extract HTTP {}", resp.status())));
    }
    #[derive(serde::Deserialize)] struct R { content: Vec<B> }
    #[derive(serde::Deserialize)] struct B { text: Option<String> }
    let parsed: R = resp.json().await?;
    let txt = parsed.content.into_iter().filter_map(|b| b.text).collect::<Vec<_>>().join(" ").trim().to_string();
    let token = txt.split_whitespace().next().unwrap_or("").to_uppercase();
    if token.is_empty() || token == "UNKNOWN" {
        return Err(AppError::Invalid("could not extract symbol".into()));
    }
    Ok(token)
}

pub struct AppState {
    pub db: Db,
    pub lance: Arc<Mutex<Option<LanceStore>>>,
    pub paths: Paths,
    pub llm: LlmClient,
}

async fn lance_get_or_open(state: &AppState) -> Result<LanceStore> {
    let g = state.lance.lock().await;
    if let Some(s) = g.as_ref() {
        return Ok(s.clone());
    }
    drop(g);
    let store = lance::open(&state.paths.lancedb()).await?;
    let mut g = state.lance.lock().await;
    *g = Some(store.clone());
    Ok(store)
}

#[tauri::command]
pub async fn list_recent_messages(state: State<'_, AppState>, limit: i64) -> Result<Vec<Message>> {
    chat_store::recent(&state.db, limit)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    settings::load(&state.db)
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, value: Settings) -> Result<()> {
    settings::save(&state.db, &value)
}

#[tauri::command]
pub async fn get_api_key_set() -> Result<bool> {
    Ok(settings::get_api_key()?.is_some())
}

#[tauri::command]
pub async fn set_api_key(value: String) -> Result<()> {
    if value.trim().is_empty() { return Err(AppError::Invalid("empty api key".into())); }
    settings::set_api_key(&value)
}

#[tauri::command]
pub async fn clear_api_key() -> Result<()> { settings::clear_api_key() }

#[tauri::command]
pub async fn cost_today(state: State<'_, AppState>) -> Result<DailyCost> { cost::today(&state.db) }

#[tauri::command]
pub async fn save_screenshot(state: State<'_, AppState>, base64_png: String, filename_hint: Option<String>) -> Result<String> {
    let bytes = STANDARD.decode(base64_png.as_bytes())
        .map_err(|e| AppError::Invalid(format!("base64: {}", e)))?;
    let id = uuid::Uuid::new_v4().to_string();
    let stem = filename_hint.unwrap_or_else(|| id.clone());
    let path = state.paths.screenshots().join(format!("{}-{}.png", stem, id));
    std::fs::create_dir_all(state.paths.screenshots())?;
    std::fs::write(&path, &bytes)?;
    Ok(path.to_string_lossy().to_string())
}

#[derive(Deserialize)]
pub struct AnalyzeReq {
    pub user_text: String,
    pub screenshot_path: Option<String>,
    pub symbol_hint: Option<String>,
}
#[derive(Serialize)]
pub struct AnalyzeAck { pub message_id: i64 }

#[tauri::command]
pub async fn analyze(app: AppHandle, state: State<'_, AppState>, req: AnalyzeReq) -> Result<AnalyzeAck> {
    // Persist user message first
    let user_id = chat_store::insert(&state.db, NewMessage {
        role: "user".into(),
        content: req.user_text.clone(),
        image_path: req.screenshot_path.clone(),
        prediction_id: None,
    })?;

    // Resize image if needed
    if let Some(p) = req.screenshot_path.as_deref() {
        let pp = PathBuf::from(p);
        if crate::image_util::needs_resize(&pp).unwrap_or(false) {
            crate::image_util::resize_in_place(&pp)?;
        }
    }

    let history = chat_store::recent(&state.db, 10)?
        .into_iter()
        .filter(|m| m.id != user_id)
        .map(|m| ChatMessage { role: m.role, content: serde_json::Value::String(m.content) })
        .collect::<Vec<_>>();

    let store = lance_get_or_open(&state).await?;

    let image_b64 = if let Some(p) = req.screenshot_path.as_deref() {
        let bytes = std::fs::read(p)?;
        Some(STANDARD.encode(&bytes))
    } else { None };

    let s = settings::load(&state.db)?;

    // Resolve symbol: prefer user hint; otherwise extract from screenshot via Haiku.
    let symbol = match (req.symbol_hint.clone(), image_b64.as_deref()) {
        (Some(h), _) if !h.trim().is_empty() => h.trim().to_uppercase(),
        (_, Some(b64)) => {
            let key = settings::require_api_key()?;
            extract_symbol_from_image(&key, &s.model_extract, b64, "image/png")
                .await
                .unwrap_or_else(|_| "BTCUSDT".into())
        }
        _ => "BTCUSDT".into(),
    };

    let q = format!("{} chart pattern analysis", symbol);
    let chunks = retrieval::search(&store, &q, 6).await?;
    let chunk_texts: Vec<String> = chunks.into_iter().map(|c| c.text).collect();

    let inputs = PromptInputs {
        retrieved_chunks: &chunk_texts,
        history: &history,
        user_text: &req.user_text,
        image_b64: image_b64.as_deref(),
    };

    let model = s.model_main.clone();
    let (tx, mut rx) = mpsc::channel::<String>(64);

    let app2 = app.clone();
    let stream_task = tokio::spawn(async move {
        while let Some(t) = rx.recv().await {
            let _ = app2.emit("analysis_chunk", &t);
        }
    });

    let out = analyze_with_cap(&state.db, &state.llm, &model, inputs, tx).await?;
    let _ = stream_task.await;

    let mut prediction_id: Option<i64> = None;
    if let Some(a) = out.analysis.as_ref() {
        let id = predictions::insert_from_analysis(&state.db, &symbol, a)?;
        prediction_id = Some(id);
        // record price now (fire-and-forget)
        let symbol2 = symbol.clone();
        let db2 = state.db.clone();
        tokio::spawn(async move {
            if let Ok(p) = crate::coingecko::get_usd_price(&symbol2).await {
                let _ = predictions::record_initial_price(&db2, id, p);
            }
        });
    }

    let asst_id = chat_store::insert(&state.db, NewMessage {
        role: "assistant".into(),
        content: out.full_text.clone(),
        image_path: None,
        prediction_id,
    })?;

    let _ = app.emit("analysis_done", serde_json::json!({
        "message_id": asst_id,
        "analysis": out.analysis,
    }));

    Ok(AnalyzeAck { message_id: asst_id })
}

#[tauri::command]
pub async fn list_documents(state: State<'_, AppState>) -> Result<Vec<crate::lance::DocInfo>> {
    let store = lance_get_or_open(&state).await?;
    store.list_docs().await
}

#[tauri::command]
pub async fn delete_document(state: State<'_, AppState>, doc_path: String) -> Result<()> {
    let store = lance_get_or_open(&state).await?;
    store.delete_by_doc_path(&doc_path).await
}

#[tauri::command]
pub async fn embeddings_ready() -> Result<bool> {
    Ok(crate::ingest::embeddings::is_ready())
}

#[tauri::command]
pub async fn ingest_path(state: State<'_, AppState>, path: String) -> Result<usize> {
    let p = PathBuf::from(&path);
    let store = lance_get_or_open(&state).await?;
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let report = match ext.as_str() {
        "pdf" => ingest_pdf(&store, &p).await?,
        "png" | "jpg" | "jpeg" | "webp" => {
            let key = settings::require_api_key()?;
            let s = settings::load(&state.db)?;
            ingest_image(&store, &key, "https://api.anthropic.com", &s.model_extract, &p).await?
        }
        _ => return Err(AppError::Invalid(format!("unsupported extension: {}", ext))),
    };
    Ok(report.chunks)
}
