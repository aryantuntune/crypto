pub mod error;
pub mod paths;
pub mod db;
pub mod chat_store;
pub mod settings;
pub mod cost;
pub mod llm;
pub mod ingest;
pub mod lance;
pub mod retrieval;
pub mod image_util;
pub mod coingecko;
pub mod predictions;
pub mod prune;
pub mod tray;
pub mod ipc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            tray::init_tray(&app.handle())?;

            // Build app state (no spawning here — safe in sync context)
            let paths = paths::Paths::from_env();
            paths.ensure_all().expect("create CogniTrade dirs");
            let db = db::open(&paths.chat_db()).expect("open chat db");
            let library = paths.library();
            let store_path = paths.lancedb();
            let state = ipc::AppState {
                db: db.clone(),
                lance: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
                paths,
                llm: crate::llm::client::LlmClient::new(),
            };
            app.manage(state);

            // Now that Tauri's runtime is up, spawn background tasks.
            crate::prune::spawn_background(db);

            // Warm up the embedding model off the async runtime so the first
            // analysis doesn't pay the (potentially download-bound) init cost.
            // Embedding is CPU/sync behind a global OnceLock, so a plain thread
            // is correct here and must not block Tauri's async runtime.
            std::thread::spawn(|| {
                if let Err(e) = crate::ingest::embeddings::warmup() {
                    tracing::warn!("embedding model warm-up failed: {}", e);
                }
            });
            tauri::async_runtime::spawn(async move {
                if let Ok(store) = lance::open(&store_path).await {
                    let _ = ingest::watcher::run_watcher(store, library);
                }
            });

            // Register global hotkey — tolerant of "already registered" from a prior crashed run
            let app_handle = app.handle().clone();
            use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
            if let Err(e) = app_handle.global_shortcut().on_shortcut(shortcut, move |app, _, ev| {
                if ev.state == ShortcutState::Pressed {
                    tray::toggle_panel(app);
                }
            }) {
                tracing::warn!("global shortcut handler registration: {}", e);
            }
            if let Err(e) = app_handle.global_shortcut().register(shortcut) {
                tracing::warn!("global shortcut registration (may already be registered): {}", e);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::list_recent_messages,
            ipc::get_settings,
            ipc::save_settings,
            ipc::get_api_key_set,
            ipc::set_api_key,
            ipc::clear_api_key,
            ipc::cost_today,
            ipc::save_screenshot,
            ipc::analyze,
            ipc::ingest_path,
            ipc::list_documents,
            ipc::delete_document,
            ipc::embeddings_ready,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
