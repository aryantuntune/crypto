# Coding Conventions & Patterns

This document captures the idioms a contributor must follow to match the **CogniTrade** codebase. It describes what the code *actually does today* — not the aspirational architecture in the spec. Where a capability exists only in the spec, it is flagged **Planned — not yet implemented**.

CogniTrade is a Tauri 2 desktop app: a Rust backend (`src-tauri/`) plus a SvelteKit / TypeScript frontend (`src/`, Svelte 5 + Tailwind 3 + Vite 6, static SPA adapter). It is an **advisory** crypto-chart analyst — it streams analysis from the Anthropic API and records self-calibrating predictions. It does **not** place orders, integrate with any exchange, or enforce stop-loss/take-profit. Keep contributions inside that scope.

Related docs:

- Project overview and hard constraints: [`../../CLAUDE.md`](../../CLAUDE.md)
- Tech stack: [`../architecture/tech-stack.md`](../architecture/tech-stack.md)
- Build & release (Windows): [`./build-and-release.md`](./build-and-release.md)
- Vision and scope: [`../project/vision-and-scope.md`](../project/vision-and-scope.md)
- Design plans (aspirational, diverges from current code): [`../superpowers/`](../superpowers/)

> **Platform note.** Windows is the **primary** target. Examples use PowerShell / `cmd` paths (`%USERPROFILE%\CogniTrade\...`) and the MSVC toolchain. `tauri.conf.json` uses `bundle.targets: "all"` (not a Windows-pinned config), so the installer the build produces follows the host OS — on Windows that means MSI (WiX) + NSIS. The app also builds on Linux/macOS for development, but ship and test against Windows.

---

## 1. Error handling

### 1.1 One error type, one alias

There is a single backend error enum, [`AppError`](../../src-tauri/src/error.rs), and a project-wide `Result<T>` alias. **Use them everywhere** in the backend — do not introduce per-module error types or return `Box<dyn Error>`.

```rust
// src-tauri/src/error.rs
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

pub type Result<T> = std::result::Result<T, AppError>;
```

Import the pair at the top of every module that can fail:

```rust
use crate::error::{AppError, Result};
```

### 1.2 Add a variant rather than stringly-typing

`AppError` derives `thiserror::Error` and has `#[from]` conversions for the foreign error types we propagate (`std::io`, `rusqlite`, `reqwest`, `serde_json`, `keyring`). Because of `#[from]`, the idiomatic call site is just `?`:

```rust
let conn = Connection::open(path)?;       // rusqlite::Error -> AppError::Sqlite
let bytes = std::fs::read(p)?;            // io::Error       -> AppError::Io
let parsed: R = resp.json().await?;       // reqwest::Error  -> AppError::Http
```

When you need a *new, semantically distinct* failure that callers (or the frontend) might branch on, **add a variant** — do not overload `Internal(String)` or smuggle meaning into a formatted string. `CostCapReached` and `NoApiKey` are the model to follow: they carry no payload and exist precisely so a caller can match on them. For example, [`analyze_with_cap`](../../src-tauri/src/llm/client.rs) returns `AppError::CostCapReached` *before* the network call, and the frontend can recognize that condition.

Reserve the string-carrying variants for their intended roles:

| Variant | Use for |
|---|---|
| `Invalid(String)` | Bad caller input (empty API key, unsupported file extension, embedding-dim mismatch, un-decodable base64). |
| `NotFound(String)` | A requested resource that does not exist. |
| `Anthropic(String)` | Non-success HTTP from the Anthropic API; include status + body. |
| `Internal(String)` | Genuinely unexpected failures from a dependency without its own variant (e.g. `fastembed` init/embed errors). |

Convert with `.map_err(...)` only when there is no `#[from]`:

```rust
let bytes = STANDARD.decode(base64_png.as_bytes())
    .map_err(|e| AppError::Invalid(format!("base64: {}", e)))?;
```

### 1.3 IPC returns `AppError` directly

`AppError` implements `Serialize` by serializing its `Display` string. Every `#[tauri::command]` returns `Result<T>` (i.e. `Result<T, AppError>`); on the error path Tauri serializes the message and rejects the JS promise. So commands **never** stringify errors themselves — just propagate with `?`:

```rust
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    settings::load(&state.db)        // AppError flows straight to the frontend
}
```

On the frontend the rejection surfaces as a thrown value; see [`lib/stores/chat.ts`](../../src/lib/stores/chat.ts), which wraps `api.analyze(...)` in `try/catch` and renders `String(e)`.

---

## 2. Module layout

### 2.1 One concern per module

Each backend file owns a single concern and re-exports through [`src-tauri/src/lib.rs`](../../src-tauri/src/lib.rs), which is the crate root and declares every module:

```rust
pub mod error;     pub mod paths;       pub mod db;
pub mod chat_store; pub mod settings;   pub mod cost;
pub mod llm;        pub mod ingest;     pub mod lance;
pub mod retrieval;  pub mod image_util; pub mod coingecko;
pub mod predictions; pub mod prune;     pub mod tray;
pub mod ipc;
```

Representative responsibilities (this table is a **subset** — the authoritative full list is the `mod` block above):

| Module | Concern |
|---|---|
| `error` | The `AppError` enum + `Result<T>` alias (Section 1). |
| `paths` | `Paths` — resolves `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}`. |
| `db` | Opens `data/chat.db`, runs the migration, exposes `type Db = Arc<Mutex<Connection>>`. |
| `chat_store` | Message persistence: insert/list `messages` rows in `chat.db`. |
| `lance` | Vector store over `data/vec.db` (see the **naming caveat** in Section 4). |
| `retrieval` | RAG entry point used by `ipc::analyze`: embed the query, brute-force cosine top-k over `doc_chunks`, build the retrieved-knowledge block. |
| `ingest` | Directory module: PDF/image ingestion (`ingest_pdf`, `ingest_image`), chunker, embeddings, and the file `watcher`. |
| `llm` | Directory module: Anthropic client, SSE streaming, prompt building, JSON-tail parsing. |
| `settings` | `Settings` struct, load/save against the `settings` table, **and** keyring API-key access. |
| `cost` | Per-day token/cost accounting + cap check. |
| `coingecko` | ~12-coin symbol→id table and the free `simple/price` lookup. |
| `predictions` | Self-calibration: insert a prediction per analysis, resolve at horizon. |
| `prune` | Background loops (chat prune + prediction resolution). |
| `tray` | System tray + panel toggle. |
| `ipc` | All `#[tauri::command]`s and `AppState`. |

A module that has internal sub-parts becomes a directory with a `mod.rs` that re-exports them — see Section 2.3.

### 2.2 Inline `#[cfg(test)]` tests

Unit tests live in the *same file* as the code they exercise, in a `mod tests` block, not in a separate `tests/` tree (that tree is reserved for `#[ignore]`d integration tests — Section 7). Follow the established shape:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;            // pull in sibling helpers as needed

    #[test]
    fn defaults_then_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("chat.db")).unwrap();
        // ...assertions
    }
}
```

Conventions visible across `cost.rs`, `settings.rs`, `db.rs`, `llm/parse.rs`, `llm/prompt.rs`, `llm/streaming.rs`:

- Use `tempfile::tempdir()` for anything that needs a real SQLite file — never touch the user's real `%USERPROFILE%\CogniTrade` directory from a test.
- Pure functions (`cost_usd`, `rates_for`, `extract_json_tail`, `parse_event`, `build_messages`) get small, deterministic table-style tests.
- `pretty_assertions` and `wiremock` are available as dev-dependencies; reach for `wiremock` instead of hitting `api.anthropic.com`.

### 2.3 `mod.rs` re-exports

A multi-file module exposes a clean surface from its `mod.rs`. [`llm/mod.rs`](../../src-tauri/src/llm/mod.rs) is purely declarative:

```rust
pub mod types;  pub mod parse;  pub mod prompt;  pub mod streaming;  pub mod client;
```

[`ingest/mod.rs`](../../src-tauri/src/ingest/mod.rs) does more — it declares submodules *and* hosts the cross-cutting entry points (`ingest_pdf`, `ingest_image`, `file_hash`, `IngestReport`) that orchestrate the submodules. Put orchestration that spans several submodules in `mod.rs`; keep single-responsibility logic (chunker, pdf, image, embeddings, watcher) in its own file. Callers then import the high-level function, e.g. `use crate::ingest::{ingest_pdf, ingest_image};`.

---

## 3. Async patterns

The backend is `tokio` (full feature set). Tauri commands are `async fn`. Follow these established patterns.

### 3.1 Streaming via an `mpsc` channel + Tauri events

Streaming text from the model to the UI uses a bounded `tokio::sync::mpsc` channel, *not* shared mutable buffers. The producer (the SSE loop in [`LlmClient::analyze`](../../src-tauri/src/llm/client.rs)) sends `String` deltas; a spawned consumer task forwards each one to the frontend as a Tauri event. See [`ipc::analyze`](../../src-tauri/src/ipc.rs):

```rust
let (tx, mut rx) = mpsc::channel::<String>(64);

let app2 = app.clone();
let stream_task = tokio::spawn(async move {
    while let Some(t) = rx.recv().await {
        let _ = app2.emit("analysis_chunk", &t);   // forward delta to UI
    }
});

let out = analyze_with_cap(&state.db, &state.llm, &model, inputs, tx).await?;
let _ = stream_task.await;                          // drain before continuing
// ... later, once parsing/persistence is done:
let _ = app.emit("analysis_done", serde_json::json!({ "message_id": asst_id, "analysis": out.analysis }));
```

Conventions:

- **Two events, fixed names:** `analysis_chunk` (each text delta) and `analysis_done` (final payload with `message_id` + parsed `analysis`). The frontend listens for exactly these strings ([`lib/stores/chat.ts`](../../src/lib/stores/chat.ts)) — if you add a stream, add a matching listener.
- Channel capacity is `64`; emits are best-effort (`let _ = ...emit(...)`) so a slow/closed UI never aborts the analysis.
- `await` the spawned forwarder (`stream_task.await`) before emitting `analysis_done`, so chunks can't arrive after the "done" signal.

### 3.2 `Arc<Mutex<...>>` for shared state

The SQLite connection is shared as `type Db = Arc<Mutex<Connection>>` ([`db.rs`](../../src-tauri/src/db.rs)). This is a **`std::sync::Mutex`**, held only for the duration of a synchronous query — never `.await` while holding it. The idiom is `let conn = db.lock().unwrap(); /* query */`. The `lance` store wraps its connection the same way (`Arc<Mutex<Connection>>`).

Application-wide state lives in `AppState`, registered with `app.manage(state)` and injected via `State<'_, AppState>`:

```rust
pub struct AppState {
    pub db: Db,                                   // Arc<Mutex<Connection>> — cheap to clone
    pub lance: Arc<Mutex<Option<LanceStore>>>,    // tokio::sync::Mutex; lazily opened (3.3)
    pub paths: Paths,
    pub llm: LlmClient,
}
```

Note the two flavours of `Mutex` in play, and keep them straight:

- **`std::sync::Mutex`** guards the in-place SQLite connections: it lives inside `Db` (the `Arc<Mutex<Connection>>` alias) and inside `LanceStore` (a private `inner: Arc<Mutex<Connection>>` field — not part of any public surface). Lock briefly, do the synchronous DB work, drop.
- **`tokio::sync::Mutex`** guards `AppState.lance` because access spans `.await` points (it may open the store, which is `async`).

### 3.3 Lazy `LanceStore` open

The vector store is opened **lazily on first use**, not at startup, to keep boot cheap and tolerate a missing DB. `AppState.lance` starts as `None`; [`lance_get_or_open`](../../src-tauri/src/ipc.rs) opens it once and caches it:

```rust
async fn lance_get_or_open(state: &AppState) -> Result<LanceStore> {
    let g = state.lance.lock().await;
    if let Some(s) = g.as_ref() { return Ok(s.clone()); }
    drop(g);                                              // release before the async open
    let store = lance::open(&state.paths.lancedb()).await?;
    let mut g = state.lance.lock().await;
    *g = Some(store.clone());
    Ok(store)
}
```

Any **IPC command** needing the store calls `lance_get_or_open(&state).await?` rather than reaching into the field directly. `LanceStore` is `Clone` (it just clones the inner `Arc`), so handing out clones is cheap.

> **Two independent handles onto the same `data/vec.db`.** `lance_get_or_open` governs only the IPC-command path. The background **file watcher does not share** `AppState.lance`: `lib.rs::run()` opens a *second, independent* `LanceStore` **eagerly** during `setup` (`lance::open(&store_path).await` in the spawned task) and hands that store to [`ingest::watcher::run_watcher`](../../src-tauri/src/ingest/watcher.rs). So there are two separate connections onto the same `vec.db` — the lazily-opened `AppState.lance` (IPC path) and the watcher's boot-time store. Both go through `LanceStore`/SQLite WAL, so concurrent writes are safe, but don't assume the watcher reuses the `AppState` handle.

### 3.4 The embedding model is a lazy global singleton

`fastembed` BGE-small is expensive to load and CPU-only, so it lives in a process-global `OnceLock<Mutex<TextEmbedding>>` that downloads the model on first use ([`ingest/embeddings.rs`](../../src-tauri/src/ingest/embeddings.rs)). `embed()` is **synchronous and serialized behind the mutex** — only one embedding batch runs at a time, which is deliberate given the 8 GB RAM ceiling. Do not spawn parallel embedding calls; batch your texts into a single `embed(Vec<String>)` instead. The embedding dimension is fixed at `EMBED_DIM = 384` and validated on insert.

### 3.5 Fire-and-forget background work

Spawn detached tasks with `tokio::spawn` for work whose result the caller doesn't need to await — e.g. recording the initial CoinGecko price after an analysis (`ipc::analyze`), or the long-running loops started by [`prune::spawn_background`](../../src-tauri/src/prune.rs) (chat prune every 6h, prediction resolution every 60s). Clone the cheap `Arc` handles (`db.clone()`, `app.clone()`) into the task; never move `State<'_, _>` across a spawn.

---

## 4. Naming caveat: "Lance" is historical — there is no LanceDB

**Do not assume LanceDB.** The names `lance.rs`, `LanceStore`, `lance::open`, and `paths::lancedb()` are **misnomers left over from an earlier design**. The actual implementation in [`lance.rs`](../../src-tauri/src/lance.rs) is **plain SQLite** (`data/vec.db`):

- Embeddings are stored as **little-endian `f32` BLOBs** in a `doc_chunks` table (`embedding_to_bytes` / `bytes_to_embedding`).
- `search()` is **brute-force cosine in Rust**: it `SELECT`s *every* row, computes `cosine()` against the query vector, sorts, and truncates to `k`. There is no ANN index, no vector extension, no LanceDB crate.
- The store is a separate SQLite database from `chat.db`. Two DBs, both WAL mode:

| File | Opened by | Tables |
|---|---|---|
| `data/chat.db` | `db::open` | `messages`, `predictions`, `cost_daily`, `settings` (schema in [`migrations/001_initial.sql`](../../src-tauri/migrations/001_initial.sql)) |
| `data/vec.db` | `lance::open` | `doc_chunks` |

When you touch this area: keep the existing names for consistency with the current code, but never reach for LanceDB APIs or document this as a vector database. If the brute-force scan is ever replaced with a real index, rename the module in the same change so the name stops lying. (A proper vector index is **Planned — not yet implemented**.)

---

## 5. Frontend conventions

The frontend is Svelte 5 + TypeScript under [`src/`](../../src/), built with SvelteKit's static adapter (SPA). The shell is a single page with three tabs — Chat / Library / Settings — in [`routes/+page.svelte`](../../src/routes/+page.svelte).

### 5.1 Typed IPC wrappers — never call `invoke` from a component

All backend calls go through the typed wrapper object `api` in [`lib/ipc.ts`](../../src/lib/ipc.ts). Components and stores import `api`; they do **not** import `invoke` directly. This keeps command names and argument shapes in one place and typed against [`lib/types.ts`](../../src/lib/types.ts).

```ts
import { invoke } from '@tauri-apps/api/core';
import type { Message, Settings, DailyCost, AnalysisJson } from './types';

export const api = {
  listRecentMessages: (limit = 50) => invoke<Message[]>('list_recent_messages', { limit }),
  getSettings: () => invoke<Settings>('get_settings'),
  saveSettings: (value: Settings) => invoke<void>('save_settings', { value }),
  analyze: (req: { user_text: string; screenshot_path?: string; symbol_hint?: string }) =>
    invoke<{ message_id: number }>('analyze', { req }),
  // ...
};
```

When you add a `#[tauri::command]`, you must in the same change: (a) register it in the `tauri::generate_handler![]` list in [`lib.rs`](../../src-tauri/src/lib.rs), (b) add a typed method to `api`, and (c) add/extend the relevant interface in `types.ts`.

### 5.2 `camelCase` invoke args; argument names must match the Rust parameters

Tauri converts JS `camelCase` invoke keys to Rust `snake_case` parameter names. The **invoke argument object keys are the Rust parameter names** (with case conversion). Two rules fall out of this:

- Scalar params map by name: `invoke('list_recent_messages', { limit })` → `limit: i64`; `invoke('save_settings', { value })` → `value: Settings`.
- A multi-field camelCase key maps to the snake_case Rust param: `saveScreenshot(base64Png, filenameHint)` calls `invoke('save_screenshot', { base64Png, filenameHint })`, which binds the Rust `base64_png` and `filename_hint` parameters.
- **Struct payloads are nested under the param name, not spread.** `analyze` takes a single Rust parameter `req: AnalyzeReq`, so the wrapper sends `{ req: { user_text, screenshot_path, symbol_hint } }` — note that the *inner* struct fields stay `snake_case` because they are deserialized by `serde`, while the *outer* key `req` is the parameter name. Match this exactly.

### 5.3 Stores and event listeners

Reactive app state lives in Svelte stores under [`lib/stores/`](../../src/lib/stores/). The chat store ([`chat.ts`](../../src/lib/stores/chat.ts)) is the template:

- Exposes `writable` stores (`messages`, `streaming`, `analysisResult`, `isAnalyzing`).
- Registers Tauri event listeners **once** (guard with a module-level `unlisten` handle so `initEvents()` is idempotent) for `analysis_chunk` (append delta to `streaming`) and `analysis_done` (set `analysisResult`, clear `streaming`, refresh messages). These event names must stay in sync with the backend emits in `ipc::analyze` (Section 3.1).
- Wraps the `api` call in `try/catch` and renders failures as text rather than letting the promise rejection escape.

Keep components thin: they read stores and call store actions / `api`, they don't own IPC plumbing.

### 5.4 Styling

Tailwind 3 utility classes inline in markup, dark zinc palette (see `+page.svelte`). No separate CSS modules.

---

## 6. Secrets and configuration

- **The Anthropic API key never touches the database or logs.** It is stored in the OS keyring via the `keyring` crate — service `"cognitrade"`, user `"anthropic_api_key"` (on Windows this is the Credential Manager). All access goes through [`settings.rs`](../../src-tauri/src/settings.rs): `get_api_key`, `set_api_key`, `clear_api_key`, and `require_api_key()` (which returns `AppError::NoApiKey` when unset). Never add the key to a `tracing` line, a serialized struct, or the `settings` table.
- **Non-secret config lives in the `settings` table** of `chat.db`, modeled by the `Settings` struct: `daily_cost_cap_usd` (default `$2`), `model_main` (default `claude-sonnet-4-6`), `model_extract` (default `claude-haiku-4-5-20251001`), `library_path`, `hotkey`. Persist via `settings::save`; read via `settings::load`. Add new non-secret options as fields here, with a default in `impl Default for Settings`.
- **Cost guardrail.** Every primary model call goes through [`analyze_with_cap`](../../src-tauri/src/llm/client.rs), which checks the day's accumulated spend (`cost_daily` table) against `daily_cost_cap_usd` **before** issuing the request and returns `AppError::CostCapReached` if over. Do not add a code path that calls the model and bypasses this check.
- **Logging.** Use `tracing` (`tracing::warn!`, etc.), as in `lib.rs`. Never log secrets, full API request bodies, or raw image bytes.

> **Scope guardrail.** The system prompt states *"You are an advisor. The user places all trades themselves."* There is **no** order execution, **no** Binance/Kraken integration, **no** Minimax engine, and **no** automated SL/TP enforcement in the code — `stop_loss_pct`/`take_profit_pct` are advisory fields in the model's JSON output and stored on the prediction row; nothing acts on them. All of those are **Planned — not yet implemented** (or out of scope). Do not present them as existing, and do not add silent trade-execution paths — pre-trade human approval is a hard constraint (see [`../../CLAUDE.md`](../../CLAUDE.md)).

---

## 7. Formatting, lint, tests, and commit hygiene

### 7.1 Rust

- **Format with `rustfmt`** before every change: `cd src-tauri && cargo fmt`. (There is no `rustfmt.toml`, so this uses default formatting. The codebase keeps a few compact single-line match arms in enum/error definitions; let `rustfmt` settle the rest and don't fight it elsewhere.)
- **Lint** with `cargo clippy` and keep it clean. *(Recommended — there is no `clippy.toml`, no CI, and no git in the repo yet, so this is a process recommendation, not an enforced gate.)*
- **Run unit tests:** `cd src-tauri && cargo test`. Single module: `cargo test settings::tests`.
- **Integration tests are `#[ignore]`d** and live in `src-tauri/tests/{ingest_integration,llm_integration}.rs`. Run them explicitly:

  ```powershell
  cd src-tauri
  cargo test -- --ignored --nocapture
  # full ingest smoke test (downloads BGE-small on first run):
  cargo test --test ingest_integration -- --ignored --nocapture
  ```

  Use `wiremock` to stub Anthropic rather than calling the live API in tests.

### 7.2 Frontend

- **Type-check** with `npm run check` (runs `svelte-kit sync && svelte-check --tsconfig ./tsconfig.json`). It must pass clean.
- Run the app in dev with `npm run tauri dev`; build installers with `npm run tauri build` (see [`./build-and-release.md`](./build-and-release.md)). `tauri.conf.json` sets `bundle.targets` to `"all"`, so the installer format is determined by the build host, not pinned to Windows: building **on Windows** produces the MSI (WiX) and NSIS installers.
- Keep IPC types in `lib/types.ts` aligned with the Rust structs they mirror (`Settings`, `Message`, `DailyCost`, `AnalysisJson`); a mismatch is a silent runtime bug, not a compile error.

### 7.3 Commit hygiene

**The repository is not git-initialized yet** — there is no `.git`, no history, no branches. Until someone runs `git init`:

- Do not assume a VCS exists; do not write tooling, hooks, or docs that depend on git state.
- If a task calls for committing, first offer to `git init`, then create a branch off the default branch before committing (per [`../../CLAUDE.md`](../../CLAUDE.md)).
- **Never commit user data.** The runtime directories under `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` (PDFs, screenshots, both SQLite DBs, logs) must stay out of version control. When git is introduced, they belong in `.gitignore` from the first commit.
- **Pre-commit gate — Planned, not yet implemented.** There is no CI, no pre-commit hook, and no git history, so nothing currently enforces these checks. Once a toolchain for hooks/CI is chosen, the *recommended* gate is: `cargo fmt --check`, `cargo clippy`, `cargo test`, and `npm run check`.
