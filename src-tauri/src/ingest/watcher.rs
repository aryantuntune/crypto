use crate::error::Result;
use crate::lance::LanceStore;
use crate::ingest::ingest_pdf;
use notify::{EventKind, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use std::collections::HashMap;

pub fn run_watcher(store: LanceStore, library: PathBuf) -> Result<()> {
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }).map_err(|e| crate::error::AppError::Internal(format!("watcher init: {}", e)))?;
    watcher.watch(&library, RecursiveMode::Recursive)
        .map_err(|e| crate::error::AppError::Internal(format!("watcher watch: {}", e)))?;

    let store = std::sync::Arc::new(store);
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    let debounce = Duration::from_millis(800);
    loop {
        if let Ok(Ok(event)) = rx.recv_timeout(Duration::from_millis(200)) {
            if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                for p in event.paths {
                    pending.insert(p, Instant::now());
                }
            }
        }
        let now = Instant::now();
        let ready: Vec<PathBuf> = pending.iter()
            .filter(|(_, t)| now.duration_since(**t) >= debounce)
            .map(|(p, _)| p.clone())
            .collect();
        for p in ready {
            pending.remove(&p);
            if !p.is_file() { continue; }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if ext == "pdf" {
                let store = store.clone();
                tokio::spawn(async move {
                    match ingest_pdf(&store, &p).await {
                        Ok(r) => tracing::info!("ingested {}: {} chunks", r.doc_path, r.chunks),
                        Err(e) => tracing::warn!("ingest failed {}: {}", p.display(), e),
                    }
                });
            } else if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                let store = store.clone();
                tokio::spawn(async move {
                    let key = match crate::settings::require_api_key() { Ok(k) => k, Err(_) => return };
                    let model = "claude-haiku-4-5-20251001"; // image describe uses cheap model
                    let res = crate::ingest::ingest_image(&store, &key, "https://api.anthropic.com", model, &p).await;
                    match res {
                        Ok(r) => tracing::info!("ingested image {}: {} chunks", r.doc_path, r.chunks),
                        Err(e) => tracing::warn!("ingest image failed {}: {}", p.display(), e),
                    }
                });
            }
        }
    }
}
