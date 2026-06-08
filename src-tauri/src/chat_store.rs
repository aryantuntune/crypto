use crate::db::Db;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: i64,
    pub ts: i64,
    pub role: String,
    pub content: String,
    pub image_path: Option<String>,
    pub prediction_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMessage {
    pub role: String,
    pub content: String,
    pub image_path: Option<String>,
    pub prediction_id: Option<i64>,
}

pub fn insert(db: &Db, msg: NewMessage) -> Result<i64> {
    let now = chrono::Utc::now().timestamp();
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO messages (ts, role, content, image_path, prediction_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![now, msg.role, msg.content, msg.image_path, msg.prediction_id],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn recent(db: &Db, limit: i64) -> Result<Vec<Message>> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, ts, role, content, image_path, prediction_id FROM messages ORDER BY ts DESC LIMIT ?1"
    )?;
    let rows = stmt.query_map([limit], |r| Ok(Message {
        id: r.get(0)?, ts: r.get(1)?, role: r.get(2)?, content: r.get(3)?,
        image_path: r.get(4)?, prediction_id: r.get(5)?,
    }))?;
    let mut v: Vec<Message> = rows.collect::<std::result::Result<_,_>>()?;
    v.reverse();
    Ok(v)
}

pub fn prune_older_than_secs(db: &Db, max_age_secs: i64) -> Result<usize> {
    let cutoff = chrono::Utc::now().timestamp() - max_age_secs;
    let conn = db.lock().unwrap();
    let n = conn.execute("DELETE FROM messages WHERE ts < ?1", [cutoff])?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn fresh() -> Db {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("chat.db");
        // leak the tempdir intentionally for test lifetime
        Box::leak(Box::new(tmp));
        db::open(&path).unwrap()
    }

    #[test]
    fn insert_and_recent() {
        let db = fresh();
        insert(&db, NewMessage { role: "user".into(), content: "hi".into(), image_path: None, prediction_id: None }).unwrap();
        insert(&db, NewMessage { role: "assistant".into(), content: "hello".into(), image_path: None, prediction_id: None }).unwrap();
        let r = recent(&db, 10).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].role, "user");
        assert_eq!(r[1].role, "assistant");
    }

    #[test]
    fn prune_removes_old() {
        let db = fresh();
        let conn_owned = db.clone();
        let conn = conn_owned.lock().unwrap();
        let old_ts = chrono::Utc::now().timestamp() - 8 * 86400;
        conn.execute("INSERT INTO messages (ts, role, content) VALUES (?1, 'user', 'old')", [old_ts]).unwrap();
        conn.execute("INSERT INTO messages (ts, role, content) VALUES (?1, 'user', 'new')", [chrono::Utc::now().timestamp()]).unwrap();
        drop(conn);
        let n = prune_older_than_secs(&db, 7 * 86400).unwrap();
        assert_eq!(n, 1);
        let remaining = recent(&db, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].content, "new");
    }
}
