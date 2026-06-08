# Data Model & Storage

This document is the authoritative reference for **every persistent store, table, and on-disk artifact** in CogniTrade. It describes only what the code actually does. Where a capability appears in the product spec (`crypto Dhriti.txt`, `docs/superpowers/`) but is not present in the implementation, it is labelled **Planned — not yet implemented**.

CogniTrade is an **advisory** crypto-chart analyst built as a Tauri 2 desktop app (Rust backend + SvelteKit/TypeScript frontend). It does **not** execute trades. All persistence is local on the user's machine; nothing is committed to source control.

Related docs:

- [Request flow & IPC](./request-flow.md) — how `ipc::analyze` reads and writes these stores (if present).
- [Ingestion pipeline](./ingestion.md) — how `doc_chunks` is populated.
- [Cost control](./cost-control.md) — how `cost_daily` is maintained and enforced.

> Cross-links above are relative to `docs/architecture/`. Some target documents may not yet exist; the canonical source remains the Rust modules cited inline.

---

## 1. Storage overview

CogniTrade persists state in four distinct places:

| Store | Kind | Location | Opened by | Contents |
|-------|------|----------|-----------|----------|
| `chat.db` | SQLite (WAL) | `%USERPROFILE%\CogniTrade\data\chat.db` | `db::open` (`src-tauri/src/db.rs`) | `messages`, `predictions`, `cost_daily`, `settings` |
| `vec.db` | SQLite (WAL) | `%USERPROFILE%\CogniTrade\data\vec.db` | `lance::open` (`src-tauri/src/lance.rs`) | `doc_chunks` (RAG vector store) |
| Anthropic API key | OS keyring secret | Windows Credential Manager | `settings.rs` (`keyring` crate) | one secret string, **never in any DB** |
| File tree | Plain files | `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` | `paths::Paths` (`src-tauri/src/paths.rs`) | source PDFs/images, saved screenshots, DB files, logs |

All paths derive from `dirs::home_dir()` joined with `CogniTrade` (see `Paths::new`). On Windows that root is `%USERPROFILE%\CogniTrade`.

### Why two separate SQLite databases?

`chat.db` and `vec.db` are opened independently and never joined. They are deliberately split:

- **Different access patterns.** `chat.db` holds small, transactional rows (chat messages, prediction outcomes, cost accumulation, key/value settings) read and written on the request path. `vec.db` holds large embedding BLOBs scanned in bulk: `LanceStore::search` loads **every** `doc_chunks` row and computes cosine similarity in Rust on each query (brute-force, no index on the vector). Keeping the heavy full-table-scan workload in its own file keeps it from contending with the lightweight transactional DB.
- **Different lifecycles.** The chat DB is migrated from a checked-in migration file (`migrations/001_initial.sql`, embedded via `include_str!`); the vector DB's schema is created inline by `lance.rs` (`SCHEMA` constant). The vector store can be deleted and rebuilt by re-ingesting the `library` folder without touching chat history, predictions, or cost data.
- **Lazy open.** `vec.db` is wrapped in `Arc<Mutex<Option<LanceStore>>>` in `AppState` and opened lazily on first ingestion/retrieval, whereas `chat.db` is opened eagerly at startup. Separate files make that lazy/eager split clean.

> **Naming caveat.** `lance.rs`, the `LanceStore` type, and `Paths::lancedb()` are **misnomers**. There is no LanceDB dependency. `vec.db` is plain SQLite; embeddings are stored as `f32` BLOBs (see §4). Do not assume any LanceDB / Arrow behavior.

Both databases run in WAL mode. `chat.db` additionally enables `PRAGMA foreign_keys=ON` (set in `db::open`); `vec.db` sets only `PRAGMA journal_mode=WAL`. Both use `rusqlite` 0.31 with the bundled SQLite.

---

## 2. `chat.db` schema (`migrations/001_initial.sql`)

The schema is defined once in `src-tauri/migrations/001_initial.sql` and applied verbatim by `db::open` on every startup (all statements are `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`, so re-running is idempotent). There is currently a **single migration**; there is no migration-version tracking table.

### 2.1 `messages`

One row per chat turn (user prompt or assistant analysis). Written by `chat_store::insert`; read by `chat_store::recent`; pruned by `chat_store::prune_older_than_secs`.

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
```

| Column | Type | Constraints | Meaning |
|--------|------|-------------|---------|
| `id` | INTEGER | PRIMARY KEY AUTOINCREMENT | Surrogate row id. |
| `ts` | INTEGER | NOT NULL | Unix epoch **seconds** (`chrono::Utc::now().timestamp()`). |
| `role` | TEXT | NOT NULL, CHECK in (`user`,`assistant`,`system`) | Author of the turn. In practice the backend writes `user` and `assistant`. |
| `content` | TEXT | NOT NULL | Message text. For assistant rows this is the full streamed analysis (including the trailing fenced ```json block). |
| `image_path` | TEXT | nullable | Absolute path to an attached screenshot (under `screenshots/`), or NULL. |
| `prediction_id` | INTEGER | FK → `predictions(id)`, nullable | Links an assistant analysis to the `predictions` row it produced. NULL for user turns and analyses with no parseable recommendation. |

**Indexes:** `idx_messages_ts` on `ts` (supports the time-ordered `recent` query — `ORDER BY ts DESC LIMIT ?` — and the prune `WHERE ts < ?`).

**Notes**

- The foreign key to `predictions` is enforced (`PRAGMA foreign_keys=ON`). The FK has no `ON DELETE` action, so it defaults to `NO ACTION`: a `predictions` row cannot be deleted while a `messages` row still references it. (The app never deletes prediction rows in code.)
- The Rust struct is `chat_store::Message` (`id, ts, role, content, image_path, prediction_id`); inserts go through `chat_store::NewMessage` (no `id`/`ts` — both are assigned on insert).

### 2.2 `predictions`

The self-calibration loop. One row per analysis that yielded a parseable recommendation. Written by `predictions::insert_from_analysis`, back-filled with the entry price by `predictions::record_initial_price` (called from a fire-and-forget task in `ipc::analyze` after a CoinGecko lookup — see §6.2), and resolved by `predictions::resolve_one`.

```sql
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
```

| Column | Type | Constraints | Meaning |
|--------|------|-------------|---------|
| `id` | INTEGER | PRIMARY KEY AUTOINCREMENT | Surrogate id; referenced by `messages.prediction_id`. |
| `ts` | INTEGER | NOT NULL | Unix epoch seconds when the prediction was made. |
| `symbol` | TEXT | NOT NULL | Resolved trading symbol (e.g. `BTCUSDT`). Used by CoinGecko lookup at resolution time. |
| `horizon_seconds` | INTEGER | NOT NULL | Prediction horizon in seconds, from `AnalysisJson.horizon` via `Horizon::seconds()`. The five horizons map to an exact integer domain: `1h`=3600, `4h`=14400, `1d`=86400, `3d`=259200, `1w`=604800. No other values are persisted. |
| `predicted_prob_up` | REAL | NOT NULL | Model's estimated probability that price moves up over the horizon (`AnalysisJson.probability_up`, 0.0–1.0). |
| `recommended_action` | TEXT | NOT NULL | `"buy"`, `"sell"`, or `"hold"` (mapped from the `Action` enum). |
| `stop_loss_pct` | REAL | nullable | Suggested stop-loss percentage, or NULL if the model omitted it. **Advisory only** — see note below. |
| `take_profit_pct` | REAL | nullable | Suggested take-profit percentage, or NULL. Advisory only. |
| `citations_json` | TEXT | nullable | `AnalysisJson.citations` serialized to JSON (`[{doc, page}]`), or NULL. |
| `outcome_status` | TEXT | NOT NULL, DEFAULT `'pending'` | Resolution state: `pending` → `resolved` or `failed`. See §6. |
| `outcome_price_at_ts` | REAL | nullable | Spot price at prediction time. The CoinGecko lookup runs in `ipc::analyze` (fire-and-forget `tokio::spawn`); `predictions::record_initial_price` only persists the resulting price into this column. NULL until/if recorded (e.g. the lookup failed). |
| `outcome_price_at_horizon` | REAL | nullable | Spot price at the moment of resolution (CoinGecko). NULL until resolved. |
| `outcome_resolved_at` | INTEGER | nullable | Unix epoch seconds when resolution ran (set for both `resolved` and `failed`). |

**Indexes:** none beyond the implicit primary key. The resolution query `WHERE outcome_status='pending' AND (ts + horizon_seconds) <= ?` (`predictions::pending_due`) is a table scan; acceptable at the expected row counts.

> **Important — `stop_loss_pct` / `take_profit_pct` are advisory.** These are values the model *suggested*; they are stored for the human's reference and for context. There is **no automated SL/TP enforcement and no order execution** in the codebase. The spec's "hard SL/TP enforcement" is **Planned — not yet implemented**. Resolution records `outcome_price_at_horizon` at the horizon and does **not** simulate whether SL or TP was hit intra-horizon.

The matching Rust struct is `predictions::Prediction` with all columns one-to-one.

### 2.3 `cost_daily`

Per-day token usage and spend, keyed by calendar date. Maintained by `cost::add_usage` (upsert) and read by `cost::today`. Drives the daily cost cap.

```sql
CREATE TABLE IF NOT EXISTS cost_daily (
  date TEXT PRIMARY KEY,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  cached_input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL NOT NULL DEFAULT 0
);
```

| Column | Type | Constraints | Meaning |
|--------|------|-------------|---------|
| `date` | TEXT | PRIMARY KEY | Calendar day as `YYYY-MM-DD` (UTC, `Utc::now().format("%Y-%m-%d")`). |
| `input_tokens` | INTEGER | NOT NULL, DEFAULT 0 | Sum of uncached input tokens for the day. |
| `cached_input_tokens` | INTEGER | NOT NULL, DEFAULT 0 | Sum of prompt-cache (cached input) tokens for the day. |
| `output_tokens` | INTEGER | NOT NULL, DEFAULT 0 | Sum of output tokens for the day. |
| `cost_usd` | REAL | NOT NULL, DEFAULT 0 | Accumulated USD cost for the day. |

**Upsert semantics:** `add_usage` does `INSERT ... ON CONFLICT(date) DO UPDATE SET col = col + excluded.col`, so repeated calls within a day accumulate. Cost is computed per call by `cost::cost_usd` using `cost::rates_for(model)`, which matches on model-id substrings:

| Model substring | Input $/Mtok | Cached input $/Mtok | Output $/Mtok |
|-----------------|-------------|----------------------|---------------|
| `opus` (Opus 4.7) | 15.0 | 1.50 | 75.0 |
| `haiku` (Haiku 4.5) | 1.0 | 0.10 | 5.0 |
| else (Sonnet 4.6 default) | 3.0 | 0.30 | 15.0 |

Token counts come from the Anthropic streaming response usage. The cap is enforced **before** the main call in `analyze_with_cap` against `settings.daily_cost_cap_usd` (default `$2`); over-cap returns `AppError::CostCapReached`. `cost::is_over_cap` treats `today_cost >= cap` as over.

### 2.4 `settings`

A simple key/value table for **non-secret** configuration. Written by `settings::save`, read by `settings::load`.

```sql
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

| Column | Type | Constraints | Meaning |
|--------|------|-------------|---------|
| `key` | TEXT | PRIMARY KEY | Setting name. |
| `value` | TEXT | NOT NULL | Stringified value (numbers/bools stored as text). |

**Recognized keys** (the typed `settings::Settings` struct; unknown keys are ignored on load, defaults fill the rest):

| Key | Default | Maps to |
|-----|---------|---------|
| `daily_cost_cap_usd` | `2.0` | `f64`, parsed from text. The daily spend cap. |
| `model_main` | `claude-sonnet-4-6` | Model for the main analysis call. |
| `model_extract` | `claude-haiku-4-5-20251001` | Model for vision symbol extraction (`analyze`) and the **manual** `ipc::ingest_path` image-description call. **Caveat:** the background library watcher (`ingest/watcher.rs`) hardcodes the literal `claude-haiku-4-5-20251001` for image description and does **not** read this setting, so changing `model_extract` only affects analyze symbol extraction and manual ingestion. |
| `library_path` | (unset → default `~/CogniTrade/library`) | Optional override for the watched library directory. Empty string ⇒ treated as default. |
| `hotkey` | `Ctrl+Shift+Space` | Global hotkey to toggle the panel. |

`settings::save` upserts all five keys with `ON CONFLICT(key) DO UPDATE SET value=excluded.value`. **The API key is not one of these keys** — it lives in the OS keyring (§3).

---

## 3. The Anthropic API key — OS keyring secret (NOT in any DB)

The Anthropic API key is the only true secret and is stored **outside SQLite**, in the OS credential store via the `keyring` crate (`settings.rs`):

| Field | Value |
|-------|-------|
| Service | `cognitrade` (`KEYRING_SERVICE`) |
| User / account | `anthropic_api_key` (`KEYRING_USER`) |
| Backing store (Windows) | **Windows Credential Manager** |

Access functions:

- `settings::get_api_key()` → `Ok(Some(key))`, or `Ok(None)` if `keyring::Error::NoEntry`, else `AppError::Keyring`.
- `settings::set_api_key(value)` → stores/overwrites the secret.
- `settings::clear_api_key()` → deletes it (no-op if already absent).
- `settings::require_api_key()` → returns the key or `AppError::NoApiKey`.

> The key is never written to `chat.db`, `vec.db`, logs, or any file under `CogniTrade\`. Do not add a `settings`-table row for it. On non-Windows platforms `keyring` uses that OS's native store (Secret Service / macOS Keychain); on Windows it is Credential Manager.

---

## 4. `vec.db` schema — `doc_chunks` and embedding encoding (`lance.rs`)

`vec.db` holds the RAG corpus. Its single table is created inline by the `SCHEMA` constant in `lance.rs` (not from the migration file):

```sql
CREATE TABLE IF NOT EXISTS doc_chunks (
  id TEXT PRIMARY KEY,
  doc_path TEXT NOT NULL,
  doc_type TEXT NOT NULL,
  chunk_index INTEGER NOT NULL,
  text TEXT NOT NULL,
  page INTEGER,
  embedding BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_doc_chunks_path ON doc_chunks(doc_path);
```

| Column | Type | Constraints | Meaning |
|--------|------|-------------|---------|
| `id` | TEXT | PRIMARY KEY | A `uuid::Uuid::new_v4()` string, generated per chunk at ingest. |
| `doc_path` | TEXT | NOT NULL | Absolute source path of the document (`path.to_string_lossy()`). Used as the natural key for replace-on-reingest. |
| `doc_type` | TEXT | NOT NULL | `"pdf"` or `"image"` (set by `ingest_pdf` / `ingest_image`). |
| `chunk_index` | INTEGER | NOT NULL | 0-based ordinal of the chunk within its document. |
| `text` | TEXT | NOT NULL | The chunk's plain text (the retrievable passage). |
| `page` | INTEGER | nullable | Page number; currently always `NULL` in both ingest paths (chunking is page-agnostic). |
| `embedding` | BLOB | NOT NULL | The 384-dim vector encoded as raw `f32` bytes — see below. |

**Indexes:** `idx_doc_chunks_path` on `doc_path` (supports `delete_by_doc_path`, the replace-on-reingest step). There is **no** index on `embedding`; search is a full scan.

### 4.1 Embedding BLOB encoding (little-endian `f32`, 384 dims)

Embeddings are produced by `fastembed` **BGE-small** (`EmbeddingModel::BGESmallENV15`) via `ingest::embeddings::embed`, a lazily-initialized global singleton (`OnceLock<Mutex<TextEmbedding>>`, CPU-only, synchronous behind the mutex; the model downloads on first use). The dimensionality is fixed:

```rust
pub const EMBED_DIM: usize = 384; // BGE-small dim
```

Encoding/decoding (`lance.rs`):

- **Encode (`embedding_to_bytes`)**: each `f32` is serialized with `f.to_le_bytes()` (**little-endian**, 4 bytes) and concatenated. A 384-dim vector → **1536 bytes**.
- **Decode (`bytes_to_embedding`)**: the BLOB is read in 4-byte chunks via `f32::from_le_bytes`. If the byte length is not a multiple of 4 it returns an empty vector (treated as a non-matching row by cosine).
- **Validation on insert (`LanceStore::insert`)**: every row's `embedding.len()` must equal `EMBED_DIM` (384), else `AppError::Invalid("embedding dim mismatch ...")`. Inserts use `INSERT OR REPLACE` inside a single transaction.
- **Search (`LanceStore::search`)**: if the query vector isn't 384-dim it returns empty. Otherwise it loads **all** rows, computes cosine similarity in Rust (`cosine`), sorts descending, and truncates to top-`k`. This is brute-force; it is the obvious bottleneck if the library grows large. (Approximate-nearest-neighbor indexing is **Planned — not yet implemented**.)

> Because the encoding is endianness-specific (`to_le_bytes`/`from_le_bytes`), `vec.db` is only portable across little-endian hosts. The Windows x86-64 target is little-endian, so this is not an issue in practice.

### 4.2 How rows are produced (ingestion summary)

- **PDF** (`ingest::ingest_pdf`): `pdf-extract` pulls text → `chunker::chunk_text(text, 500, 50)` → embed → `delete_by_doc_path(path)` then `insert(rows)` (replace-on-reingest). `doc_type = "pdf"`.
- **Image** (`ingest::ingest_image`): a Haiku vision call (`describe_image`) produces a description → `chunk_text(desc, 500, 50)` → embed → replace then insert. `doc_type = "image"`. Requires an API key (the watcher skips images silently when no key is set).
- **Chunking** (`chunker::chunk_text`): the parameters `500`/`50` are *token* targets, not word counts. The implementation converts them to **characters at ~4 chars/token** (target ≈ 2000 chars, overlap ≈ 200 chars), backing up to a sentence boundary (`.`/`!`/`?`/newline) when possible. Note that ≈2000 chars is roughly **300–400 words**, materially fewer than 500 *words*: the spec's / CLAUDE.md's "≈500-word chunks" framing is loose and overstates the chunk size. The on-disk truth is the char-based heuristic described here.
- **Auto-ingest** (`ingest::watcher`): files dropped into the library dir are ingested via `notify` with an 800 ms debounce.

See [Ingestion pipeline](./ingestion.md) for full detail.

---

## 5. On-disk layout under `%USERPROFILE%\CogniTrade`

`Paths` (`src-tauri/src/paths.rs`) defines the tree; `Paths::ensure_all` creates `library`, `screenshots`, `data`, and `logs` at startup (called from `lib.rs::run`).

```
%USERPROFILE%\CogniTrade\
├── library\        # user-provided source documents (PDFs, images) — watched & ingested
├── screenshots\    # chart screenshots saved by the app (referenced by messages.image_path)
├── data\
│   ├── chat.db     # SQLite: messages, predictions, cost_daily, settings  (Paths::chat_db)
│   ├── chat.db-wal # WAL sidecar (present while WAL mode is active)
│   ├── chat.db-shm # shared-memory sidecar
│   ├── vec.db      # SQLite vector store: doc_chunks                        (Paths::lancedb)
│   ├── vec.db-wal
│   └── vec.db-shm
└── logs\           # application logs (tracing)
```

| Directory | `Paths` accessor | Purpose |
|-----------|------------------|---------|
| `library\` | `Paths::library()` | Drop zone for trading PDFs/research images. The `notify` watcher auto-ingests new/changed files into `vec.db`. Override via `settings.library_path`. |
| `screenshots\` | `Paths::screenshots()` | Chart images saved during analysis (the `save_screenshot` IPC). Absolute paths land in `messages.image_path`. |
| `data\` | `Paths::data()` | Both SQLite DBs plus their WAL/SHM sidecars. `Paths::chat_db()` → `data\chat.db`; `Paths::lancedb()` → `data\vec.db` (note the misleading name). |
| `logs\` | `Paths::logs()` | `tracing`/`tracing-subscriber` output. |

> **Never commit this tree.** It contains user documents, screenshots, chat history, and prediction data. WAL/SHM sidecars are managed by SQLite and may or may not be present at rest depending on checkpoint state.

---

## 6. Data lifecycle & retention

Two background loops are spawned by `prune::spawn_background` (started from `lib.rs::run`):

### 6.1 Chat message retention — 7-day prune

- **What:** `chat_store::prune_older_than_secs(db, 7 * 86400)` runs `DELETE FROM messages WHERE ts < cutoff`, where `cutoff = now - 7 days`.
- **Cadence:** every 6 hours (`Duration::from_secs(6 * 3600)`), with one immediate run on startup before the first sleep.
- **Effect:** chat history older than 7 days is permanently deleted. Because of the `messages.prediction_id → predictions(id)` FK with `NO ACTION`, a `messages` row that still references a `predictions` row is fine to delete (the child holds the reference, not the parent); `predictions` rows are **not** pruned and persist as the long-term calibration record.

### 6.2 Prediction resolution — states `pending` → `resolved` / `failed`

- **What:** every 60 seconds, `predictions::pending_due` selects rows where `outcome_status='pending' AND (ts + horizon_seconds) <= now`; each is passed to `predictions::resolve_one`.
- **Resolution:** `resolve_one` polls the current CoinGecko USD price for the symbol:
  - **Success** → `outcome_status='resolved'`, `outcome_price_at_horizon = price`, `outcome_resolved_at = now`.
  - **Failure** (e.g. CoinGecko error / unlisted symbol) → `outcome_status='failed'`, `outcome_resolved_at = now` (no horizon price recorded).
- **Entry price:** `outcome_price_at_ts` is filled separately at prediction time by `record_initial_price` (fire-and-forget from `ipc::analyze`); it may be NULL if that lookup failed.

State machine:

```
                 (insert_from_analysis)
                          │
                          ▼
                      [pending] ──────────────┐
                          │                    │ horizon elapses,
   horizon elapses,       │                    │ CoinGecko price
   CoinGecko OK           ▼                    ▼ fetch errors
                     [resolved]            [failed]
```

| `outcome_status` | Set by | Terminal? | `outcome_price_at_horizon` | `outcome_resolved_at` |
|------------------|--------|-----------|----------------------------|------------------------|
| `pending` | insert default | no | NULL | NULL |
| `resolved` | `resolve_one` (price OK) | yes | price at resolution | set |
| `failed` | `resolve_one` (price error) | yes | NULL | set |

> Resolution measures **price at the horizon vs. price at prediction**. It does **not** evaluate whether `stop_loss_pct` / `take_profit_pct` were breached intra-horizon, and there is no trade simulation or execution. Intra-horizon SL/TP outcome tracking is **Planned — not yet implemented**.

### 6.3 Cost rows

`cost_daily` rows accumulate per UTC day and are **not** pruned by any loop. Each new day creates a new keyed row; old rows persist as a spend ledger.

### 6.4 `doc_chunks` lifecycle

- **Replace-on-reingest:** re-ingesting the same `doc_path` runs `delete_by_doc_path(doc_path)` then re-inserts, so a document's chunks are fully refreshed (no stale duplicates).
- **No automatic expiry:** chunks persist until the document is re-ingested (replacing them) or `vec.db` is deleted/rebuilt. There is no delete-on-file-removal hook in the watcher; removing a file from `library\` does not automatically remove its chunks.

---

## 7. Entity-relationship sketch

```
┌─────────────────────────── chat.db (SQLite, WAL) ────────────────────────────┐
│                                                                               │
│   messages                              predictions                           │
│   ─────────                             ───────────                           │
│   id            PK ◄──┐                 id            PK ◄────────┐            │
│   ts                  │                 ts                        │            │
│   role  CHECK(...)    │                 symbol                    │            │
│   content             │                 horizon_seconds           │            │
│   image_path ─────────┼─► file in       predicted_prob_up         │            │
│   prediction_id  FK ──┘   screenshots\  recommended_action        │            │
│        │                                stop_loss_pct  (advisory) │            │
│        └────────────────────────────────► (references) ──────────┘            │
│                                         take_profit_pct (advisory)            │
│                                         citations_json                        │
│                                         outcome_status  pending|resolved|failed│
│                                         outcome_price_at_ts                    │
│                                         outcome_price_at_horizon               │
│                                         outcome_resolved_at                    │
│                                                                               │
│   cost_daily                            settings                              │
│   ──────────                            ────────                              │
│   date  PK (YYYY-MM-DD)                 key   PK                              │
│   input_tokens                          value NOT NULL                        │
│   cached_input_tokens                   (daily_cost_cap_usd, model_main,       │
│   output_tokens                          model_extract, library_path, hotkey) │
│   cost_usd                                                                    │
└───────────────────────────────────────────────────────────────────────────────┘
   (no SQL relationship between chat.db and vec.db — separate files, never joined)

┌──────────────────────────── vec.db (SQLite, WAL) ────────────────────────────┐
│   doc_chunks                                                                   │
│   ──────────                                                                   │
│   id          PK (uuid v4)                                                     │
│   doc_path    ──► source file in library\   (idx_doc_chunks_path)             │
│   doc_type    "pdf" | "image"                                                  │
│   chunk_index                                                                  │
│   text        (retrievable passage)                                            │
│   page        (currently NULL)                                                 │
│   embedding   BLOB: 384 × little-endian f32  (1536 bytes)                       │
└───────────────────────────────────────────────────────────────────────────────┘

┌──────────── OS keyring (Windows Credential Manager) ─────────────┐
│   service = "cognitrade",  user = "anthropic_api_key"  →  <secret> │
│   (never stored in any SQLite DB or file)                          │
└───────────────────────────────────────────────────────────────────┘
```

**Relationship summary**

- `messages.prediction_id` → `predictions.id` (nullable FK, enforced, `NO ACTION`). The only enforced relationship in the data model.
- `messages.image_path` and `doc_chunks.doc_path` are **soft references** to files on disk (no FK; just string paths).
- `cost_daily`, `settings`, and `doc_chunks` have no foreign keys; they are independent stores.
- `chat.db` and `vec.db` are physically separate SQLite files and are **never** joined.
- The Anthropic API key has no database representation at all.
