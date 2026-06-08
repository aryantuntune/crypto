use crate::chat_store;
use crate::db::Db;
use crate::predictions;
use std::time::Duration;

pub fn spawn_background(db: Db) {
    let db_a = db.clone();
    tauri::async_runtime::spawn(async move {
        let interval = Duration::from_secs(6 * 3600);
        loop {
            if let Err(e) = chat_store::prune_older_than_secs(&db_a, 7 * 86400) {
                tracing::warn!("prune failed: {}", e);
            }
            tokio::time::sleep(interval).await;
        }
    });

    let db_b = db;
    tauri::async_runtime::spawn(async move {
        let interval = Duration::from_secs(60);
        loop {
            tokio::time::sleep(interval).await;
            let due = match predictions::pending_due(&db_b) { Ok(v) => v, Err(_) => continue };
            for p in due {
                let _ = predictions::resolve_one(&db_b, &p).await;
            }
        }
    });
}
