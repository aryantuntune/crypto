use crate::db::Db;
use crate::error::{AppError, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "cognitrade";
const KEYRING_USER: &str = "anthropic_api_key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub daily_cost_cap_usd: f64,
    pub model_main: String,           // "claude-sonnet-4-6" | "claude-opus-4-7"
    pub model_extract: String,        // "claude-haiku-4-5-20251001"
    pub library_path: Option<String>, // override; None = default ~/CogniTrade/library
    pub hotkey: String,               // "Ctrl+Shift+Space"
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            daily_cost_cap_usd: 2.0,
            model_main: "claude-sonnet-4-6".into(),
            model_extract: "claude-haiku-4-5-20251001".into(),
            library_path: None,
            hotkey: "Ctrl+Shift+Space".into(),
        }
    }
}

pub fn load(db: &Db) -> Result<Settings> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok::<(String, String), rusqlite::Error>((r.get(0)?, r.get(1)?)))?;
    let mut s = Settings::default();
    for row in rows {
        let (k, v) = row?;
        match k.as_str() {
            "daily_cost_cap_usd" => if let Ok(f) = v.parse() { s.daily_cost_cap_usd = f; },
            "model_main" => s.model_main = v,
            "model_extract" => s.model_extract = v,
            "library_path" => s.library_path = Some(v),
            "hotkey" => s.hotkey = v,
            _ => {}
        }
    }
    Ok(s)
}

pub fn save(db: &Db, s: &Settings) -> Result<()> {
    // The daily cost cap gates every paid API call (see cost::analyze_with_cap),
    // so reject anything that can't be compared meaningfully against accrued cost.
    if !s.daily_cost_cap_usd.is_finite() || s.daily_cost_cap_usd < 0.0 {
        return Err(AppError::Invalid(
            "Daily cost cap must be a number of 0 or greater".into(),
        ));
    }
    let conn = db.lock().unwrap();
    let pairs: [(&str, String); 5] = [
        ("daily_cost_cap_usd", s.daily_cost_cap_usd.to_string()),
        ("model_main", s.model_main.clone()),
        ("model_extract", s.model_extract.clone()),
        ("library_path", s.library_path.clone().unwrap_or_default()),
        ("hotkey", s.hotkey.clone()),
    ];
    for (k, v) in pairs {
        conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            rusqlite::params![k, v],
        )?;
    }
    Ok(())
}

pub fn get_api_key() -> Result<Option<String>> {
    let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e)),
    }
}

pub fn set_api_key(value: &str) -> Result<()> {
    let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    entry.set_password(value)?;
    Ok(())
}

pub fn clear_api_key() -> Result<()> {
    let entry = Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    match entry.delete_password() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(e)),
    }
}

pub fn require_api_key() -> Result<String> {
    get_api_key()?.ok_or(AppError::NoApiKey)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn defaults_then_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("chat.db")).unwrap();
        let s = load(&db).unwrap();
        assert_eq!(s.daily_cost_cap_usd, 2.0);
        let mut s2 = s.clone();
        s2.daily_cost_cap_usd = 5.0;
        s2.model_main = "claude-opus-4-7".into();
        save(&db, &s2).unwrap();
        let s3 = load(&db).unwrap();
        assert_eq!(s3.daily_cost_cap_usd, 5.0);
        assert_eq!(s3.model_main, "claude-opus-4-7");
    }

    #[test]
    fn save_rejects_invalid_cost_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("chat.db")).unwrap();
        let mut s = Settings::default();

        s.daily_cost_cap_usd = -1.0;
        assert!(matches!(save(&db, &s), Err(AppError::Invalid(_))));

        s.daily_cost_cap_usd = f64::NAN;
        assert!(matches!(save(&db, &s), Err(AppError::Invalid(_))));

        s.daily_cost_cap_usd = f64::INFINITY;
        assert!(matches!(save(&db, &s), Err(AppError::Invalid(_))));

        // Boundary: 0.0 is allowed (effectively disables paid calls).
        s.daily_cost_cap_usd = 0.0;
        assert!(save(&db, &s).is_ok());
    }
}
