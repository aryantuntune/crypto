use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub type Db = Arc<Mutex<Connection>>;

const MIGRATION_001: &str = include_str!("../migrations/001_initial.sql");

pub fn open(path: &Path) -> Result<Db> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(MIGRATION_001)?;
    Ok(Arc::new(Mutex::new(conn)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_migrates() {
        let tmp = tempfile::tempdir().unwrap();
        let db = open(&tmp.path().join("chat.db")).unwrap();
        let conn = db.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('messages','predictions','cost_daily','settings')",
            [], |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 4);
    }
}
