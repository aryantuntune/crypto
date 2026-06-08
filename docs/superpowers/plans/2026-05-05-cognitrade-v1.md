# CogniTrade v1 Implementation Plan

> **Status (2026-05-07):** Implemented and packaged. All 31 tasks have outcomes. `cargo build` clean, 30/30 backend tests pass, frontend `npm run build` passes, **release MSI installer produced at `src-tauri/target/release/bundle/msi/cognitrade-app_0.1.0_x64_en-US.msi` (16.5 MB)**. One architectural deviation from the original plan: lancedb was replaced with a SQLite-backed brute-force vector store after lance v0.17 hit a rustc compile-depth limit and lance-encoding required protoc — the simpler store is also a better fit for the 8GB RAM budget and a personal doc library scale. See commit `a82e5c1`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Windows desktop tray app (Tauri + Svelte) that ingests trading PDFs and reference chart images, accepts pasted screenshots in a slide-out chat panel, and returns LLM-backed analysis with structured probability/action/citations using a hybrid local-RAG + Claude-API pipeline. Advisor only — never trades.

**Architecture:** Tauri 2.x Rust backend hosts: ingester → BGE-small ONNX embeddings via `fastembed` → LanceDB → retrieval → Anthropic Messages API (Sonnet 4.6 default) → JSON-tailed analysis. SQLite stores chat (7-day prune), predictions, daily cost. Svelte+Tailwind frontend lives in a tray-anchored panel pinned to the right edge of the primary monitor.

**Tech Stack:** Rust 1.78+, Tauri 2.x, Svelte 5, Vite, Tailwind, LanceDB, fastembed-rs, rusqlite, keyring, reqwest, tokio, pdf-extract, image, serde_json. Anthropic Messages API. CoinGecko free API.

---

## File Structure

### Created files

**Frontend (Svelte):**
- `src/main.ts` — Svelte entry
- `src/App.svelte` — root view router (chat / library / settings)
- `src/app.css` — Tailwind directives
- `src/lib/ipc.ts` — typed wrappers around `invoke()` calls
- `src/lib/stores/chat.ts` — chat messages store + streaming
- `src/lib/stores/settings.ts` — settings store
- `src/lib/stores/library.ts` — library state
- `src/lib/components/ChatPanel.svelte` — chat scroll + composer
- `src/lib/components/MessageBubble.svelte` — single message render
- `src/lib/components/PredictionCard.svelte` — structured analysis display
- `src/lib/components/LibraryView.svelte` — drag-drop + list
- `src/lib/components/SettingsView.svelte` — API key, cost cap, paths
- `src/lib/components/ScreenshotPaste.svelte` — paste handler
- `src/lib/types.ts` — shared TS types matching Rust IPC contracts

**Backend (Rust, in `src-tauri/src/`):**
- `lib.rs` — Tauri builder, plugins, IPC registration
- `main.rs` — generated entry
- `paths.rs` — resolves `%USERPROFILE%/CogniTrade/*` paths
- `error.rs` — `AppError` enum (single error type for IPC)
- `db.rs` — SQLite connection + migrations
- `chat_store.rs` — `messages` CRUD + 7-day prune
- `predictions.rs` — `predictions` CRUD + outcome resolver
- `cost.rs` — token counting, daily aggregation, cap check
- `settings.rs` — settings table CRUD + `keyring` for API key
- `coingecko.rs` — minimal price fetch client
- `llm/mod.rs` — module root
- `llm/types.rs` — `Action`, `AnalysisJson`, `Horizon`, etc.
- `llm/prompt.rs` — system prompt + message assembly
- `llm/parse.rs` — JSON-tail extractor
- `llm/streaming.rs` — SSE parser
- `llm/client.rs` — Anthropic HTTP client (uses cost tracker)
- `image_util.rs` — image resize (5MB / 1568px threshold)
- `ingest/mod.rs` — module root + public `ingest_file()`
- `ingest/chunker.rs` — text chunking
- `ingest/pdf.rs` — PDF text extraction
- `ingest/image.rs` — image-to-description path
- `ingest/embeddings.rs` — fastembed wrapper, lazy-loaded
- `ingest/watcher.rs` — filesystem watcher on library path
- `lance.rs` — LanceDB connection + schema + write
- `retrieval.rs` — top-k cosine search
- `prune.rs` — 7-day chat prune scheduled task
- `tray.rs` — tray icon, panel show/hide, hotkey
- `ipc.rs` — `#[tauri::command]` handlers, all IPC entry points

**Config:**
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src-tauri/build.rs` (generated)
- `src-tauri/icons/*` (generated)
- `package.json`, `vite.config.ts`, `svelte.config.js`, `tsconfig.json`, `tailwind.config.js`, `postcss.config.js`

### Test files
- `src-tauri/src/llm/parse.rs` — inline `#[cfg(test)] mod tests`
- `src-tauri/src/llm/prompt.rs` — inline tests
- `src-tauri/src/ingest/chunker.rs` — inline tests
- `src-tauri/src/cost.rs` — inline tests
- `src-tauri/src/chat_store.rs` — inline tests (uses tempdir)
- `src-tauri/src/image_util.rs` — inline tests
- `src-tauri/tests/llm_integration.rs` — wiremock-based end-to-end LLM client test
- `src-tauri/tests/ingest_integration.rs` — PDF + LanceDB end-to-end

---

## Phase 0 — Scaffolding

### Task 1: Bootstrap the Tauri 2 + Svelte project

**Files:**
- Create: project skeleton at repo root via scaffold tool

- [ ] **Step 1: Run the scaffold**

Run from the repo root:

```powershell
npm create tauri-app@latest -- --template svelte-ts --manager npm cognitrade-app
```

This creates a `cognitrade-app/` subdirectory. We want the contents at the repo root instead.

- [ ] **Step 2: Move scaffold contents to repo root**

```powershell
Move-Item cognitrade-app\* . -Force
Move-Item cognitrade-app\.* . -Force -ErrorAction SilentlyContinue
Remove-Item cognitrade-app -Recurse
```

- [ ] **Step 3: Install dependencies**

```powershell
npm install
```

- [ ] **Step 4: Smoke-test the dev build**

```powershell
npm run tauri dev
```

Expected: a window opens showing the default Svelte+Tauri demo. Close it.

- [ ] **Step 5: Commit**

```powershell
git add -A
git commit -m "chore: scaffold Tauri 2 + Svelte TS project"
```

---

### Task 2: Add Tailwind, project dependencies, and Tauri plugins

**Files:**
- Create: `tailwind.config.js`, `postcss.config.js`
- Modify: `src/app.css`, `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`

- [ ] **Step 1: Install frontend deps**

```powershell
npm install -D tailwindcss postcss autoprefixer
npm install @tauri-apps/plugin-fs @tauri-apps/plugin-dialog @tauri-apps/plugin-global-shortcut
npx tailwindcss init -p
```

- [ ] **Step 2: Configure Tailwind**

Replace `tailwind.config.js`:
```js
/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{svelte,ts}'],
  theme: { extend: {} },
  plugins: [],
};
```

Replace `src/app.css`:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;

html, body, #app { height: 100%; margin: 0; }
body { background: #0b0d10; color: #e6e8eb; font-family: ui-sans-serif, system-ui, sans-serif; }
```

Make sure `src/main.ts` imports it: add `import './app.css';` if not present.

- [ ] **Step 3: Add Rust deps to `src-tauri/Cargo.toml`**

Append to `[dependencies]`:
```toml
tauri-plugin-global-shortcut = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
tokio = { version = "1", features = ["full"] }
rusqlite = { version = "0.31", features = ["bundled"] }
keyring = "2"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4"] }
sha2 = "0.10"
hex = "0.4"
notify = "6"
pdf-extract = "0.7"
image = "0.25"
lancedb = "0.10"
arrow-array = "52"
arrow-schema = "52"
fastembed = "4"
futures = "0.3"
async-trait = "0.1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Append to `[dev-dependencies]`:
```toml
tempfile = "3"
wiremock = "0.6"
tokio = { version = "1", features = ["full", "test-util"] }
pretty_assertions = "1"
```

- [ ] **Step 4: Confirm cargo resolves**

```powershell
cd src-tauri
cargo check
cd ..
```

Expected: completes without errors. Some "unused dep" warnings are OK.

- [ ] **Step 5: Commit**

```powershell
git add -A
git commit -m "chore: add Tailwind, Rust deps, and Tauri plugins"
```

---

### Task 3: Path resolver and AppError type

**Files:**
- Create: `src-tauri/src/paths.rs`, `src-tauri/src/error.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write paths.rs with failing test**

`src-tauri/src/paths.rs`:
```rust
use std::path::PathBuf;

pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(home: PathBuf) -> Self {
        Self { root: home.join("CogniTrade") }
    }

    pub fn library(&self) -> PathBuf { self.root.join("library") }
    pub fn screenshots(&self) -> PathBuf { self.root.join("screenshots") }
    pub fn data(&self) -> PathBuf { self.root.join("data") }
    pub fn lancedb(&self) -> PathBuf { self.data().join("lancedb") }
    pub fn chat_db(&self) -> PathBuf { self.data().join("chat.db") }
    pub fn logs(&self) -> PathBuf { self.root.join("logs") }

    pub fn ensure_all(&self) -> std::io::Result<()> {
        for p in [&self.library(), &self.screenshots(), &self.data(), &self.lancedb(), &self.logs()] {
            std::fs::create_dir_all(p)?;
        }
        Ok(())
    }

    pub fn from_env() -> Self {
        let home = dirs::home_dir().expect("home dir resolvable");
        Self::new(home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensures_all_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::new(tmp.path().to_path_buf());
        p.ensure_all().unwrap();
        assert!(p.library().is_dir());
        assert!(p.screenshots().is_dir());
        assert!(p.lancedb().is_dir());
        assert!(p.logs().is_dir());
    }
}
```

Add `dirs = "5"` to `[dependencies]` in `Cargo.toml`.

- [ ] **Step 2: Write error.rs**

`src-tauri/src/error.rs`:
```rust
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("io error: {0}")] Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")] Sqlite(#[from] rusqlite::Error),
    #[error("http error: {0}")] Http(#[from] reqwest::Error),
    #[error("serde error: {0}")] Serde(#[from] serde_json::Error),
    #[error("keyring error: {0}")] Keyring(#[from] keyring::Error),
    #[error("anthropic api error: {0}")] Anthropic(String),
    #[error("daily cost cap reached")] CostCapReached,
    #[error("api key not set")] NoApiKey,
    #[error("not found: {0}")] NotFound(String),
    #[error("invalid input: {0}")] Invalid(String),
    #[error("internal: {0}")] Internal(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
```

- [ ] **Step 3: Wire modules into lib.rs**

In `src-tauri/src/lib.rs`, add at the top (above the existing `run()`):
```rust
pub mod error;
pub mod paths;
```

- [ ] **Step 4: Run the test**

```powershell
cd src-tauri
cargo test paths::tests
cd ..
```

Expected: `1 passed`.

- [ ] **Step 5: Commit**

```powershell
git add -A
git commit -m "feat: add Paths resolver and AppError type"
```

---

## Phase 1 — Foundations: storage, settings, cost tracker

### Task 4: SQLite migrations and shared connection

**Files:**
- Create: `src-tauri/src/db.rs`
- Create: `src-tauri/migrations/001_initial.sql`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write migrations/001_initial.sql**

`src-tauri/migrations/001_initial.sql`:
```sql
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('user','assistant','system')),
  content TEXT NOT NULL,
  image_path TEXT,
  prediction_id INTEGER REFERENCES predictions(id)
);
CREATE INDEX IF NOT EXISTS idx_messages_ts ON messages(ts);

CREATE TABLE IF NOT EXISTS predictions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  symbol TEXT NOT NULL,
  horizon_seconds INTEGER NOT NULL,
  predicted_prob_up REAL NOT NULL,
  recommended_action TEXT NOT NULL,
  stop_loss_pct REAL,
  take_profit_pct REAL,
  citations_json TEXT,
  outcome_status TEXT NOT NULL DEFAULT 'pending',
  outcome_price_at_ts REAL,
  outcome_price_at_horizon REAL,
  outcome_resolved_at INTEGER
);

CREATE TABLE IF NOT EXISTS cost_daily (
  date TEXT PRIMARY KEY,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  cached_input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

- [ ] **Step 2: Write db.rs**

`src-tauri/src/db.rs`:
```rust
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
```

- [ ] **Step 3: Wire into lib.rs**

Add `pub mod db;` near the other `pub mod` lines.

- [ ] **Step 4: Run test**

```powershell
cd src-tauri
cargo test db::tests
cd ..
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add -A
git commit -m "feat: SQLite connection + initial migration"
```

---

### Task 5: Chat store with insert, list, prune

**Files:**
- Create: `src-tauri/src/chat_store.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write chat_store.rs with failing tests**

`src-tauri/src/chat_store.rs`:
```rust
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
```

- [ ] **Step 2: Wire and run**

Add `pub mod chat_store;` to `lib.rs`.

```powershell
cd src-tauri
cargo test chat_store::tests
cd ..
```

Expected: 2 passed.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: chat_store with insert/recent/prune"
```

---

### Task 6: Settings table CRUD + keyring API key

**Files:**
- Create: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write settings.rs**

`src-tauri/src/settings.rs`:
```rust
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
}
```

- [ ] **Step 2: Wire and run**

Add `pub mod settings;` to `lib.rs`.

```powershell
cd src-tauri
cargo test settings::tests
cd ..
```

Expected: PASS. (Keyring tests are skipped — they require an interactive session.)

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: settings CRUD + keyring api key helpers"
```

---

### Task 7: Cost tracker

**Files:**
- Create: `src-tauri/src/cost.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write cost.rs**

Token rates per the published 2026 Anthropic pricing (USD per million tokens). These values are used as constants — update them as pricing changes.

`src-tauri/src/cost.rs`:
```rust
use crate::db::Db;
use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct ModelRates {
    pub input_per_mtok: f64,
    pub cached_per_mtok: f64,
    pub output_per_mtok: f64,
}

pub fn rates_for(model: &str) -> ModelRates {
    // Sonnet 4.6: $3/$0.30/$15. Opus 4.7: $15/$1.50/$75. Haiku 4.5: $1/$0.10/$5.
    if model.contains("opus") {
        ModelRates { input_per_mtok: 15.0, cached_per_mtok: 1.5, output_per_mtok: 75.0 }
    } else if model.contains("haiku") {
        ModelRates { input_per_mtok: 1.0, cached_per_mtok: 0.10, output_per_mtok: 5.0 }
    } else {
        ModelRates { input_per_mtok: 3.0, cached_per_mtok: 0.30, output_per_mtok: 15.0 }
    }
}

pub fn cost_usd(rates: ModelRates, input: u64, cached: u64, output: u64) -> f64 {
    let m = 1_000_000.0;
    (input as f64 * rates.input_per_mtok
        + cached as f64 * rates.cached_per_mtok
        + output as f64 * rates.output_per_mtok) / m
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyCost {
    pub date: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

pub fn add_usage(db: &Db, model: &str, input: u64, cached: u64, output: u64) -> Result<DailyCost> {
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let added = cost_usd(rates_for(model), input, cached, output);
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO cost_daily(date, input_tokens, cached_input_tokens, output_tokens, cost_usd)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date) DO UPDATE SET
            input_tokens = input_tokens + excluded.input_tokens,
            cached_input_tokens = cached_input_tokens + excluded.cached_input_tokens,
            output_tokens = output_tokens + excluded.output_tokens,
            cost_usd = cost_usd + excluded.cost_usd",
        rusqlite::params![date, input as i64, cached as i64, output as i64, added],
    )?;
    let row = conn.query_row(
        "SELECT date, input_tokens, cached_input_tokens, output_tokens, cost_usd FROM cost_daily WHERE date=?1",
        [&date],
        |r| Ok(DailyCost {
            date: r.get(0)?,
            input_tokens: r.get::<_, i64>(1)? as u64,
            cached_input_tokens: r.get::<_, i64>(2)? as u64,
            output_tokens: r.get::<_, i64>(3)? as u64,
            cost_usd: r.get(4)?,
        })
    )?;
    Ok(row)
}

pub fn today(db: &Db) -> Result<DailyCost> {
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let conn = db.lock().unwrap();
    let row = conn.query_row(
        "SELECT date, input_tokens, cached_input_tokens, output_tokens, cost_usd FROM cost_daily WHERE date=?1",
        [&date],
        |r| Ok(DailyCost {
            date: r.get(0)?,
            input_tokens: r.get::<_, i64>(1)? as u64,
            cached_input_tokens: r.get::<_, i64>(2)? as u64,
            output_tokens: r.get::<_, i64>(3)? as u64,
            cost_usd: r.get(4)?,
        })
    ).unwrap_or(DailyCost { date, ..Default::default() });
    Ok(row)
}

pub fn is_over_cap(today_cost: f64, cap: f64) -> bool { today_cost >= cap }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn cost_arithmetic_sonnet() {
        let r = rates_for("claude-sonnet-4-6");
        // 1M input + 0 cached + 0 output = $3
        assert!((cost_usd(r, 1_000_000, 0, 0) - 3.0).abs() < 1e-9);
        // 0 input + 0 cached + 1M output = $15
        assert!((cost_usd(r, 0, 0, 1_000_000) - 15.0).abs() < 1e-9);
        // 1M cached = $0.30
        assert!((cost_usd(r, 0, 1_000_000, 0) - 0.30).abs() < 1e-9);
    }

    #[test]
    fn rates_pick_correct_model() {
        assert_eq!(rates_for("claude-haiku-4-5-20251001").input_per_mtok, 1.0);
        assert_eq!(rates_for("claude-opus-4-7").input_per_mtok, 15.0);
        assert_eq!(rates_for("claude-sonnet-4-6").input_per_mtok, 3.0);
    }

    #[test]
    fn add_usage_accumulates_same_day() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("chat.db")).unwrap();
        add_usage(&db, "claude-sonnet-4-6", 1000, 0, 200).unwrap();
        let r = add_usage(&db, "claude-sonnet-4-6", 500, 100, 50).unwrap();
        assert_eq!(r.input_tokens, 1500);
        assert_eq!(r.cached_input_tokens, 100);
        assert_eq!(r.output_tokens, 250);
        assert!(r.cost_usd > 0.0);
    }

    #[test]
    fn cap_check() {
        assert!(!is_over_cap(1.0, 2.0));
        assert!(is_over_cap(2.0, 2.0));
        assert!(is_over_cap(2.5, 2.0));
    }
}
```

- [ ] **Step 2: Wire and run**

Add `pub mod cost;` to `lib.rs`.

```powershell
cd src-tauri
cargo test cost::tests
cd ..
```

Expected: 4 passed.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: cost tracker with per-model rates and daily aggregation"
```

---

## Phase 2 — LLM client

### Task 8: LLM types and JSON-tail parser

**Files:**
- Create: `src-tauri/src/llm/mod.rs`, `src-tauri/src/llm/types.rs`, `src-tauri/src/llm/parse.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write llm/types.rs**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action { Buy, Sell, Hold }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Horizon {
    #[serde(rename = "1h")] H1,
    #[serde(rename = "4h")] H4,
    #[serde(rename = "1d")] D1,
    #[serde(rename = "3d")] D3,
    #[serde(rename = "1w")] W1,
}

impl Horizon {
    pub fn seconds(&self) -> i64 {
        match self {
            Horizon::H1 => 3600,
            Horizon::H4 => 4 * 3600,
            Horizon::D1 => 86_400,
            Horizon::D3 => 3 * 86_400,
            Horizon::W1 => 7 * 86_400,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Citation {
    pub doc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisJson {
    pub action: Action,
    pub probability_up: f32,
    pub horizon: Horizon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_loss_pct: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take_profit_pct: Option<f32>,
    #[serde(default)]
    pub key_signals: Vec<String>,
    #[serde(default)]
    pub citations: Vec<Citation>,
}

impl AnalysisJson {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.probability_up) {
            return Err(format!("probability_up out of range: {}", self.probability_up));
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Write llm/parse.rs**

```rust
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
```

- [ ] **Step 3: Write llm/mod.rs**

```rust
pub mod types;
pub mod parse;
pub mod prompt;
pub mod streaming;
pub mod client;
```

(`prompt`, `streaming`, `client` will be added in Tasks 9–11; for now declare the modules and create empty placeholder files in Step 4 so the crate compiles.)

- [ ] **Step 4: Create placeholders**

Create empty files:
- `src-tauri/src/llm/prompt.rs` containing only `// filled in Task 9`
- `src-tauri/src/llm/streaming.rs` containing only `// filled in Task 10`
- `src-tauri/src/llm/client.rs` containing only `// filled in Task 11`

- [ ] **Step 5: Wire and test**

Add `pub mod llm;` to `lib.rs`.

```powershell
cd src-tauri
cargo test llm::parse::tests
cd ..
```

Expected: 4 passed.

- [ ] **Step 6: Commit**

```powershell
git add -A
git commit -m "feat: llm types and JSON-tail parser"
```

---

### Task 9: Prompt builder

**Files:**
- Modify: `src-tauri/src/llm/prompt.rs`

- [ ] **Step 1: Replace prompt.rs**

```rust
use serde::Serialize;

pub const SYSTEM_PROMPT: &str = r#"You are CogniTrade, a personal crypto trading analyst. The user will share a chart screenshot. Your job:

1. Read the chart: identify symbol, timeframe, current price, key indicators, recent pattern.
2. Use the retrieved knowledge passages below as supporting context. Cite by document name (and page if PDF).
3. Output a brief plain-English explanation, then end your response with a fenced ```json block matching this schema:

{
  "action": "buy" | "sell" | "hold",
  "probability_up": <float 0..1>,
  "horizon": "1h" | "4h" | "1d" | "3d" | "1w",
  "stop_loss_pct": <float, e.g. 2.5>,
  "take_profit_pct": <float, e.g. 5.0>,
  "key_signals": [<short strings>],
  "citations": [{"doc": "<filename>", "page": <int or omit>}]
}

Rules:
- If the chart is unreadable or ambiguous, return action: "hold" and ask the user for a clearer image.
- probability_up MUST be in [0, 1].
- Be calibrated: probabilities should reflect realistic uncertainty, not advocacy.
- Cite at least one retrieved passage when you used it; if no retrieval was relevant, say so.
- Never recommend trades you can't justify with what's on the chart and the citations.
- You are an advisor. The user places all trades themselves.
"#;

#[derive(Debug, Clone, Serialize)]
pub struct ContentBlock<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSource<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageSource<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str, // "base64"
    pub media_type: &'a str, // "image/png"
    pub data: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheControl { #[serde(rename = "type")] pub kind: String }
impl CacheControl { pub fn ephemeral() -> Self { Self { kind: "ephemeral".into() } } }

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
}

pub struct PromptInputs<'a> {
    pub retrieved_chunks: &'a [String],
    pub history: &'a [ChatMessage],
    pub user_text: &'a str,
    pub image_b64: Option<&'a str>, // base64 PNG
}

pub fn build_messages(inputs: &PromptInputs) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    out.extend(inputs.history.iter().cloned());

    // Build the new user turn: retrieved context (cached) + image + text
    let mut blocks: Vec<serde_json::Value> = Vec::new();
    if !inputs.retrieved_chunks.is_empty() {
        let joined = format!("Retrieved knowledge:\n\n{}", inputs.retrieved_chunks.join("\n\n---\n\n"));
        blocks.push(serde_json::json!({
            "type": "text",
            "text": joined,
            "cache_control": {"type": "ephemeral"}
        }));
    }
    if let Some(b64) = inputs.image_b64 {
        blocks.push(serde_json::json!({
            "type": "image",
            "source": {"type": "base64", "media_type": "image/png", "data": b64}
        }));
    }
    blocks.push(serde_json::json!({"type": "text", "text": inputs.user_text}));

    out.push(ChatMessage {
        role: "user".into(),
        content: serde_json::Value::Array(blocks),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_user_turn_with_image_and_chunks() {
        let chunks = vec!["pattern A says X".to_string(), "pattern B says Y".to_string()];
        let history = vec![];
        let inputs = PromptInputs {
            retrieved_chunks: &chunks,
            history: &history,
            user_text: "what should I do?",
            image_b64: Some("ZmFrZQ=="),
        };
        let msgs = build_messages(&inputs);
        assert_eq!(msgs.len(), 1);
        let last = &msgs[0];
        assert_eq!(last.role, "user");
        let arr = last.content.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(arr[1]["type"], "image");
        assert_eq!(arr[2]["type"], "text");
        assert_eq!(arr[2]["text"], "what should I do?");
    }

    #[test]
    fn no_chunks_means_no_cached_block() {
        let inputs = PromptInputs {
            retrieved_chunks: &[],
            history: &[],
            user_text: "hi",
            image_b64: None,
        };
        let msgs = build_messages(&inputs);
        let arr = msgs[0].content.as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn includes_history_first() {
        let history = vec![
            ChatMessage { role: "user".into(), content: serde_json::json!("earlier q") },
            ChatMessage { role: "assistant".into(), content: serde_json::json!("earlier a") },
        ];
        let inputs = PromptInputs { retrieved_chunks: &[], history: &history, user_text: "now", image_b64: None };
        let msgs = build_messages(&inputs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "user");
    }
}
```

- [ ] **Step 2: Run tests**

```powershell
cd src-tauri
cargo test llm::prompt::tests
cd ..
```

Expected: 3 passed.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: prompt builder with retrieval caching"
```

---

### Task 10: SSE streaming parser

**Files:**
- Modify: `src-tauri/src/llm/streaming.rs`

- [ ] **Step 1: Replace streaming.rs**

```rust
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Text(String),
    Usage { input_tokens: u64, cached_input_tokens: u64, output_tokens: u64 },
    Done,
}

/// Anthropic SSE message_start.message.usage
#[derive(Debug, Deserialize)]
struct MessageStart {
    message: MessageStartInner,
}
#[derive(Debug, Deserialize)]
struct MessageStartInner {
    usage: Option<UsageRaw>,
}

#[derive(Debug, Deserialize, Default)]
struct UsageRaw {
    #[serde(default)] input_tokens: u64,
    #[serde(default)] cache_creation_input_tokens: u64,
    #[serde(default)] cache_read_input_tokens: u64,
    #[serde(default)] output_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct MessageDelta {
    usage: UsageRaw,
}

#[derive(Debug, Deserialize)]
struct ContentBlockDelta { delta: BlockDelta }

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum BlockDelta {
    #[serde(rename = "text_delta")] TextDelta { text: String },
    #[serde(other)] Other,
}

/// Parse a single Anthropic SSE event (the JSON object after `data: `).
/// Returns Some(event) when the line yields a recognized event, None otherwise.
pub fn parse_event(event_name: &str, data_json: &str) -> Option<StreamEvent> {
    match event_name {
        "content_block_delta" => {
            let cbd: ContentBlockDelta = serde_json::from_str(data_json).ok()?;
            match cbd.delta {
                BlockDelta::TextDelta { text } => Some(StreamEvent::Text(text)),
                _ => None,
            }
        }
        "message_start" => {
            let m: MessageStart = serde_json::from_str(data_json).ok()?;
            let u = m.message.usage?;
            // Treat cache_read as cached input; cache_creation also counts as input but is billed separately;
            // we sum cache_creation into input_tokens for a conservative cost estimate.
            Some(StreamEvent::Usage {
                input_tokens: u.input_tokens + u.cache_creation_input_tokens,
                cached_input_tokens: u.cache_read_input_tokens,
                output_tokens: u.output_tokens,
            })
        }
        "message_delta" => {
            let m: MessageDelta = serde_json::from_str(data_json).ok()?;
            Some(StreamEvent::Usage {
                input_tokens: m.usage.input_tokens + m.usage.cache_creation_input_tokens,
                cached_input_tokens: m.usage.cache_read_input_tokens,
                output_tokens: m.usage.output_tokens,
            })
        }
        "message_stop" => Some(StreamEvent::Done),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta() {
        let e = parse_event("content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#).unwrap();
        assert_eq!(e, StreamEvent::Text("hello".into()));
    }

    #[test]
    fn parses_message_start_usage() {
        let e = parse_event("message_start",
            r#"{"type":"message_start","message":{"id":"x","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-6","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":120,"cache_creation_input_tokens":10,"cache_read_input_tokens":50,"output_tokens":0}}}"#).unwrap();
        assert!(matches!(e, StreamEvent::Usage { input_tokens: 130, cached_input_tokens: 50, output_tokens: 0 }));
    }

    #[test]
    fn parses_message_stop() {
        assert_eq!(parse_event("message_stop", r#"{"type":"message_stop"}"#), Some(StreamEvent::Done));
    }

    #[test]
    fn ignores_unknown_event() {
        assert!(parse_event("ping", "{}").is_none());
    }
}
```

- [ ] **Step 2: Run tests**

```powershell
cd src-tauri
cargo test llm::streaming::tests
cd ..
```

Expected: 4 passed.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: SSE event parser for Anthropic streaming"
```

---

### Task 11: Anthropic HTTP client with mocked end-to-end test

**Files:**
- Modify: `src-tauri/src/llm/client.rs`
- Create: `src-tauri/tests/llm_integration.rs`

- [ ] **Step 1: Write client.rs**

```rust
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
```

- [ ] **Step 2: Write integration test with wiremock**

`src-tauri/tests/llm_integration.rs`:
```rust
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
```

Note: the integration test references the crate as `cognitrade_lib`. To make this work, the crate's library name must be `cognitrade_lib`. Set this in `src-tauri/Cargo.toml`:

```toml
[lib]
name = "cognitrade_lib"
crate-type = ["staticlib", "cdylib", "rlib"]
```

(Tauri's default scaffold typically generates `<package>_lib`. Adjust to match.) Also ensure `lib.rs` has `pub mod llm;` already (Task 8).

- [ ] **Step 3: Run tests**

```powershell
cd src-tauri
cargo test --test llm_integration
cd ..
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add -A
git commit -m "feat: Anthropic streaming client with cost-cap wrapper"
```

---

## Phase 3 — RAG pipeline

### Task 12: Text chunker

**Files:**
- Create: `src-tauri/src/ingest/mod.rs`, `src-tauri/src/ingest/chunker.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write chunker.rs**

```rust
/// Split text into ~`target_tokens` chunks with `overlap_tokens` overlap.
/// Token approximation: 1 token ≈ 4 chars (English heuristic). We chunk on
/// paragraph/sentence boundaries when possible.
pub fn chunk_text(text: &str, target_tokens: usize, overlap_tokens: usize) -> Vec<String> {
    let target_chars = target_tokens * 4;
    let overlap_chars = overlap_tokens * 4;
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() { return vec![]; }

    let bytes: Vec<char> = normalized.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let end = (start + target_chars).min(bytes.len());
        // Try to back up to a sentence boundary if not at the end of the text
        let mut split = end;
        if end < bytes.len() {
            let window_start = start + (target_chars * 2 / 3);
            for i in (window_start..end).rev() {
                if matches!(bytes[i], '.' | '!' | '?' | '\n') {
                    split = i + 1;
                    break;
                }
            }
        }
        let chunk: String = bytes[start..split].iter().collect();
        chunks.push(chunk.trim().to_string());
        if split >= bytes.len() { break; }
        start = split.saturating_sub(overlap_chars);
    }
    chunks.into_iter().filter(|c| !c.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_returns_empty() {
        assert!(chunk_text("", 100, 10).is_empty());
        assert!(chunk_text("   \n\n  ", 100, 10).is_empty());
    }

    #[test]
    fn short_text_is_one_chunk() {
        let c = chunk_text("Hello world. This is short.", 500, 50);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn long_text_splits_with_overlap() {
        let para = "Lorem ipsum dolor sit amet. ".repeat(200); // ~5400 chars
        let c = chunk_text(&para, 500, 50); // target ~2000 chars per chunk
        assert!(c.len() >= 2);
        // Ensure each chunk is below a generous bound
        for chunk in &c {
            assert!(chunk.len() <= 500 * 4 + 100, "chunk too big: {}", chunk.len());
        }
    }

    #[test]
    fn prefers_sentence_boundary() {
        let s = "First sentence here. Second one is here. Third lands here.";
        let c = chunk_text(s, 5, 0); // tiny target forces splits
        // Each chunk should not start mid-sentence ideally; just check we split
        assert!(c.len() >= 2);
    }
}
```

- [ ] **Step 2: Write ingest/mod.rs**

```rust
pub mod chunker;
pub mod pdf;
pub mod image;
pub mod embeddings;
pub mod watcher;
```

Add `pub mod ingest;` to `lib.rs`.

Create placeholder files:
- `pdf.rs`: `// filled in Task 13`
- `image.rs`: `// filled in Task 17`
- `embeddings.rs`: `// filled in Task 14`
- `watcher.rs`: `// filled in Task 16`

- [ ] **Step 3: Test**

```powershell
cd src-tauri
cargo test ingest::chunker::tests
cd ..
```

Expected: 4 passed.

- [ ] **Step 4: Commit**

```powershell
git add -A
git commit -m "feat: text chunker with sentence-aware splits"
```

---

### Task 13: PDF text extraction

**Files:**
- Modify: `src-tauri/src/ingest/pdf.rs`

- [ ] **Step 1: Write pdf.rs**

```rust
use crate::error::{AppError, Result};
use std::path::Path;

pub struct PdfText {
    pub full_text: String,
}

pub fn extract(path: &Path) -> Result<PdfText> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| AppError::Internal(format!("pdf extract: {}", e)))?;
    Ok(PdfText { full_text: text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_missing_file_errors() {
        let r = extract(Path::new("definitely-not-here.pdf"));
        assert!(r.is_err());
    }
}
```

(Real PDF parsing is exercised in Task 16's integration test against a fixture PDF added there.)

- [ ] **Step 2: Test**

```powershell
cd src-tauri
cargo test ingest::pdf::tests
cd ..
```

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: PDF text extraction"
```

---

### Task 14: Embeddings via fastembed (lazy-loaded)

**Files:**
- Modify: `src-tauri/src/ingest/embeddings.rs`

- [ ] **Step 1: Write embeddings.rs**

```rust
use crate::error::{AppError, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::{Mutex, OnceLock};

pub const EMBED_DIM: usize = 384; // BGE-small dim

static MODEL: OnceLock<Mutex<TextEmbedding>> = OnceLock::new();

fn get_or_init() -> Result<&'static Mutex<TextEmbedding>> {
    if MODEL.get().is_none() {
        let model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::BGESmallENV15))
            .map_err(|e| AppError::Internal(format!("fastembed init: {}", e)))?;
        let _ = MODEL.set(Mutex::new(model));
    }
    Ok(MODEL.get().expect("set above"))
}

pub fn embed(texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() { return Ok(vec![]); }
    let m = get_or_init()?;
    let mut guard = m.lock().unwrap();
    let v = guard.embed(texts, None)
        .map_err(|e| AppError::Internal(format!("fastembed embed: {}", e)))?;
    Ok(v)
}
```

(No unit test — fastembed downloads model weights on first run, which is too heavy/network-y for unit tests. Exercised via manual ingestion smoke test in Phase 8.)

- [ ] **Step 2: Verify build**

```powershell
cd src-tauri
cargo check
cd ..
```

Expected: compiles.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: BGE-small embeddings via fastembed (lazy)"
```

---

### Task 15: LanceDB schema and write/search

**Files:**
- Create: `src-tauri/src/lance.rs`, `src-tauri/src/retrieval.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write lance.rs**

```rust
use crate::error::{AppError, Result};
use crate::ingest::embeddings::EMBED_DIM;
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int32Array, RecordBatch, RecordBatchIterator,
    StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use lancedb::query::ExecutableQuery;
use lancedb::Connection;
use std::path::Path;
use std::sync::Arc;

const TABLE: &str = "documents";

pub struct LanceStore {
    pub conn: Connection,
}

pub async fn open(path: &Path) -> Result<LanceStore> {
    std::fs::create_dir_all(path)?;
    let conn = lancedb::connect(path.to_str().unwrap()).execute().await
        .map_err(|e| AppError::Internal(format!("lancedb connect: {}", e)))?;
    Ok(LanceStore { conn })
}

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("doc_path", DataType::Utf8, false),
        Field::new("doc_type", DataType::Utf8, false),
        Field::new("chunk_index", DataType::Int32, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("page", DataType::Int32, true),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                EMBED_DIM as i32,
            ),
            false,
        ),
    ]))
}

#[derive(Debug, Clone)]
pub struct DocRow {
    pub id: String,
    pub doc_path: String,
    pub doc_type: String, // "pdf" | "image"
    pub chunk_index: i32,
    pub text: String,
    pub page: Option<i32>,
    pub embedding: Vec<f32>,
}

impl LanceStore {
    pub async fn ensure_table(&self) -> Result<()> {
        let names = self.conn.table_names().execute().await
            .map_err(|e| AppError::Internal(format!("lance table_names: {}", e)))?;
        if names.contains(&TABLE.to_string()) { return Ok(()); }
        let s = schema();
        let empty_iter = RecordBatchIterator::new(vec![], s.clone());
        self.conn.create_table(TABLE, Box::new(empty_iter)).execute().await
            .map_err(|e| AppError::Internal(format!("lance create_table: {}", e)))?;
        Ok(())
    }

    pub async fn insert(&self, rows: Vec<DocRow>) -> Result<()> {
        if rows.is_empty() { return Ok(()); }
        self.ensure_table().await?;
        let s = schema();
        let ids = StringArray::from_iter_values(rows.iter().map(|r| r.id.as_str()));
        let paths = StringArray::from_iter_values(rows.iter().map(|r| r.doc_path.as_str()));
        let kinds = StringArray::from_iter_values(rows.iter().map(|r| r.doc_type.as_str()));
        let chunks = Int32Array::from_iter_values(rows.iter().map(|r| r.chunk_index));
        let texts = StringArray::from_iter_values(rows.iter().map(|r| r.text.as_str()));
        let pages = Int32Array::from(rows.iter().map(|r| r.page).collect::<Vec<_>>());

        // Build FixedSizeList<Float32, EMBED_DIM>
        let flat: Vec<Option<f32>> = rows.iter().flat_map(|r| r.embedding.iter().map(|f| Some(*f))).collect();
        let values = Float32Array::from(flat);
        let field = Arc::new(Field::new("item", DataType::Float32, true));
        let emb = FixedSizeListArray::new(field, EMBED_DIM as i32, Arc::new(values), None);

        let batch = RecordBatch::try_new(s.clone(), vec![
            Arc::new(ids), Arc::new(paths), Arc::new(kinds), Arc::new(chunks),
            Arc::new(texts), Arc::new(pages), Arc::new(emb),
        ]).map_err(|e| AppError::Internal(format!("arrow batch: {}", e)))?;
        let iter = RecordBatchIterator::new(vec![Ok(batch)], s);
        let table = self.conn.open_table(TABLE).execute().await
            .map_err(|e| AppError::Internal(format!("lance open_table: {}", e)))?;
        table.add(Box::new(iter)).execute().await
            .map_err(|e| AppError::Internal(format!("lance add: {}", e)))?;
        Ok(())
    }

    pub async fn search(&self, query_emb: Vec<f32>, k: usize) -> Result<Vec<DocRow>> {
        let names = self.conn.table_names().execute().await
            .map_err(|e| AppError::Internal(format!("lance table_names: {}", e)))?;
        if !names.contains(&TABLE.to_string()) { return Ok(vec![]); }
        let table = self.conn.open_table(TABLE).execute().await
            .map_err(|e| AppError::Internal(format!("lance open_table: {}", e)))?;
        let stream = table.query()
            .nearest_to(query_emb)
            .map_err(|e| AppError::Internal(format!("lance nearest_to: {}", e)))?
            .limit(k)
            .execute().await
            .map_err(|e| AppError::Internal(format!("lance query exec: {}", e)))?;
        use futures::TryStreamExt;
        let batches: Vec<RecordBatch> = stream.try_collect().await
            .map_err(|e| AppError::Internal(format!("lance collect: {}", e)))?;

        let mut out = Vec::new();
        for batch in batches {
            let n = batch.num_rows();
            let ids = batch.column_by_name("id").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
            let paths = batch.column_by_name("doc_path").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
            let kinds = batch.column_by_name("doc_type").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
            let chunks = batch.column_by_name("chunk_index").unwrap().as_any().downcast_ref::<Int32Array>().unwrap();
            let texts = batch.column_by_name("text").unwrap().as_any().downcast_ref::<StringArray>().unwrap();
            let pages = batch.column_by_name("page").unwrap().as_any().downcast_ref::<Int32Array>().unwrap();
            for i in 0..n {
                out.push(DocRow {
                    id: ids.value(i).to_string(),
                    doc_path: paths.value(i).to_string(),
                    doc_type: kinds.value(i).to_string(),
                    chunk_index: chunks.value(i),
                    text: texts.value(i).to_string(),
                    page: if pages.is_null(i) { None } else { Some(pages.value(i)) },
                    embedding: vec![],
                });
            }
        }
        Ok(out)
    }

    pub async fn delete_by_doc_path(&self, doc_path: &str) -> Result<()> {
        let names = self.conn.table_names().execute().await
            .map_err(|e| AppError::Internal(format!("lance table_names: {}", e)))?;
        if !names.contains(&TABLE.to_string()) { return Ok(()); }
        let table = self.conn.open_table(TABLE).execute().await
            .map_err(|e| AppError::Internal(format!("lance open_table: {}", e)))?;
        table.delete(&format!("doc_path = '{}'", doc_path.replace('\'', "''"))).await
            .map_err(|e| AppError::Internal(format!("lance delete: {}", e)))?;
        Ok(())
    }
}
```

- [ ] **Step 2: Write retrieval.rs**

```rust
use crate::error::Result;
use crate::ingest::embeddings;
use crate::lance::{DocRow, LanceStore};

pub async fn search(store: &LanceStore, query: &str, k: usize) -> Result<Vec<DocRow>> {
    let emb = embeddings::embed(vec![query.to_string()])?;
    let q = emb.into_iter().next().unwrap_or_default();
    if q.is_empty() { return Ok(vec![]); }
    store.search(q, k).await
}
```

- [ ] **Step 3: Wire and build**

Add `pub mod lance;` and `pub mod retrieval;` to `lib.rs`.

```powershell
cd src-tauri
cargo check
cd ..
```

Expected: compiles. (No unit tests yet — exercised in Task 16.)

- [ ] **Step 4: Commit**

```powershell
git add -A
git commit -m "feat: LanceDB schema and retrieval"
```

---

### Task 16: Document ingester (PDF path) + integration test

**Files:**
- Create: `src-tauri/src/ingest/watcher.rs` (replace placeholder)
- Modify: `src-tauri/src/ingest/mod.rs` to add `ingest_file()`
- Create: `src-tauri/tests/ingest_integration.rs`
- Create: `src-tauri/tests/fixtures/sample.pdf` (any small text PDF; commit it)

- [ ] **Step 1: Add `ingest_file` in ingest/mod.rs**

Replace `src-tauri/src/ingest/mod.rs`:
```rust
pub mod chunker;
pub mod pdf;
pub mod image;
pub mod embeddings;
pub mod watcher;

use crate::error::{AppError, Result};
use crate::lance::{DocRow, LanceStore};
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

pub fn file_hash(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

#[derive(Debug)]
pub struct IngestReport {
    pub doc_path: String,
    pub chunks: usize,
}

pub async fn ingest_pdf(store: &LanceStore, path: &Path) -> Result<IngestReport> {
    let extracted = pdf::extract(path)?;
    let chunks = chunker::chunk_text(&extracted.full_text, 500, 50);
    if chunks.is_empty() {
        return Err(AppError::Invalid(format!("no text in {}", path.display())));
    }
    let embs = embeddings::embed(chunks.clone())?;
    let path_str = path.to_string_lossy().to_string();
    let rows: Vec<DocRow> = chunks.iter().enumerate().zip(embs).map(|((i, t), e)| DocRow {
        id: Uuid::new_v4().to_string(),
        doc_path: path_str.clone(),
        doc_type: "pdf".into(),
        chunk_index: i as i32,
        text: t.clone(),
        page: None,
        embedding: e,
    }).collect();
    store.delete_by_doc_path(&path_str).await?;
    let n = rows.len();
    store.insert(rows).await?;
    Ok(IngestReport { doc_path: path_str, chunks: n })
}
```

- [ ] **Step 2: Write watcher.rs (debounced)**

```rust
use crate::error::Result;
use crate::lance::LanceStore;
use crate::ingest::ingest_pdf;
use notify::{EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
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
            }
            // image branch added in Task 17
        }
    }
}
```

- [ ] **Step 3: Add fixture PDF**

Create a tiny text PDF. From PowerShell using a Python one-liner if available, or include any small PDF file. The test only requires that `pdf-extract` returns *some* text. If you don't have one handy, generate from a markdown file:

```powershell
# If pandoc is available; otherwise place any small PDF at the path manually.
'Hello cognitrade fixture document.' | Out-File -Encoding ascii src-tauri\tests\fixtures\sample.txt
# Use any conversion tool. Example with `wkhtmltopdf` or just commit a known PDF.
```

If toolchain not available, manually drop any small PDF named `sample.pdf` into `src-tauri/tests/fixtures/`.

- [ ] **Step 4: Integration test**

`src-tauri/tests/ingest_integration.rs`:
```rust
use cognitrade_lib::lance;
use cognitrade_lib::ingest::ingest_pdf;
use std::path::Path;

#[tokio::test]
#[ignore] // network-touching: downloads BGE-small on first run; manual smoke test
async fn ingest_pdf_into_lancedb() {
    let tmp = tempfile::tempdir().unwrap();
    let store = lance::open(tmp.path()).await.unwrap();
    let fixture = Path::new("tests/fixtures/sample.pdf");
    let report = ingest_pdf(&store, fixture).await.unwrap();
    assert!(report.chunks > 0);
    let hits = cognitrade_lib::retrieval::search(&store, "hello", 3).await.unwrap();
    assert!(!hits.is_empty());
}
```

Marked `#[ignore]` because fastembed downloads ~100MB on first run.

- [ ] **Step 5: Manual smoke test**

```powershell
cd src-tauri
cargo test --test ingest_integration -- --ignored --nocapture
cd ..
```

Expected: chunks > 0 and at least one search hit.

- [ ] **Step 6: Commit**

```powershell
git add -A
git commit -m "feat: PDF ingester + filesystem watcher with end-to-end test"
```

---

### Task 17: Image ingestion path (Claude vision → description → embed)

**Files:**
- Modify: `src-tauri/src/ingest/image.rs`
- Modify: `src-tauri/src/ingest/mod.rs` (add `ingest_image`)
- Modify: `src-tauri/src/ingest/watcher.rs` (call `ingest_image` for png/jpg)

- [ ] **Step 1: Write image.rs**

```rust
use crate::error::{AppError, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

const DESCRIBE_PROMPT: &str = "You are indexing this chart image for later retrieval. Output a single dense paragraph (no bullets, no JSON) describing: symbol/timeframe if visible, the pattern type, key indicator readings, conclusion the user might draw. Be specific and factual.";

#[derive(Deserialize)]
struct AnthropicNonStream {
    content: Vec<Block>,
}
#[derive(Deserialize)]
struct Block { #[allow(dead_code)] r#type: String, text: Option<String> }

pub async fn describe_image(api_key: &str, base_url: &str, model: &str, path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > 5 * 1024 * 1024 {
        return Err(AppError::Invalid(format!("image > 5MB: {}", path.display())));
    }
    let media_type = match path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return Err(AppError::Invalid("unsupported image type".into())),
    };
    let b64 = STANDARD.encode(&bytes);
    let body = json!({
        "model": model,
        "max_tokens": 600,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image", "source": {"type": "base64", "media_type": media_type, "data": b64}},
                {"type": "text", "text": DESCRIBE_PROMPT}
            ]
        }]
    });
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", base_url))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send().await?;
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(AppError::Anthropic(format!("HTTP {}: {}", s, t)));
    }
    let parsed: AnthropicNonStream = resp.json().await?;
    let txt = parsed.content.into_iter().filter_map(|b| b.text).collect::<Vec<_>>().join("\n").trim().to_string();
    if txt.is_empty() { return Err(AppError::Anthropic("empty description".into())); }
    Ok(txt)
}
```

Add to `Cargo.toml` under `[dependencies]`: `base64 = "0.22"`.

- [ ] **Step 2: Add ingest_image() in ingest/mod.rs**

Append:
```rust
pub async fn ingest_image(
    store: &LanceStore,
    api_key: &str,
    base_url: &str,
    model: &str,
    path: &Path,
) -> Result<IngestReport> {
    let desc = image::describe_image(api_key, base_url, model, path).await?;
    let chunks = chunker::chunk_text(&desc, 500, 50);
    let embs = embeddings::embed(chunks.clone())?;
    let path_str = path.to_string_lossy().to_string();
    let rows: Vec<DocRow> = chunks.iter().enumerate().zip(embs).map(|((i, t), e)| DocRow {
        id: Uuid::new_v4().to_string(),
        doc_path: path_str.clone(),
        doc_type: "image".into(),
        chunk_index: i as i32,
        text: t.clone(),
        page: None,
        embedding: e,
    }).collect();
    store.delete_by_doc_path(&path_str).await?;
    let n = rows.len();
    store.insert(rows).await?;
    Ok(IngestReport { doc_path: path_str, chunks: n })
}
```

- [ ] **Step 3: Wire into watcher**

In `watcher.rs`, after the PDF branch add:
```rust
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
```

- [ ] **Step 4: Build**

```powershell
cd src-tauri
cargo check
cd ..
```

Expected: compiles.

- [ ] **Step 5: Commit**

```powershell
git add -A
git commit -m "feat: image ingestion via Claude vision description"
```

---

## Phase 4 — Predictions & outcomes

### Task 18: Image utility (resize)

**Files:**
- Create: `src-tauri/src/image_util.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write image_util.rs**

```rust
use crate::error::Result;
use image::ImageReader;
use std::path::Path;

pub const MAX_LONG_EDGE: u32 = 1568;
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

pub fn needs_resize(path: &Path) -> Result<bool> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_BYTES { return Ok(true); }
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()
        .map_err(|e| crate::error::AppError::Internal(format!("decode: {}", e)))?;
    let (w, h) = (img.width(), img.height());
    Ok(w.max(h) > MAX_LONG_EDGE)
}

pub fn resize_in_place(path: &Path) -> Result<()> {
    let img = ImageReader::open(path)?.with_guessed_format()?.decode()
        .map_err(|e| crate::error::AppError::Internal(format!("decode: {}", e)))?;
    let (w, h) = (img.width(), img.height());
    let scale = MAX_LONG_EDGE as f32 / w.max(h) as f32;
    let nw = (w as f32 * scale) as u32;
    let nh = (h as f32 * scale) as u32;
    let resized = img.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);
    resized.save(path).map_err(|e| crate::error::AppError::Internal(format!("save: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn small_image_no_resize_needed() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("small.png");
        let buf: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_fn(100, 100, |_, _| Rgb([0u8, 0, 0]));
        buf.save(&p).unwrap();
        assert!(!needs_resize(&p).unwrap());
    }

    #[test]
    fn big_image_gets_resized() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("big.png");
        let buf: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_fn(2400, 1200, |_, _| Rgb([255u8, 255, 255]));
        buf.save(&p).unwrap();
        assert!(needs_resize(&p).unwrap());
        resize_in_place(&p).unwrap();
        let img = ImageReader::open(&p).unwrap().decode().unwrap();
        assert!(img.width().max(img.height()) <= MAX_LONG_EDGE);
    }
}
```

- [ ] **Step 2: Wire and test**

Add `pub mod image_util;` to `lib.rs`.

```powershell
cd src-tauri
cargo test image_util::tests
cd ..
```

Expected: 2 passed.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: image resize util (1568px long edge / 5MB threshold)"
```

---

### Task 19: CoinGecko price client

**Files:**
- Create: `src-tauri/src/coingecko.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write coingecko.rs**

```rust
use crate::error::{AppError, Result};
use serde::Deserialize;

const BASE: &str = "https://api.coingecko.com/api/v3";

/// Map common ticker (BTC, ETH, BTCUSDT, etc) to CoinGecko id.
pub fn symbol_to_id(symbol: &str) -> Option<&'static str> {
    let s = symbol.to_uppercase();
    let s = s.trim_end_matches("USDT").trim_end_matches("USD").trim_end_matches("USDC");
    match s {
        "BTC" => Some("bitcoin"),
        "ETH" => Some("ethereum"),
        "SOL" => Some("solana"),
        "BNB" => Some("binancecoin"),
        "XRP" => Some("ripple"),
        "ADA" => Some("cardano"),
        "DOGE" => Some("dogecoin"),
        "AVAX" => Some("avalanche-2"),
        "MATIC" => Some("matic-network"),
        "LINK" => Some("chainlink"),
        "DOT" => Some("polkadot"),
        "LTC" => Some("litecoin"),
        _ => None,
    }
}

#[derive(Deserialize)]
struct PriceResp(std::collections::HashMap<String, std::collections::HashMap<String, f64>>);

pub async fn get_usd_price(symbol: &str) -> Result<f64> {
    let id = symbol_to_id(symbol)
        .ok_or_else(|| AppError::Invalid(format!("unknown symbol for CoinGecko: {}", symbol)))?;
    let url = format!("{}/simple/price?ids={}&vs_currencies=usd", BASE, id);
    let r = reqwest::Client::new().get(&url).send().await?;
    if !r.status().is_success() {
        return Err(AppError::Internal(format!("coingecko HTTP {}", r.status())));
    }
    let body: PriceResp = r.json().await?;
    let price = body.0.get(id).and_then(|m| m.get("usd")).copied()
        .ok_or_else(|| AppError::Internal("price missing in response".into()))?;
    Ok(price)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_normalization() {
        assert_eq!(symbol_to_id("btc"), Some("bitcoin"));
        assert_eq!(symbol_to_id("BTCUSDT"), Some("bitcoin"));
        assert_eq!(symbol_to_id("ETHUSDC"), Some("ethereum"));
        assert_eq!(symbol_to_id("XYZ"), None);
    }
}
```

- [ ] **Step 2: Wire and test**

Add `pub mod coingecko;` to `lib.rs`.

```powershell
cd src-tauri
cargo test coingecko::tests
cd ..
```

Expected: 1 passed.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: CoinGecko price client with symbol mapping"
```

---

### Task 20: Predictions store + outcome resolver

**Files:**
- Create: `src-tauri/src/predictions.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write predictions.rs**

```rust
use crate::coingecko;
use crate::db::Db;
use crate::error::Result;
use crate::llm::types::{Action, AnalysisJson};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub id: i64,
    pub ts: i64,
    pub symbol: String,
    pub horizon_seconds: i64,
    pub predicted_prob_up: f64,
    pub recommended_action: String,
    pub stop_loss_pct: Option<f64>,
    pub take_profit_pct: Option<f64>,
    pub citations_json: Option<String>,
    pub outcome_status: String,
    pub outcome_price_at_ts: Option<f64>,
    pub outcome_price_at_horizon: Option<f64>,
    pub outcome_resolved_at: Option<i64>,
}

pub fn insert_from_analysis(db: &Db, symbol: &str, a: &AnalysisJson) -> Result<i64> {
    let ts = chrono::Utc::now().timestamp();
    let action = match a.action { Action::Buy => "buy", Action::Sell => "sell", Action::Hold => "hold" };
    let cit = serde_json::to_string(&a.citations).ok();
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO predictions(ts, symbol, horizon_seconds, predicted_prob_up, recommended_action, stop_loss_pct, take_profit_pct, citations_json)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![
            ts, symbol, a.horizon.seconds(),
            a.probability_up as f64, action,
            a.stop_loss_pct.map(|f| f as f64), a.take_profit_pct.map(|f| f as f64),
            cit
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn pending_due(db: &Db) -> Result<Vec<Prediction>> {
    let now = chrono::Utc::now().timestamp();
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, ts, symbol, horizon_seconds, predicted_prob_up, recommended_action, stop_loss_pct, take_profit_pct, citations_json, outcome_status, outcome_price_at_ts, outcome_price_at_horizon, outcome_resolved_at
         FROM predictions WHERE outcome_status='pending' AND (ts + horizon_seconds) <= ?1"
    )?;
    let rows = stmt.query_map([now], |r| Ok(Prediction {
        id: r.get(0)?, ts: r.get(1)?, symbol: r.get(2)?, horizon_seconds: r.get(3)?,
        predicted_prob_up: r.get(4)?, recommended_action: r.get(5)?,
        stop_loss_pct: r.get(6)?, take_profit_pct: r.get(7)?,
        citations_json: r.get(8)?, outcome_status: r.get(9)?,
        outcome_price_at_ts: r.get(10)?, outcome_price_at_horizon: r.get(11)?,
        outcome_resolved_at: r.get(12)?,
    }))?;
    Ok(rows.collect::<std::result::Result<_,_>>()?)
}

pub async fn resolve_one(db: &Db, p: &Prediction) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let price_now_res = coingecko::get_usd_price(&p.symbol).await;
    match price_now_res {
        Ok(price_now) => {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE predictions SET outcome_status='resolved', outcome_price_at_horizon=?1, outcome_resolved_at=?2 WHERE id=?3",
                rusqlite::params![price_now, now, p.id]
            )?;
        }
        Err(_) => {
            let conn = db.lock().unwrap();
            conn.execute("UPDATE predictions SET outcome_status='failed', outcome_resolved_at=?1 WHERE id=?2",
                rusqlite::params![now, p.id])?;
        }
    }
    Ok(())
}

pub fn record_initial_price(db: &Db, prediction_id: i64, price: f64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute("UPDATE predictions SET outcome_price_at_ts=?1 WHERE id=?2",
        rusqlite::params![price, prediction_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::llm::types::{Citation, Horizon};

    #[test]
    fn insert_then_pending_due() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("c.db")).unwrap();
        let a = AnalysisJson {
            action: Action::Buy, probability_up: 0.6, horizon: Horizon::H1,
            stop_loss_pct: Some(2.0), take_profit_pct: Some(4.0),
            key_signals: vec!["x".into()],
            citations: vec![Citation { doc: "doc.pdf".into(), page: Some(1) }],
        };
        let id = insert_from_analysis(&db, "BTCUSDT", &a).unwrap();
        // not yet due
        assert!(pending_due(&db).unwrap().is_empty());
        // shift its ts back so it becomes due
        {
            let conn = db.lock().unwrap();
            conn.execute("UPDATE predictions SET ts = ts - 7200 WHERE id=?1", [id]).unwrap();
        }
        assert_eq!(pending_due(&db).unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: Wire and test**

Add `pub mod predictions;` to `lib.rs`.

```powershell
cd src-tauri
cargo test predictions::tests
cd ..
```

Expected: 1 passed.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: predictions store and outcome resolver"
```

---

### Task 21: Background tasks — prune + outcome poller

**Files:**
- Create: `src-tauri/src/prune.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write prune.rs**

```rust
use crate::chat_store;
use crate::db::Db;
use crate::predictions;
use std::time::Duration;

pub fn spawn_background(db: Db) {
    let db_a = db.clone();
    tokio::spawn(async move {
        let interval = Duration::from_secs(6 * 3600);
        loop {
            if let Err(e) = chat_store::prune_older_than_secs(&db_a, 7 * 86400) {
                tracing::warn!("prune failed: {}", e);
            }
            tokio::time::sleep(interval).await;
        }
    });

    let db_b = db;
    tokio::spawn(async move {
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
```

- [ ] **Step 2: Wire**

Add `pub mod prune;` to `lib.rs`.

```powershell
cd src-tauri
cargo check
cd ..
```

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: background tasks for prune and outcome resolution"
```

---

## Phase 5 — Tray, IPC, and frontend wiring

### Task 22: Tray icon + slide-out panel positioning

**Files:**
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`

- [ ] **Step 1: Configure window in tauri.conf.json**

In `src-tauri/tauri.conf.json`, replace the `app.windows` array with:
```json
{
  "label": "main",
  "title": "CogniTrade",
  "width": 420,
  "height": 800,
  "resizable": true,
  "decorations": false,
  "transparent": false,
  "visible": false,
  "alwaysOnTop": true,
  "skipTaskbar": true
}
```

Also add `"trayIcon": { "iconPath": "icons/icon.png" }` under `app`.

- [ ] **Step 2: Write tray.rs**

```rust
use tauri::{
    tray::TrayIconBuilder,
    menu::{Menu, MenuItem},
    AppHandle, Manager, PhysicalPosition, PhysicalSize,
};

pub fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle_item = MenuItem::with_id(app, "toggle", "Toggle Panel", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_item, &quit_item])?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "toggle" => toggle_panel(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn toggle_panel(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let visible = win.is_visible().unwrap_or(false);
        if visible { let _ = win.hide(); }
        else {
            position_panel(&win);
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn position_panel(win: &tauri::WebviewWindow) {
    if let Ok(monitors) = win.available_monitors() {
        let monitor = win.current_monitor().ok().flatten().or_else(|| monitors.into_iter().next());
        if let Some(m) = monitor {
            let size = m.size();
            let pos = m.position();
            let panel_w = 420u32;
            let panel_h = 800u32;
            let scale = m.scale_factor();
            let x = pos.x + (size.width as f64 / scale) as i32 - panel_w as i32 - 16;
            let y = pos.y + 60;
            let _ = win.set_size(PhysicalSize::new((panel_w as f64 * scale) as u32, (panel_h as f64 * scale) as u32));
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
    }
}
```

- [ ] **Step 3: Hook into setup in lib.rs**

In `lib.rs`, modify `pub fn run()` so the builder calls `setup`:
```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            tray::init_tray(&app.handle())?;
            // Hide window on startup (already invisible per config); register hotkey:
            let app_handle = app.handle().clone();
            use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
            app_handle.global_shortcut().on_shortcut(shortcut, move |app, _, ev| {
                if ev.state == ShortcutState::Pressed {
                    tray::toggle_panel(app);
                }
            })?;
            app_handle.global_shortcut().register(shortcut)?;
            Ok(())
        })
        // .invoke_handler(...)  // added in Task 23
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Add `pub mod tray;` near other module declarations.

- [ ] **Step 4: Run dev**

```powershell
npm run tauri dev
```

Expected: tray icon appears. Click it → panel slides into right edge of screen. Press `Ctrl+Shift+Space` → also toggles. Close dev.

- [ ] **Step 5: Commit**

```powershell
git add -A
git commit -m "feat: tray icon, right-edge panel positioning, global hotkey"
```

---

### Task 23: IPC commands

**Files:**
- Create: `src-tauri/src/ipc.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write ipc.rs**

```rust
use crate::chat_store::{self, Message, NewMessage};
use crate::cost::{self, DailyCost};
use crate::db::{self, Db};
use crate::error::{AppError, Result};
use crate::ingest::{self, ingest_pdf, ingest_image};
use crate::lance::{self, LanceStore};
use crate::llm::client::{LlmClient, analyze_with_cap};
use crate::llm::prompt::{ChatMessage, PromptInputs};
use crate::llm::parse::extract_json_tail;
use crate::predictions;
use crate::retrieval;
use crate::settings::{self, Settings};
use crate::paths::Paths;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
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
        return Ok(LanceStore { conn: s.conn.clone() });
    }
    drop(g);
    let store = lance::open(&state.paths.lancedb()).await?;
    let mut g = state.lance.lock().await;
    *g = Some(LanceStore { conn: store.conn.clone() });
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
```

- [ ] **Step 2: Register handlers in lib.rs**

In `pub fn run()`, replace the placeholder line with:
```rust
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
])
.manage({
    let paths = paths::Paths::from_env();
    paths.ensure_all().expect("create CogniTrade dirs");
    let db = db::open(&paths.chat_db()).expect("open chat db");
    crate::prune::spawn_background(db.clone());
    let library = paths.library();
    {
        let library2 = library.clone();
        let store_path = paths.lancedb();
        tokio::spawn(async move {
            if let Ok(store) = lance::open(&store_path).await {
                let _ = ingest::watcher::run_watcher(store, library2);
            }
        });
    }
    ipc::AppState {
        db,
        lance: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        paths,
        llm: crate::llm::client::LlmClient::new(),
    }
})
```

Add `pub mod ipc;` to module list.

- [ ] **Step 3: Build**

```powershell
cd src-tauri
cargo check
cd ..
```

Expected: compiles.

- [ ] **Step 4: Commit**

```powershell
git add -A
git commit -m "feat: IPC commands for chat, settings, ingest, analyze"
```

---

## Phase 6 — Frontend

### Task 24: TypeScript IPC layer and shared types

**Files:**
- Create: `src/lib/types.ts`, `src/lib/ipc.ts`

- [ ] **Step 1: Write types.ts**

```typescript
export type Role = 'user' | 'assistant' | 'system';

export interface Message {
  id: number;
  ts: number;
  role: Role;
  content: string;
  image_path?: string | null;
  prediction_id?: number | null;
}

export type Action = 'buy' | 'sell' | 'hold';
export type Horizon = '1h' | '4h' | '1d' | '3d' | '1w';

export interface Citation { doc: string; page?: number; }

export interface AnalysisJson {
  action: Action;
  probability_up: number;
  horizon: Horizon;
  stop_loss_pct?: number;
  take_profit_pct?: number;
  key_signals: string[];
  citations: Citation[];
}

export interface DailyCost {
  date: string;
  input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  cost_usd: number;
}

export interface Settings {
  daily_cost_cap_usd: number;
  model_main: string;
  model_extract: string;
  library_path: string | null;
  hotkey: string;
}
```

- [ ] **Step 2: Write ipc.ts**

```typescript
import { invoke } from '@tauri-apps/api/core';
import type { Message, Settings, DailyCost, AnalysisJson } from './types';

export const api = {
  listRecentMessages: (limit = 50) => invoke<Message[]>('list_recent_messages', { limit }),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (value: Settings) => invoke<void>('save_settings', { value }),
  getApiKeySet: () => invoke<boolean>('get_api_key_set'),
  setApiKey: (value: string) => invoke<void>('set_api_key', { value }),
  clearApiKey: () => invoke<void>('clear_api_key'),
  costToday: () => invoke<DailyCost>('cost_today'),
  saveScreenshot: (base64Png: string, filenameHint?: string) =>
    invoke<string>('save_screenshot', { base64Png, filenameHint }),
  analyze: (req: { user_text: string; screenshot_path?: string; symbol_hint?: string }) =>
    invoke<{ message_id: number }>('analyze', { req }),
  ingestPath: (path: string) => invoke<number>('ingest_path', { path }),
};

export type { Message, Settings, DailyCost, AnalysisJson };
```

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: typed IPC client for the Svelte frontend"
```

---

### Task 25: Stores

**Files:**
- Create: `src/lib/stores/chat.ts`, `src/lib/stores/settings.ts`, `src/lib/stores/library.ts`

- [ ] **Step 1: Write chat.ts**

```typescript
import { writable, get } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import { api } from '../ipc';
import type { Message, AnalysisJson } from '../types';

export const messages = writable<Message[]>([]);
export const streaming = writable<string>('');     // assistant text being streamed
export const analysisResult = writable<AnalysisJson | null>(null);
export const isAnalyzing = writable<boolean>(false);

export async function refreshMessages() {
  messages.set(await api.listRecentMessages(50));
}

let unlistenChunk: (() => void) | null = null;
let unlistenDone: (() => void) | null = null;

export async function initEvents() {
  if (unlistenChunk) return;
  unlistenChunk = await listen<string>('analysis_chunk', (e) => {
    streaming.update((s) => s + e.payload);
  });
  unlistenDone = await listen<{ message_id: number; analysis: AnalysisJson | null }>(
    'analysis_done',
    async (e) => {
      analysisResult.set(e.payload.analysis);
      streaming.set('');
      isAnalyzing.set(false);
      await refreshMessages();
    }
  );
}

export async function sendAnalyze(opts: { text: string; screenshotPath?: string; symbolHint?: string }) {
  isAnalyzing.set(true);
  streaming.set('');
  analysisResult.set(null);
  try {
    await api.analyze({
      user_text: opts.text,
      screenshot_path: opts.screenshotPath,
      symbol_hint: opts.symbolHint,
    });
  } catch (e) {
    isAnalyzing.set(false);
    streaming.set(`\n\n[error: ${String(e)}]`);
  } finally {
    // Done event will refresh; but if it failed before any event, refresh anyway:
    if (get(isAnalyzing)) {
      isAnalyzing.set(false);
      await refreshMessages();
    }
  }
}
```

- [ ] **Step 2: Write settings.ts**

```typescript
import { writable } from 'svelte/store';
import { api } from '../ipc';
import type { Settings, DailyCost } from '../types';

export const settings = writable<Settings | null>(null);
export const apiKeySet = writable<boolean>(false);
export const cost = writable<DailyCost | null>(null);

export async function refreshAll() {
  settings.set(await api.getSettings());
  apiKeySet.set(await api.getApiKeySet());
  cost.set(await api.costToday());
}

export async function save(s: Settings) {
  await api.saveSettings(s);
  settings.set(s);
}

export async function setKey(k: string) {
  await api.setApiKey(k);
  apiKeySet.set(true);
}

export async function clearKey() {
  await api.clearApiKey();
  apiKeySet.set(false);
}
```

- [ ] **Step 3: Write library.ts**

```typescript
import { writable } from 'svelte/store';
import { api } from '../ipc';

export const ingestStatus = writable<{ path: string; chunks: number; ok: boolean; err?: string }[]>([]);

export async function ingest(path: string) {
  try {
    const chunks = await api.ingestPath(path);
    ingestStatus.update((arr) => [...arr, { path, chunks, ok: true }]);
  } catch (e) {
    ingestStatus.update((arr) => [...arr, { path, chunks: 0, ok: false, err: String(e) }]);
  }
}
```

- [ ] **Step 4: Commit**

```powershell
git add -A
git commit -m "feat: chat / settings / library Svelte stores"
```

---

### Task 26: Components — chat panel, message bubble, prediction card

**Files:**
- Create: `src/lib/components/MessageBubble.svelte`, `src/lib/components/PredictionCard.svelte`, `src/lib/components/ScreenshotPaste.svelte`, `src/lib/components/ChatPanel.svelte`

- [ ] **Step 1: PredictionCard.svelte**

```svelte
<script lang="ts">
  import type { AnalysisJson } from '../types';
  export let a: AnalysisJson;
  $: actionColor = a.action === 'buy' ? 'bg-green-600' : a.action === 'sell' ? 'bg-red-600' : 'bg-zinc-600';
  $: probPct = Math.round(a.probability_up * 100);
</script>

<div class="rounded-xl border border-zinc-700 p-3 my-2 bg-zinc-900">
  <div class="flex items-center justify-between">
    <span class={`px-2 py-1 rounded text-xs font-semibold uppercase ${actionColor}`}>{a.action}</span>
    <span class="text-sm text-zinc-300">P(up) <span class="font-semibold">{probPct}%</span> · {a.horizon}</span>
  </div>
  {#if a.stop_loss_pct != null || a.take_profit_pct != null}
    <div class="mt-2 text-xs text-zinc-400">
      SL: {a.stop_loss_pct ?? '—'}% · TP: {a.take_profit_pct ?? '—'}%
    </div>
  {/if}
  {#if a.key_signals?.length}
    <ul class="mt-2 text-xs text-zinc-300 list-disc list-inside">
      {#each a.key_signals as s}<li>{s}</li>{/each}
    </ul>
  {/if}
  {#if a.citations?.length}
    <div class="mt-2 text-xs text-zinc-500">
      Cited: {a.citations.map((c) => c.page ? `${c.doc}#${c.page}` : c.doc).join(', ')}
    </div>
  {/if}
</div>
```

- [ ] **Step 2: MessageBubble.svelte**

```svelte
<script lang="ts">
  import type { Message } from '../types';
  export let m: Message;
  $: isUser = m.role === 'user';
</script>

<div class={`my-2 flex ${isUser ? 'justify-end' : 'justify-start'}`}>
  <div class={`max-w-[85%] rounded-lg px-3 py-2 text-sm ${isUser ? 'bg-blue-600 text-white' : 'bg-zinc-800 text-zinc-100'}`}>
    {#if m.image_path}
      <img src={'file://' + m.image_path} alt="screenshot" class="mb-2 rounded max-h-48" />
    {/if}
    <pre class="whitespace-pre-wrap font-sans">{m.content}</pre>
  </div>
</div>
```

- [ ] **Step 3: ScreenshotPaste.svelte**

```svelte
<script lang="ts">
  import { api } from '../ipc';
  import { createEventDispatcher } from 'svelte';
  const dispatch = createEventDispatcher<{ saved: { path: string } }>();
  export let busy = false;

  async function onPaste(e: ClipboardEvent) {
    if (!e.clipboardData) return;
    const item = Array.from(e.clipboardData.items).find((it) => it.type.startsWith('image/'));
    if (!item) return;
    const blob = item.getAsFile();
    if (!blob) return;
    busy = true;
    const buf = await blob.arrayBuffer();
    const b64 = btoa(String.fromCharCode(...new Uint8Array(buf)));
    const path = await api.saveScreenshot(b64, 'paste');
    busy = false;
    dispatch('saved', { path });
  }
</script>

<svelte:window on:paste={onPaste} />
```

- [ ] **Step 4: ChatPanel.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { messages, streaming, analysisResult, isAnalyzing, refreshMessages, initEvents, sendAnalyze } from '../stores/chat';
  import MessageBubble from './MessageBubble.svelte';
  import PredictionCard from './PredictionCard.svelte';
  import ScreenshotPaste from './ScreenshotPaste.svelte';

  let text = '';
  let pendingScreenshot: string | undefined;
  let symbolHint = '';

  onMount(async () => {
    await refreshMessages();
    await initEvents();
  });

  async function send() {
    if (!text.trim() && !pendingScreenshot) return;
    const t = text;
    const ss = pendingScreenshot;
    const sh = symbolHint || undefined;
    text = ''; pendingScreenshot = undefined;
    await sendAnalyze({ text: t, screenshotPath: ss, symbolHint: sh });
  }

  function onPaste(e: CustomEvent<{ path: string }>) { pendingScreenshot = e.detail.path; }
</script>

<ScreenshotPaste on:saved={onPaste} />

<div class="flex flex-col h-full">
  <div class="flex-1 overflow-y-auto px-3 py-2">
    {#each $messages as m (m.id)}
      <MessageBubble {m} />
    {/each}
    {#if $streaming}
      <div class="my-2 flex justify-start">
        <div class="max-w-[85%] rounded-lg px-3 py-2 text-sm bg-zinc-800 text-zinc-100">
          <pre class="whitespace-pre-wrap font-sans">{$streaming}</pre>
        </div>
      </div>
    {/if}
    {#if $analysisResult}
      <PredictionCard a={$analysisResult} />
    {/if}
  </div>

  <div class="border-t border-zinc-800 p-2 space-y-2">
    {#if pendingScreenshot}
      <div class="text-xs text-zinc-400">📎 screenshot ready: {pendingScreenshot.split('\\').pop()}</div>
    {/if}
    <input bind:value={symbolHint} placeholder="symbol (e.g. BTCUSDT)" class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-sm" />
    <textarea bind:value={text} rows="2" placeholder="ask about the chart… (paste screenshot with Ctrl+V)" class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-sm"></textarea>
    <button on:click={send} disabled={$isAnalyzing} class="w-full rounded bg-blue-600 hover:bg-blue-500 disabled:bg-zinc-700 text-white py-2 text-sm font-medium">
      {$isAnalyzing ? 'Analyzing…' : 'Analyze'}
    </button>
  </div>
</div>
```

- [ ] **Step 5: Commit**

```powershell
git add -A
git commit -m "feat: chat panel, message bubble, prediction card, screenshot paste"
```

---

### Task 27: Library and Settings views

**Files:**
- Create: `src/lib/components/LibraryView.svelte`, `src/lib/components/SettingsView.svelte`

- [ ] **Step 1: LibraryView.svelte**

```svelte
<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { ingestStatus, ingest } from '../stores/library';

  async function pickFiles() {
    const sel = await open({
      multiple: true,
      filters: [{ name: 'PDF and images', extensions: ['pdf', 'png', 'jpg', 'jpeg', 'webp'] }],
    });
    const list = Array.isArray(sel) ? sel : sel ? [sel] : [];
    for (const p of list) await ingest(p);
  }
</script>

<div class="p-3 space-y-3">
  <button on:click={pickFiles} class="rounded bg-blue-600 hover:bg-blue-500 px-3 py-2 text-sm">Add documents</button>
  <p class="text-xs text-zinc-400">PDFs and chart images go here. Files are also picked up automatically when dropped into ~/CogniTrade/library.</p>
  <div class="space-y-1 text-xs">
    {#each $ingestStatus as s}
      <div class="text-zinc-300">
        {s.ok ? '✅' : '❌'} {s.path.split(/[\\/]/).pop()} {s.ok ? `(${s.chunks} chunks)` : `— ${s.err}`}
      </div>
    {/each}
  </div>
</div>
```

- [ ] **Step 2: SettingsView.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { settings, apiKeySet, cost, refreshAll, save, setKey, clearKey } from '../stores/settings';

  let key = '';
  let cap = 2;
  let model = 'claude-sonnet-4-6';

  onMount(async () => {
    await refreshAll();
    if ($settings) {
      cap = $settings.daily_cost_cap_usd;
      model = $settings.model_main;
    }
  });

  async function persist() {
    if (!$settings) return;
    await save({ ...$settings, daily_cost_cap_usd: cap, model_main: model });
  }
</script>

<div class="p-3 space-y-4 text-sm">
  <section>
    <h3 class="font-semibold mb-1">API key</h3>
    {#if $apiKeySet}
      <div class="text-zinc-300">key is set</div>
      <button on:click={clearKey} class="mt-1 px-2 py-1 rounded bg-red-600 hover:bg-red-500 text-xs">Clear</button>
    {:else}
      <input type="password" bind:value={key} placeholder="sk-ant-…" class="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1" />
      <button on:click={() => setKey(key)} class="mt-1 px-2 py-1 rounded bg-blue-600 hover:bg-blue-500 text-xs">Save key</button>
    {/if}
  </section>
  <section>
    <h3 class="font-semibold mb-1">Daily cap (USD)</h3>
    <input type="number" min="0.5" step="0.5" bind:value={cap} class="w-32 bg-zinc-900 border border-zinc-700 rounded px-2 py-1" />
    {#if $cost}
      <div class="text-xs text-zinc-400 mt-1">today: ${$cost.cost_usd.toFixed(3)}</div>
    {/if}
  </section>
  <section>
    <h3 class="font-semibold mb-1">Main model</h3>
    <select bind:value={model} class="bg-zinc-900 border border-zinc-700 rounded px-2 py-1">
      <option value="claude-sonnet-4-6">claude-sonnet-4-6 (default)</option>
      <option value="claude-opus-4-7">claude-opus-4-7 (slower, costlier)</option>
    </select>
  </section>
  <button on:click={persist} class="px-3 py-2 rounded bg-blue-600 hover:bg-blue-500">Save</button>
</div>
```

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: library and settings views"
```

---

### Task 28: App.svelte view switching

**Files:**
- Modify: `src/App.svelte`

- [ ] **Step 1: Replace App.svelte**

```svelte
<script lang="ts">
  import ChatPanel from './lib/components/ChatPanel.svelte';
  import LibraryView from './lib/components/LibraryView.svelte';
  import SettingsView from './lib/components/SettingsView.svelte';
  type Tab = 'chat' | 'library' | 'settings';
  let tab: Tab = 'chat';
</script>

<div class="flex flex-col h-screen">
  <header class="flex items-center gap-2 px-3 py-2 border-b border-zinc-800 bg-zinc-950">
    <span class="font-semibold text-sm">CogniTrade</span>
    <nav class="ml-auto text-xs flex gap-3">
      <button class:underline={tab === 'chat'} on:click={() => (tab = 'chat')}>Chat</button>
      <button class:underline={tab === 'library'} on:click={() => (tab = 'library')}>Library</button>
      <button class:underline={tab === 'settings'} on:click={() => (tab = 'settings')}>Settings</button>
    </nav>
  </header>
  <main class="flex-1 overflow-hidden">
    {#if tab === 'chat'}<ChatPanel />{/if}
    {#if tab === 'library'}<LibraryView />{/if}
    {#if tab === 'settings'}<SettingsView />{/if}
  </main>
</div>
```

- [ ] **Step 2: Visually verify**

```powershell
npm run tauri dev
```

Expected: tray app opens, panel shows three tabs, all render without console errors. Close.

- [ ] **Step 3: Commit**

```powershell
git add -A
git commit -m "feat: tabbed app shell (chat/library/settings)"
```

---

## Phase 7 — Polish

### Task 29: CLAUDE.md update with build/run/test commands

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Append a Commands section**

Add after the existing "Repository status" section in `CLAUDE.md`:

```markdown
## Commands

- `npm install` — install JS deps (run once after clone)
- `npm run tauri dev` — run the app (dev) with hot reload
- `npm run tauri build` — produce a release MSI installer (Windows)
- `cd src-tauri && cargo test` — run all backend Rust tests (some integration tests are `#[ignore]`d; pass `-- --ignored` to include them)
- `cd src-tauri && cargo test <module>::tests` — run a single module's tests
- `cd src-tauri && cargo test --test ingest_integration -- --ignored --nocapture` — manual full-pipeline smoke test (downloads BGE-small on first run)

## Where things live (post-implementation)

- Rust backend: `src-tauri/src/` — see Phase 1–5 of `docs/superpowers/plans/2026-05-05-cognitrade-v1.md`
- Svelte frontend: `src/` — chat panel, library, settings, IPC wrappers
- User data (NEVER commit): `%USERPROFILE%\CogniTrade\library\`, `screenshots\`, `data\`, `logs\`
```

- [ ] **Step 2: Commit**

```powershell
git add -A
git commit -m "docs: add commands and post-impl pointers to CLAUDE.md"
```

---

### Task 30: Manual smoke walkthrough

> **Status (2026-05-06):** Pending. Cannot execute until C++ Build Tools are repaired on the workstation. All preceding Rust+frontend code is laid down and committed; resume here once `cargo test` and `npm run tauri dev` work.

This task is verification, not new code. Run the app end-to-end and check the listed behaviors. Open an issue/note if any item fails.

- [ ] **Step 1: Set API key**

```powershell
npm run tauri dev
```
Open Settings → paste a real Anthropic API key → click Save.

- [ ] **Step 2: Add one PDF and one chart image to the library**

Drop both into `%USERPROFILE%\CogniTrade\library\`. Wait ~5 seconds. Confirm in the app's logs (or terminal) that ingestion ran (you should see "ingested … N chunks" lines).

- [ ] **Step 3: Send a screenshot**

Take a screenshot (`Win+Shift+S`), paste it into the chat panel (`Ctrl+V`), type a question like "is this a buy or hold?" and click Analyze.

- [ ] **Step 4: Verify**

- Streaming text appears chunk by chunk.
- A prediction card renders (action, P(up)%, horizon, SL/TP, citations).
- `cost.cost_usd` increased.
- A row appears in the predictions table (inspect via `sqlite3 ~/CogniTrade/data/chat.db ".dump predictions"`).

- [ ] **Step 5: Verify cap enforcement**

Set the daily cap to `0.001` in Settings, then try another analysis. Expected: `daily cost cap reached` error in chat.

- [ ] **Step 6: Verify 7-day prune (optional, slow)**

Manually backdate a row: `UPDATE messages SET ts = ts - (8 * 86400) WHERE id=1;`. Trigger the prune (restart app and wait 5 min, or manually call from a debug REPL). Confirm the row is gone.

- [ ] **Step 7: Commit any tweaks**

If anything needed fixing, commit per task. Otherwise just tag the milestone:

```powershell
git tag v0.1.0-smoke-pass
```

---

## Phase 8 — Release packaging

### Task 31: Build MSI installer

> **Status (2026-05-06):** Pending. Requires `npm run tauri build`, which requires the C++ linker on this Windows machine. Resume after Task 30 passes.

- [ ] **Step 1: Build**

```powershell
npm run tauri build
```

Expected output: an MSI/EXE under `src-tauri/target/release/bundle/msi/`.

- [ ] **Step 2: Test the installer on a clean directory**

Install, launch from Start menu, repeat the smoke walkthrough (Task 30) on the installed binary.

- [ ] **Step 3: Commit any installer config tweaks**

If `tauri.conf.json` needed adjustments (icon paths, identifier, productName), commit them. Otherwise no commit.

---

## Notes for the executor

- After Task 7 (cost tracker), every Anthropic-touching code path must go through `analyze_with_cap` for the streaming chart-analysis flow. The two non-streaming Anthropic calls — image-ingestion description (Task 17) and symbol extraction (Task 23) — are kept outside the cap wrapper for v1; they are bounded by user action and individually cheap. Track this as a v2 follow-up if usage patterns change.
- API-key log scrubbing (spec §11) is a v2 polish item. v1 mitigates by never logging the key value at all: it is only read via `settings::require_api_key()` and passed directly to the `x-api-key` HTTP header. Do not add `tracing::*!("…{}…", api_key)` calls anywhere.
- Tests that touch the network or download large model weights are `#[ignore]`'d. They are surfaced in the manual smoke walkthrough (Task 30).
- The image-ingestion path (Task 17) currently does **not** count toward the cost cap; this is acceptable for v1 because each image describe is cheap (~$0.001) and bounded by user action. Revisit if image ingestion grows to bulk imports.
- If `cargo create-tauri-app` produces files at a different path than expected (Task 1), reconcile before continuing rather than fighting the scaffold.
- The `cognitrade_lib` library name (Task 11) MUST match what `Cargo.toml` declares; test files import from it.
