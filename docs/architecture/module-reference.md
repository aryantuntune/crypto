# Backend Module Reference

Per-module reference for the CogniTrade Rust backend (`src-tauri/src/`).

CogniTrade is a [Tauri 2](https://tauri.app) desktop app: a Rust backend plus a
SvelteKit/TypeScript frontend. It is an **advisory crypto-chart analyst**, not an
autonomous trader. There is no exchange integration, no order execution, and no
automated stop-loss/take-profit enforcement. The backend's job is: take a user
message (optionally with a chart screenshot), retrieve relevant passages from
ingested documents, ask Claude to analyze the chart, stream the answer back to the
UI, and log a structured *prediction* so the system can later self-calibrate its
probability estimates against real price moves.

> **Scope note.** This document describes only what the code in `src-tauri/src/`
> actually does as of this writing. Several capabilities named in the original
> spec (`crypto Dhriti.txt`) and design docs under
> [`../superpowers/`](../superpowers/) — exchange/order execution, a Minimax
> decision engine, hard SL/TP enforcement — are **Planned — not yet implemented**
> and are called out as such where relevant.

Related docs:
[design spec](../superpowers/specs/2026-05-05-cognitrade-design.md) ·
[implementation plan](../superpowers/plans/2026-05-05-cognitrade-v1.md).

---

## Module dependency overview

The backend is a flat set of modules declared in `lib.rs`, with the `llm/` and
`ingest/` directories each forming a small submodule tree. The dependency
direction is roughly: **`ipc` orchestrates everything**, the `llm/*` and
`ingest/*` trees provide capability, and the storage/util modules (`db`,
`lance`, `cost`, `settings`, `paths`, `error`, …) sit underneath.

```
lib::run (app bootstrap)
 ├── tray
 ├── paths ──────────────► (filesystem layout)
 ├── db ───────────────────► chat.db  (chat_store, settings, cost, predictions)
 ├── prune ──► chat_store, predictions ──► coingecko   (two spawned loops)
 ├── (spawned task) lance::open(vec.db) ──► ingest::watcher (blocking loop)
 │        └── ingest::{ingest_pdf, ingest_image}
 └── ipc::AppState { db, lance, paths, llm }
        │
        ├── ipc::analyze ──► chat_store          (persist messages)
        │                 ├─ image_util          (resize screenshot)
        │                 ├─ settings            (api key, models, cap)
        │                 ├─ extract_symbol_…    (Haiku vision, inline in ipc.rs)
        │                 ├─ retrieval ──► ingest::embeddings + lance::search
        │                 ├─ llm::client::analyze_with_cap
        │                 │     ├─ cost           (cap check + usage accounting)
        │                 │     ├─ llm::prompt    (build_messages, SYSTEM_PROMPT)
        │                 │     ├─ llm::streaming (SSE parse)
        │                 │     └─ llm::parse ──► llm::types::AnalysisJson
        │                 ├─ predictions ──► coingecko (record initial price)
        │                 └─ (Tauri events) analysis_chunk / analysis_done
        │
        └── ipc::ingest_path ──► ingest::{ingest_pdf, ingest_image}
                                   ├─ ingest::pdf / ingest::image
                                   ├─ ingest::chunker
                                   ├─ ingest::embeddings (fastembed BGE-small)
                                   └─ lance (delete_by_doc_path + insert)

error::AppError / error::Result<T>  ← used by ~every module
```

Two SQLite databases, both in `%USERPROFILE%\CogniTrade\data\`:

| File       | Opened by      | Tables                                      |
|------------|----------------|---------------------------------------------|
| `chat.db`  | `db::open`     | `messages`, `predictions`, `cost_daily`, `settings` |
| `vec.db`   | `lance::open`  | `doc_chunks`                                |

Both use WAL journaling. The Anthropic API key is **never** stored in either DB —
it lives in the OS keyring (see [`settings`](#settings-settingsrs)).

> **Build note (Windows).** `reqwest` is declared with `default-features = false`
> and the `rustls-tls` feature, so the HTTPS stack is **rustls** — no `native-tls`
> / system OpenSSL is required on Windows. `anyhow` and `async-trait` are also
> declared in `Cargo.toml` but are largely unused by the modules documented here.

---

## `lib.rs` — application bootstrap

**Responsibility.** Declares the module tree (`pub mod …`) and defines
`run()`, the Tauri entry point invoked from `main.rs`.

**What `run()` does, in order:**

1. Builds a `tauri::Builder` with plugins `opener`, `global-shortcut`, `dialog`,
   and `fs`.
2. In `.setup(...)`:
   - `tray::init_tray(&app.handle())` installs the system tray.
   - `paths::Paths::from_env()` + `paths.ensure_all()` create
     `~/CogniTrade/{library,screenshots,data,logs}`.
   - `db::open(paths.chat_db())` opens `chat.db`.
   - Constructs `ipc::AppState { db, lance: Arc<Mutex<None>>, paths, llm }` and
     `app.manage(...)`s it. **The vector store is *not* opened here** — `lance`
     starts as `None` and is opened lazily on first use (see
     [`ipc`](#ipc-ipcrs)).
   - `prune::spawn_background(db)` starts the two background loops.
   - Spawns a **separate** `tauri::async_runtime::spawn` task that calls
     `lance::open(&store_path)` to open its **own** `LanceStore` for `vec.db`,
     then calls `ingest::watcher::run_watcher(store, library)` — a **synchronous
     blocking loop** (called, not `.await`ed in a way that yields). This watcher
     store is independent of the lazily-opened `AppState.lance` that `analyze`
     and `ingest_path` use.
   - Registers the `Ctrl+Shift+Space` global hotkey that calls
     `tray::toggle_panel`.
3. Registers all `#[tauri::command]` handlers via `generate_handler!` (see the
   table in [`ipc`](#ipc-ipcrs)) and runs the app.

**Invariants / gotchas.**

- Background tasks are spawned **only after** Tauri's async runtime is up
  (inside `.setup`), never at the top of `run()` — spawning before the runtime
  exists would panic. The inline comment flags this.
- Hotkey registration is **tolerant of "already registered"** errors: both
  `on_shortcut` handler registration and `register` only `tracing::warn!` on
  failure, so a stale registration from a previously crashed run won't prevent
  startup.
- `paths.ensure_all().expect(...)` and `db::open(...).expect(...)` will **panic**
  at startup if the data directory can't be created or the DB can't be opened —
  this is intentional (no usable app without them).

---

## `ipc.rs` — Tauri command surface & analyze orchestration

**Responsibility.** Defines `AppState` and every `#[tauri::command]` exposed to
the frontend. This is the orchestration layer; the headline command is
`analyze`.

### `AppState`

```rust
pub struct AppState {
    pub db: Db,                                   // Arc<Mutex<Connection>> over chat.db
    pub lance: Arc<Mutex<Option<LanceStore>>>,    // vec.db, opened lazily
    pub paths: Paths,
    pub llm: LlmClient,
}
```

`lance_get_or_open(state)` is the lazy accessor: returns the cached `LanceStore`
clone if present, otherwise drops the guard, calls
`lance::open(&state.paths.lancedb())`, re-locks, and caches it. This is a
double-checked / TOCTOU-tolerant pattern: under a race two opens can happen, but
only one `LanceStore` is kept in the cache.

### Commands (all registered in `lib.rs`)

| Command | Signature (args) | Returns | Notes |
|---|---|---|---|
| `list_recent_messages` | `limit: i64` | `Vec<Message>` | `chat_store::recent` |
| `get_settings` | — | `Settings` | non-secret settings from `chat.db` |
| `save_settings` | `value: Settings` | `()` | |
| `get_api_key_set` | — | `bool` | whether a keyring entry exists (never returns the key) |
| `set_api_key` | `value: String` | `()` | rejects empty/whitespace; writes to keyring |
| `clear_api_key` | — | `()` | deletes keyring entry |
| `cost_today` | — | `DailyCost` | today's accumulated cost |
| `save_screenshot` | `base64_png`, `filename_hint?` | `String` (path) | decodes base64 PNG, writes to `screenshots/` |
| `analyze` | `AnalyzeReq` | `AnalyzeAck { message_id }` | the main flow, below |
| `ingest_path` | `path: String` | `usize` (chunks) | dispatches on file extension |

### `analyze` — end-to-end flow

`AnalyzeReq { user_text, screenshot_path?, symbol_hint? }`. Steps:

1. **Persist the user message** (`chat_store::insert`) up front, so it survives
   even if analysis later fails.
2. **Resize the screenshot** in place if `image_util::needs_resize` is true
   (Claude's image limits).
3. Load the **last 10 messages** as chat history (excluding the just-inserted
   user message).
4. **Open the vector store** lazily and **base64-encode the image** if present.
5. **Resolve the symbol** (priority order):
   1. `symbol_hint` (uppercased) if non-empty;
   2. else, if there's an image, `extract_symbol_from_image` — a one-shot
      non-streaming **Haiku** vision call (`settings.model_extract`) that returns
      just the ticker; on any error it falls back to `"BTCUSDT"`;
   3. else `"BTCUSDT"`.
6. **RAG retrieval.** Build the query `"{symbol} chart pattern analysis"` and run
   `retrieval::search(store, q, 6)` → up to 6 chunk texts.
7. Build `PromptInputs { retrieved_chunks, history, user_text, image_b64 }`.
8. **Stream.** Create an `mpsc` channel; spawn a task that forwards each text
   chunk to the frontend via the Tauri event **`analysis_chunk`**. Call
   `analyze_with_cap(db, llm, model, inputs, tx)` (model = `settings.model_main`).
9. **Persist the prediction.** If the response contained a parseable JSON tail
   (`out.analysis`), `predictions::insert_from_analysis` writes a row, and a
   fire-and-forget task records the current CoinGecko price as
   `outcome_price_at_ts`.
10. **Persist the assistant message** (`out.full_text`, linked to
    `prediction_id`).
11. Emit the Tauri event **`analysis_done`** with `{ message_id, analysis }` and
    return `AnalyzeAck { message_id }`.

### `extract_symbol_from_image`

A private helper (defined in `ipc.rs`, not in `llm/`). Non-streaming POST to
`https://api.anthropic.com/v1/messages` with `max_tokens: 40`, image + a terse
prompt; takes the first whitespace token, uppercased; errors on empty/`UNKNOWN`.

### `ingest_path`

Dispatches on lowercased extension: `pdf` → `ingest_pdf` (no API key needed);
`png|jpg|jpeg|webp` → `ingest_image` (requires `settings::require_api_key`, uses
`model_extract`); anything else → `AppError::Invalid`.

**Invariants / gotchas.**

- **Advisory, not executing.** `analyze` produces text + a structured
  *suggestion*; nothing here places trades, sets exchange-side SL/TP, or touches
  Binance/Kraken. (Order execution and SL/TP enforcement are **Planned — not yet
  implemented**.)
- The recommendation is a **JSON tail of free text**, not Anthropic tool-use. If
  the model omits or malforms the fenced block, `out.analysis` is `None`, **no
  prediction row is written**, and the assistant message is still saved.
- Symbol fallback is silent: an unreadable chart quietly becomes `BTCUSDT`.
- Initial-price recording is best-effort; if the symbol isn't in CoinGecko's
  table (see [`coingecko`](#coingecko-coingeckors)) the price is simply never
  recorded.
- `analyze` returns *after* the stream completes (`stream_task.await`); the
  frontend renders incrementally from `analysis_chunk` events, not from the
  return value.

---

## `llm/` — Anthropic client, prompt, streaming, parsing

`llm/mod.rs` re-exports five submodules: `types`, `parse`, `prompt`,
`streaming`, `client`.

### `llm/types.rs`

**Responsibility.** The structured analysis schema the model is asked to emit.

- `Action` — `buy | sell | hold` (serde `lowercase`).
- `Horizon` — `1h | 4h | 1d | 3d | 1w`; `Horizon::seconds()` converts to seconds
  (used to compute when a prediction is "due").
- `Citation { doc, page? }`.
- `AnalysisJson { action, probability_up: f32, horizon, stop_loss_pct?,
  take_profit_pct?, key_signals, citations }`.
- `AnalysisJson::validate()` enforces `probability_up ∈ [0.0, 1.0]`.

**Gotcha.** `stop_loss_pct` / `take_profit_pct` are `Option` and only *recorded*
as advisory metadata on the prediction row. They are **not enforced** anywhere —
treat any expectation of automated SL/TP as **Planned — not yet implemented**.

### `llm/prompt.rs`

**Responsibility.** The system prompt and the user-turn builder.

- `SYSTEM_PROMPT` — instructs the model to read the chart, cite retrieved
  passages, output prose, then a fenced ```json``` block matching the
  `AnalysisJson` schema. It ends with: *"You are an advisor. The user places all
  trades themselves."*
- `build_messages(&PromptInputs) -> Vec<ChatMessage>` constructs the request
  messages: prior history first, then one new `user` turn whose content array is
  `[retrieved-knowledge text block (if any), image block (if any), user_text
  block]`.

**Invariants / gotchas.**

- **Prompt caching.** The retrieved-knowledge block carries
  `cache_control: {type: ephemeral}`, and the system prompt is *also* marked
  ephemeral (in `client.rs`). This is what drives the cached-input token
  accounting in [`cost`](#cost-costrs).
- If `retrieved_chunks` is empty, **no** knowledge block is added (verified by
  unit test `no_chunks_means_no_cached_block`).
- The image media type is hard-coded `image/png`; screenshots saved by
  `save_screenshot` are always PNG, so this holds for the chat path.

### `llm/streaming.rs`

**Responsibility.** Parse one Anthropic SSE event into a `StreamEvent`.

- `StreamEvent` = `Text(String)` | `Usage { input_tokens, cached_input_tokens,
  output_tokens }` | `Done`.
- `parse_event(event_name, data_json)` handles `content_block_delta`
  (text_delta → `Text`), `message_start` and `message_delta` (→ `Usage`),
  `message_stop` (→ `Done`); everything else → `None`.

**Gotcha — token accounting.** `cache_creation_input_tokens` is **summed into
`input_tokens`** (a deliberately conservative cost estimate, per the inline
comment); `cache_read_input_tokens` maps to `cached_input_tokens`. So "input"
here is *uncached + cache-creation*, billed at the full input rate.

### `llm/parse.rs`

**Responsibility.** `extract_json_tail(text) -> Option<AnalysisJson>`.

Finds the **last** ```` ```json ```` fence, parses the body as `AnalysisJson`,
and returns it only if `validate()` passes.

**Gotchas.**

- Picks the **last** fence when several exist (unit-tested), so a final
  corrected block wins.
- Returns `None` (not an error) when missing, unparseable, or invalid — the
  caller treats "no analysis" as a normal outcome.

### `llm/streaming.rs` ↔ `llm/client.rs` boundary

`client.rs` does the byte-stream → `\n\n`-delimited event-block splitting and
hands each block's `event:`/`data:` lines to `parse_event`; `streaming.rs` owns
only the per-event JSON decoding.

### `llm/client.rs`

**Responsibility.** The HTTP/streaming Anthropic client plus the cost-capped
wrapper.

- `LlmClient { base_url, http }` — `new()` targets
  `https://api.anthropic.com`; `with_base(...)` is used by integration tests
  (e.g. wiremock).
- `LlmClient::analyze(api_key, AnalyzeArgs, tx)`:
  - Builds the request (`stream: true`, `max_tokens` from args, system prompt
    with ephemeral cache_control, messages from `build_messages`).
  - POSTs with headers `x-api-key`, `anthropic-version: 2023-06-01`, and an
    explicit `content-type: application/json`. (The inline
    `extract_symbol_from_image` and `ingest::image::describe_image` helpers use
    reqwest's `.json()`, which sets the content type implicitly, so they don't
    set it explicitly.)
  - Reads `bytes_stream()`, splits on `\n\n`, forwards text deltas to `tx`,
    accumulates `full` text + token usage.
  - Returns `AnalyzeOutput { full_text, analysis (extract_json_tail), input/
    cached/output tokens }`.
- `analyze_with_cap(db, client, model, inputs, tx)`:
  - Loads settings + today's cost; if `cost::is_over_cap(today, cap)` returns
    `AppError::CostCapReached` **before** any network call.
  - `settings::require_api_key()`; runs `analyze` with `max_tokens: 1500`;
    `cost::add_usage(...)` after success.

**Invariants / gotchas.**

- **Cap is checked before the call, accounted after.** A single call that
  overshoots the cap still completes and is billed; the cap blocks the *next*
  call. (`is_over_cap` uses `>=`.)
- Non-2xx responses surface as `AppError::Anthropic("HTTP {status}: {body}")`.
- `max_tokens` is **1500** for the main analysis (hard-coded in
  `analyze_with_cap`), distinct from the 40/600 used by the inline extract and
  image-describe helpers.

---

## `ingest/` — document ingestion pipeline

`ingest/mod.rs` re-exports `chunker`, `pdf`, `image`, `embeddings`, `watcher`,
and defines the two top-level ingest functions plus `file_hash`.

### `ingest/mod.rs`

- `file_hash(path)` — SHA-256 hex of file bytes (helper; available for
  dedup/change-detection).
- `IngestReport { doc_path, chunks }`.
- `ingest_pdf(store, path)`: `pdf::extract` → `chunk_text(text, 500, 50)` →
  error if no chunks → `embeddings::embed` → build `DocRow`s
  (`doc_type = "pdf"`) → `store.delete_by_doc_path(path)` → `store.insert`.
- `ingest_image(store, api_key, base_url, model, path)`: `image::describe_image`
  → same chunk/embed/replace path, `doc_type = "image"`.

**Invariant — re-ingest is idempotent-by-path.** Both functions
`delete_by_doc_path` before inserting, so re-ingesting the same file replaces its
chunks rather than duplicating them.

### `ingest/pdf.rs`

`extract(path) -> PdfText { full_text }` via `pdf_extract::extract_text_from_mem`.
**Gotcha.** Text-only extraction; scanned/image PDFs yield little or no text, and
`ingest_pdf` then errors with `"no text in …"`. No OCR.

### `ingest/image.rs`

`describe_image(api_key, base_url, model, path)` — sends the image to Claude
(Haiku) with a prompt asking for a single dense descriptive paragraph for
retrieval, and returns that text (later chunked/embedded like a document).

**Gotchas.** Rejects images > **5 MiB** (`bytes.len() > 5 * 1024 * 1024` — the
same threshold and unit as `image_util::MAX_BYTES`); supports `png/jpg/jpeg/webp`
only; errors on
empty model output. This is the **ingest-time** image describer — distinct from
`ipc::extract_symbol_from_image`, which is the analyze-time symbol extractor.

### `ingest/chunker.rs`

`chunk_text(text, target_tokens, overlap_tokens) -> Vec<String>`. Called as
`chunk_text(text, 500, 50)`.

**Gotcha — it's a character heuristic, not a tokenizer.** "Tokens" are
approximated at **1 token ≈ 4 chars**; so 500/50 means ~2000-char chunks with
~200-char overlap. It normalizes whitespace, then walks the text trying to back
up to a sentence boundary (`.!?\n`) within the last third of each window.

### `ingest/embeddings.rs`

**Responsibility.** Local text embeddings via [`fastembed`](https://crates.io/crates/fastembed).

- `EMBED_DIM = 384` (BGE-small dimension) — the canonical constant; `lance.rs`
  imports it and validates every insert against it.
- A lazy global `static MODEL: OnceLock<Mutex<TextEmbedding>>` initialized with
  `BGESmallENV15`.
- `embed(texts) -> Vec<Vec<f32>>`.

**Invariants / gotchas.**

- **First use downloads the model.** `BGESmallENV15` is fetched on the first
  `embed`/`search` call (slow, network-dependent); subsequent calls reuse the
  cached singleton.
- **CPU-only and fully serialized.** All embedding goes through one `Mutex`, so
  embedding is single-threaded across the whole process. This is a deliberate
  fit for the **8 GB RAM hard constraint** (no GPU, bounded memory), but it
  means concurrent ingests/searches contend on this lock.
- `get_or_init` uses `MODEL.set(...)` without holding a lock across init, so
  under a race two models could be constructed transiently; only one is kept.

### `ingest/watcher.rs`

`run_watcher(store, library)` — a blocking loop (started on its own task in
`lib.rs`) that watches the library directory via
[`notify`](https://crates.io/crates/notify) (recursive) and auto-ingests dropped
files.

**Invariants / gotchas.**

- **800 ms debounce.** `Create`/`Modify` events are coalesced per path; a path
  is processed only once it's been quiet for ≥ 800 ms (avoids ingesting a file
  mid-copy).
- PDFs ingest with no API key; images need a key — and if
  `require_api_key()` fails, **image ingest is silently skipped** (`return`),
  not surfaced to the user.
- The watcher model id for images is **hard-coded** here
  (`claude-haiku-4-5-20251001`) rather than read from settings — a known
  divergence from `model_extract`.
- Errors are logged via `tracing` only; there is no UI feedback for
  watcher-triggered ingests.

---

## `lance.rs` — vector store ⚠️ **MISNOMER: this is NOT LanceDB**

> **Read this first.** The names `lance.rs`, `LanceStore`, `lance::open`, and
> `paths::lancedb()` are **misnomers**. There is **no LanceDB** anywhere in this
> project. This module is a **plain SQLite table (`doc_chunks`) with embeddings
> stored as little-endian `f32` BLOBs**, and similarity search is a
> **brute-force cosine scan in Rust** over *every* row. The names are historical;
> do not infer any LanceDB behavior (columnar storage, ANN indexes, etc.).

**Responsibility.** Persist and search document-chunk embeddings (`vec.db`).

- `DocRow { id, doc_path, doc_type, chunk_index, text, page, embedding: Vec<f32> }`.
- `LanceStore { inner: Arc<Mutex<Connection>> }` (cheaply `Clone`).
- `open(path)` — creates parent dir, opens SQLite, sets WAL, applies the
  `doc_chunks` schema + a `doc_path` index.
- `insert(rows)` — transactional; **validates `embedding.len() == EMBED_DIM`
  (384)** per row, erroring otherwise; uses `INSERT OR REPLACE` keyed on `id`.
- `search(query_emb, k)` — returns early-empty if the query dim ≠ 384; otherwise
  loads **all** rows, computes `cosine` against each, sorts descending,
  truncates to `k`.
- `delete_by_doc_path(doc_path)` — used to replace a document's chunks on
  re-ingest.

**Invariants / gotchas.**

- **O(N) per query.** Every search deserializes and scores the entire table.
  Fine at the small corpus sizes the 8 GB-RAM target implies; it will not scale
  to large libraries. ANN indexing is **Planned — not yet implemented**.
- Embedding (de)serialization is manual little-endian `f32`
  (`embedding_to_bytes` / `bytes_to_embedding`); a BLOB whose length isn't a
  multiple of 4 decodes to an empty vector, which then scores `0.0`.
- `cosine` returns `0.0` for length-mismatched or empty vectors rather than
  erroring — dimension drift degrades silently to "no match" instead of a hard
  failure.

---

## `retrieval.rs` — query → top-k chunks

**Responsibility.** Thin RAG glue. `search(store, query, k)` embeds the query
string via `embeddings::embed`, takes the first vector, and calls
`store.search`. Returns `vec![]` if the embedding is empty.

**Gotcha.** This is the only place embeddings and the vector store are joined for
retrieval; `ipc::analyze` always asks for `k = 6`.

---

## `predictions.rs` — self-calibration loop

**Responsibility.** Record each analysis as a *prediction* and later resolve it
against real price movement. This is the system's self-calibration mechanism.

- `Prediction` mirrors the `predictions` table (symbol, `horizon_seconds`,
  `predicted_prob_up`, `recommended_action`, `stop_loss_pct?`,
  `take_profit_pct?`, `citations_json?`, `outcome_status`, the three outcome
  fields).
- `insert_from_analysis(db, symbol, &AnalysisJson)` — inserts a row with
  `outcome_status = 'pending'` (default) and returns its id.
- `pending_due(db)` — `SELECT … WHERE outcome_status='pending' AND
  (ts + horizon_seconds) <= now`.
- `resolve_one(db, &p)` — fetches the **current** CoinGecko price; on success
  sets `outcome_status='resolved'`, `outcome_price_at_horizon`,
  `outcome_resolved_at`; on price-fetch failure sets `outcome_status='failed'`.
- `record_initial_price(db, id, price)` — sets `outcome_price_at_ts` (called
  fire-and-forget from `analyze`).

**Invariants / gotchas.**

- **Resolution records, it does not simulate.** `resolve_one` compares
  *price-at-resolution* vs *price-at-prediction* (`outcome_price_at_ts`). It does
  **not** check whether SL or TP would have been hit *intra-horizon* — there's no
  intrabar/tick simulation. Any "did my stop get hit?" logic is **Planned — not
  yet implemented**.
- Resolution price is sampled **whenever the resolver happens to run after the
  horizon elapses**, not exactly at `ts + horizon_seconds`; with the 60 s poll
  (see [`prune`](#prune-pruners)) it can lag by up to ~60 s plus any backlog.
- A symbol absent from CoinGecko's table causes resolution to mark the row
  `failed` (the price fetch errors).

---

## `coingecko.rs` — spot price lookups

**Responsibility.** USD spot prices from CoinGecko's free
`/simple/price` endpoint.

- `symbol_to_id(symbol)` — uppercases, strips a trailing `USDT`/`USD`/`USDC`
  suffix, then maps a **hard-coded ~12-coin table** (BTC, ETH, SOL, BNB, XRP,
  ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC) to CoinGecko ids. Anything else →
  `None`.
- `get_usd_price(symbol)` — errors `AppError::Invalid` for unmapped symbols,
  otherwise GETs the price.

**Gotchas.**

- **Only those ~12 coins are supported.** A valid-but-unlisted ticker fails
  symbol resolution; this affects both initial-price recording and prediction
  resolution.
- Suffix stripping is order-sensitive: `trim_end_matches("USDT")` then `"USD"`
  then `"USDC"`. (e.g. `BTCUSDC` → `BTC`.)
- No API key, no rate-limit handling, no retry — a 429/5xx surfaces as an error
  and (for resolution) marks the prediction `failed`.

---

## `cost.rs` — token cost accounting & cap

**Responsibility.** Track per-day Anthropic spend in `cost_daily` and enforce the
daily cap.

- `ModelRates { input_per_mtok, cached_per_mtok, output_per_mtok }`.
- `rates_for(model)` — substring match: contains `"opus"` → Opus rates
  ($15 / $1.50 / $75 per Mtok in/cached/out); contains `"haiku"` → Haiku
  ($1 / $0.10 / $5); **everything else defaults to Sonnet** ($3 / $0.30 / $15).
- `cost_usd(rates, input, cached, output)` — the arithmetic.
- `add_usage(db, model, …)` — upserts today's row (`ON CONFLICT(date) DO UPDATE`,
  accumulating) and returns the running `DailyCost`.
- `today(db)` — today's `DailyCost`, or a zeroed default if no row yet.
- `is_over_cap(today_cost, cap)` — `today_cost >= cap`.

**Invariants / gotchas.**

- **Rates are hard-coded** and are a substring heuristic. An unknown/misspelled
  model id is silently billed at **Sonnet** rates — verify model names match the
  intended tier.
- "Cached input" is billed at the cheap cached rate; recall from
  [`streaming`](#llmstreamingrs) that *cache-creation* tokens are folded into the
  full-rate `input` bucket.
- Accounting is **per calendar day in UTC** (`Utc::now().format("%Y-%m-%d")`).

---

## `settings.rs` — config & API-key (keyring)

**Responsibility.** Non-secret app settings (in the `settings` table) **and** the
Anthropic API key (in the OS keyring — never the DB).

- `Settings { daily_cost_cap_usd, model_main, model_extract, library_path?,
  hotkey }`; `Default` = cap **$2.0**, `model_main = claude-sonnet-4-6`,
  `model_extract = claude-haiku-4-5-20251001`, hotkey `Ctrl+Shift+Space`.
- `load(db)` / `save(db, &Settings)` — key/value upserts in the `settings` table.
- API key via [`keyring`](https://crates.io/crates/keyring): service
  `"cognitrade"`, user `"anthropic_api_key"`. `get_api_key` / `set_api_key` /
  `clear_api_key` / `require_api_key` (the last returns `AppError::NoApiKey` when
  unset).

**Invariants / gotchas.**

- **The API key is never persisted to SQLite.** On Windows the keyring backend is
  the **Windows Credential Manager**. `get_api_key_set` only reveals
  presence/absence, never the value.
- `load` tolerates a partially-populated `settings` table: it starts from
  `Settings::default()` and overlays whatever rows exist (unparseable
  `daily_cost_cap_usd` is ignored, keeping the default).

---

## `paths.rs` — filesystem layout

**Responsibility.** Canonical locations under `%USERPROFILE%\CogniTrade\`.

| Method | Path |
|---|---|
| `library()` | `…\CogniTrade\library` |
| `screenshots()` | `…\CogniTrade\screenshots` |
| `data()` | `…\CogniTrade\data` |
| `chat_db()` | `…\CogniTrade\data\chat.db` |
| `lancedb()` | `…\CogniTrade\data\vec.db` ⚠️ misnamed accessor — see [`lance`](#lancers--vector-store--misnomer-this-is-not-lancedb) |
| `logs()` | `…\CogniTrade\logs` |

- `from_env()` resolves the home dir via `dirs::home_dir()` (panics if
  unresolvable).
- `ensure_all()` creates `library/screenshots/data/logs` (not the DB files
  themselves — `db::open`/`lance::open` create those).

**Gotcha.** `lancedb()` returns the path to **`vec.db`** (SQLite), reinforcing the
LanceDB misnomer. The method name is misleading; the file is plain SQLite.

---

## `db.rs` — chat.db connection & migration

**Responsibility.** Open and migrate `chat.db`.

- `type Db = Arc<Mutex<Connection>>` — the shared handle threaded through
  `chat_store`, `settings`, `cost`, `predictions`, `prune`.
- `open(path)` — creates parent dir, opens SQLite, sets `journal_mode=WAL` and
  `foreign_keys=ON`, and applies `migrations/001_initial.sql` (embedded via
  `include_str!`).

**Schema (`migrations/001_initial.sql`):** `messages`, `predictions`,
`cost_daily`, `settings`. `messages.prediction_id` is a FK to `predictions(id)`;
`messages.role` is `CHECK`ed to `user|assistant|system`.

**Gotchas.**

- **Single-migration, idempotent (`IF NOT EXISTS`)** — there is no versioned
  migration runner yet; schema changes mean editing `001` or adding new logic.
- All DB access is serialized behind one `Mutex` — every helper does
  `db.lock().unwrap()`. A poisoned lock (panic while held) would propagate via
  the `unwrap`.

---

## `chat_store.rs` — message persistence

**Responsibility.** CRUD for the `messages` table.

- `Message` (full row) / `NewMessage` (insert payload:
  `role, content, image_path?, prediction_id?`).
- `insert(db, NewMessage)` — stamps `ts = now`, returns the new id.
- `recent(db, limit)` — newest `limit` rows by `ts DESC`, then **reversed** so the
  caller gets chronological (oldest→newest) order.
- `prune_older_than_secs(db, max_age_secs)` — deletes rows older than the cutoff;
  returns the deleted count.

**Gotcha.** `recent` orders by `ts`, and `ts` is a whole-second UNIX timestamp;
two messages inserted within the same second can sort by insertion artifact, not
true order. In practice user/assistant turns are seconds apart.

---

## `prune.rs` — background maintenance loops

**Responsibility.** `spawn_background(db)` starts **two** independent Tauri async
tasks:

1. **Chat prune** — `chat_store::prune_older_than_secs(db, 7 days)`. The loop
   prunes **before** its first `sleep`, so it runs **once immediately at
   startup**, then every **6 hours**.
2. **Prediction resolution** — `predictions::pending_due` then
   `predictions::resolve_one` for each due prediction. This loop `sleep`s **at
   the top**, so the **first resolution pass is delayed ~60 s after launch**;
   thereafter it runs every **60 seconds**.

**Invariants / gotchas.**

- **First-run timing differs between the two loops** (see above): chat-prune
  fires at startup; prediction resolution waits one 60 s interval first.
- Both loops run forever; errors are logged (`tracing::warn!`) or swallowed
  (`continue`) and never crash the app.
- **Chat history is hard-capped at 7 days.** Messages older than a week are
  deleted permanently — there is no archival. Predictions are *not* pruned (only
  resolved).
- Resolution is **serial** within a tick: due predictions are awaited one at a
  time, so a large backlog plus CoinGecko latency lengthens the effective cycle
  beyond 60 s.

---

## `tray.rs` — system tray & panel positioning

**Responsibility.** The tray icon, its menu, and show/hide of the panel window.

- `init_tray(app)` — builds a tray with **Toggle Panel** / **Quit** menu items;
  left-click on the icon also toggles. **Quit** calls `app.exit(0)`.
- `toggle_panel(app)` — shows/hides the `"main"` webview window; on show it
  repositions and focuses.
- `position_panel(win)` — docks a **420×800** (logical) panel to the top-right of
  the current monitor, applying the monitor's `scale_factor` to compute physical
  size/position.

**Gotchas.**

- The window id must be `"main"` (`get_webview_window("main")`); renaming it in
  `tauri.conf.json` breaks toggling.
- Sizing/positioning math is DPI-aware via `scale_factor`; it falls back to the
  first available monitor when `current_monitor` is unavailable.
- This is the target of both the tray click/menu and the
  `Ctrl+Shift+Space` global hotkey registered in `lib.rs`.

---

## `image_util.rs` — screenshot resizing

**Responsibility.** Keep images within Claude's vision limits before they're sent.

- `MAX_LONG_EDGE = 1568`, `MAX_BYTES = 5 MiB`.
- `needs_resize(path)` — `true` if the file exceeds 5 MiB **or** its longest edge
  exceeds 1568 px.
- `resize_in_place(path)` — Lanczos3 downscale preserving aspect ratio,
  **overwriting the original file**.

**Gotchas.**

- **In-place overwrite.** `resize_in_place` mutates the user's screenshot file on
  disk; the original full-resolution image is lost.
- It re-saves in the format inferred from the path extension; screenshots are
  PNG, matching the `image/png` media type assumed downstream.
- `needs_resize` decodes the image just to read dimensions — a corrupt image
  yields `AppError::Internal("decode: …")`. `analyze` calls it with
  `.unwrap_or(false)`, so a decode failure simply skips resizing.

---

## `error.rs` — unified error type

**Responsibility.** One `AppError` enum + a `Result<T>` alias used across the
backend, serialized to the frontend.

- Variants: `Io`, `Sqlite`, `Http`, `Serde`, `Keyring` (all `#[from]`),
  `Anthropic(String)`, `CostCapReached`, `NoApiKey`, `NotFound(String)`,
  `Invalid(String)`, `Internal(String)`.
- `pub type Result<T> = std::result::Result<T, AppError>;`
- **Custom `Serialize`** — serializes to its `Display` string (via
  `thiserror`'s `#[error("…")]`). Because every `#[tauri::command]` returns
  `Result<T>`, the frontend receives a plain error *string*, **not** a structured
  error object.

**Gotchas.**

- The frontend must match on the **message text** to distinguish failures (there
  are no error codes). Notably `CostCapReached` serializes to
  `"daily cost cap reached"` and `NoApiKey` to `"api key not set"`.
- The blanket `#[from]` conversions mean most `?` propagations land in `Io`,
  `Sqlite`, `Http`, etc. automatically; reserve `Invalid`/`Internal` for
  domain-specific failures.
