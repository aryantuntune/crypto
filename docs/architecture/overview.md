# System Architecture Overview

CogniTrade is a **Tauri 2 desktop application** — a Rust backend plus a SvelteKit/TypeScript (Svelte 5) static-SPA frontend — that acts as an **advisory crypto-chart analyst**. The user drops or pastes a chart screenshot; the backend reads it with Claude (vision), retrieves relevant passages from ingested PDFs/images (RAG), and streams back a plain-English analysis ending in a structured JSON recommendation.

> **It is advisory, not autonomous.** There is no exchange integration (Binance/Kraken), no order execution, no Minimax decision engine, and no automated stop-loss/take-profit enforcement. The system prompt is explicit: *"You are an advisor. The user places all trades themselves."* The original brief (`crypto Dhriti.txt`) and the design docs under [`docs/superpowers/`](../superpowers/) describe a far more ambitious autonomous trading bot — treat those as aspirational background, not a description of this codebase. Where this document mentions a spec capability that does not exist in code, it is labeled **Planned — not yet implemented**.

Related docs:

- Codebase guide and conventions: [`CLAUDE.md`](../../CLAUDE.md)
- Original product brief (aspirational): [`crypto Dhriti.txt`](../../crypto%20Dhriti.txt)
- Phase-by-phase plan (aspirational, diverges from code): [`docs/superpowers/`](../superpowers/)

---

## 1. High-level component diagram

The app is a single OS process. The frontend (WebView2 on Windows) talks to the Rust backend exclusively through **Tauri IPC** (`invoke` commands) and receives streamed output through **Tauri events**. Two external HTTP services are used: the **Anthropic API** (analysis + vision) and **CoinGecko** (spot prices).

```
+--------------------------------------------------------------------------+
|  Desktop process (Tauri 2)                                               |
|                                                                          |
|  +-----------------------------+        +-----------------------------+  |
|  |  Svelte 5 frontend (SPA)    |        |  Rust backend (lib.rs run())|  |
|  |  src/routes/+page.svelte    |        |  AppState { db, lance,      |  |
|  |   3 tabs: Chat/Library/     |        |             paths, llm }    |  |
|  |          Settings           |        |                             |  |
|  |  lib/ipc.ts  (invoke)       |        |  IPC handlers (ipc.rs):     |  |
|  |  lib/stores/chat.ts (events)| <----> |   analyze, ingest_path,     |  |
|  +-------------+---------------+  Tauri |   get/save_settings,        |  |
|                |                   IPC  |   get/set/clear_api_key,    |  |
|   invoke('analyze', ...)   <-- cmd ---> |   cost_today, save_screenshot|  |
|   listen('analysis_chunk') <-- event -- |   list_recent_messages      |  |
|   listen('analysis_done')  <-- event -- +------+----------------------+  |
|                                                |                         |
|                          +---------------------+---------------------+   |
|                          |                                           |   |
|         +----------------v-----------+        +--------------------v---+ |
|         | LLM client (llm/*)          |        | RAG store "LanceStore" | |
|         |  client.rs  streaming SSE   |        |  lance.rs  (SQLite!)   | |
|         |  prompt.rs  build_messages  |        |  brute-force cosine    | |
|         |  streaming.rs SSE parser    |        |  data/vec.db doc_chunks| |
|         |  parse.rs   extract_json_tail|       +-----------+------------+ |
|         |  types.rs   AnalysisJson    |                    ^              |
|         +----------------+------------+        +-----------+------------+ |
|                          |                     | Ingest (ingest/*)      | |
|         +----------------v------------+        |  pdf.rs / image.rs     | |
|         | Predictions (predictions.rs)|        |  chunker.rs (500/50)   | |
|         |  insert_from_analysis       |        |  embeddings.rs BGE-small| |
|         |  resolve_one (calibration)  |        |  watcher.rs (notify)   | |
|         +----------------+------------+        +-----------+------------+ |
|                          |                                 |             |
|         +----------------v------------+        +-----------v----------+  |
|         | Cost (cost.rs) cost_daily   |        | Settings (settings.rs)| |
|         |  rates_for / analyze_with_cap|       | + OS keyring (API key)| |
|         +-----------------------------+        +----------------------+  |
|                                                                          |
|  Storage: data/chat.db (db.rs) + data/vec.db (lance.rs), both WAL        |
|  Shell: tray.rs (system tray) + Ctrl+Shift+Space global hotkey          |
+----------------------------+----------------------------+----------------+
                             |                            |
                  HTTPS (reqwest, rustls)         HTTPS (reqwest)
                             v                            v
                  api.anthropic.com               api.coingecko.com
                  (analysis + vision)             (simple/price USD)
```

### Component responsibilities

| Component | File(s) | Responsibility |
|-----------|---------|----------------|
| Frontend shell | `src/routes/+page.svelte` | 3-tab UI: Chat / Library / Settings |
| IPC wrappers | `src/lib/ipc.ts` | Typed `invoke()` over every backend command |
| Chat store | `src/lib/stores/chat.ts` | `listen`s for `analysis_chunk` / `analysis_done`; drives Svelte stores |
| App bootstrap | `src-tauri/src/lib.rs` | Builds `AppState`, tray, hotkey, spawns watcher + background loops, registers IPC handlers |
| IPC handlers | `src-tauri/src/ipc.rs` | All `#[tauri::command]`s; `analyze` is the core flow |
| LLM client | `src-tauri/src/llm/{mod,client,prompt,streaming,parse,types}.rs` | Anthropic streaming client, prompt builder, SSE parser, JSON-tail parser, `AnalysisJson` |
| RAG / vector store | `src-tauri/src/lance.rs`, `retrieval.rs` | SQLite-backed embedding store + brute-force cosine search |
| Ingest | `src-tauri/src/ingest/{mod,chunker,pdf,image,embeddings,watcher}.rs` | PDF/image → chunk → embed → store; library file watcher |
| Predictions | `src-tauri/src/predictions.rs` | Self-calibration: record each analysis, resolve later vs CoinGecko |
| Cost control | `src-tauri/src/cost.rs` | Per-day token accounting + daily USD cap |
| Settings & secrets | `src-tauri/src/settings.rs` | Non-secret settings in DB; API key in OS keyring |
| Prices | `src-tauri/src/coingecko.rs` | Hardcoded ~12-coin ticker → CoinGecko id, USD spot price |
| Background loops | `src-tauri/src/prune.rs` | Chat prune (6h) + prediction resolution (60s) |
| Tray | `src-tauri/src/tray.rs` | System tray + panel toggle |

---

## 2. The core flow: `ipc::analyze`

The entire user-facing app is essentially one IPC command, `analyze` (`src-tauri/src/ipc.rs`). The frontend calls it through `api.analyze(...)` ([`src/lib/ipc.ts`](../../src/lib/ipc.ts)), then consumes two Tauri events:

- `analysis_chunk` — a string text delta, emitted repeatedly as the model streams.
- `analysis_done` — `{ message_id, analysis }`, emitted once at the end with the parsed `AnalysisJson` (or `null` if no valid JSON tail was found).

`src/lib/stores/chat.ts` registers both listeners in `initEvents()`; `analysis_chunk` appends to the `streaming` store, and `analysis_done` sets `analysisResult`, clears `streaming`, flips `isAnalyzing` off, and refreshes the message list.

> **`analysis_done` is not the only path that clears `isAnalyzing`.** Error paths in `analyze` (e.g. `AppError::CostCapReached` when the daily cap is hit, or `AppError::NoApiKey` when no key is set) reject the `invoke` **without emitting `analysis_done`**. The frontend's `sendAnalyze` (`chat.ts`) handles this directly: its `catch` block flips `isAnalyzing` off and writes an inline `[error: ...]` string into the `streaming` store, and a `finally` block clears `isAnalyzing` (and refreshes) if it is still set. So a reader should not assume the only way `isAnalyzing` goes false is the `analysis_done` event.

### 2.1 Detailed sequence diagram

```
Svelte UI         ipc::analyze (Rust)         Anthropic API        CoinGecko        SQLite (chat.db / vec.db)
   |                     |                          |                  |                       |
   | invoke('analyze',   |                          |                  |                       |
   |   { req })          |                          |                  |                       |
   |-------------------->|                          |                  |                       |
   |                     | chat_store::insert(user message)            |                       |
   |                     |--------------------------------------------------------------------->| (messages)
   |                     |                          |                  |                       |
   |                     | image_util::needs_resize / resize_in_place  |                       |
   |                     |   (downscale to <=1568px long edge, <=5MB)  |                       |
   |                     |                          |                  |                       |
   |                     | chat_store::recent(10)  (history, minus this user msg)              |
   |                     |<--------------------------------------------------------------------|
   |                     |                          |                  |                       |
   |                     | lance_get_or_open()  (lazy: open data/vec.db once, cache in Mutex)  |
   |                     |                          |                  |                       |
   |                     | resolve SYMBOL:          |                  |                       |
   |                     |  (a) req.symbol_hint if non-empty -> uppercased                     |
   |                     |  (b) else extract_symbol_from_image()       |                       |
   |                     |        Haiku vision (model_extract) -------->|                       |
   |                     |        "ONLY the ticker" <------------------|  (on any error -> "BTCUSDT")
   |                     |  (c) else fallback "BTCUSDT"                 |                       |
   |                     |                          |                  |                       |
   |                     | RAG: q = "{SYMBOL} chart pattern analysis"   |                       |
   |                     |  retrieval::search(store, q, k=6)           |                       |
   |                     |   embeddings::embed(q)  (BGE-small, 384-dim, behind global mutex)    |
   |                     |   LanceStore::search    (load ALL rows, cosine in Rust, top-6)      |
   |                     |<--------------------------------------------------------------------| (doc_chunks)
   |                     |                          |                  |                       |
   |                     | analyze_with_cap():      |                  |                       |
   |                     |  - settings::load + cost::today                                     |
   |                     |  - if cost::is_over_cap(cost_usd, cap) -> Err(CostCapReached)       |
   |                     |  - settings::require_api_key() (OS keyring)  |                       |
   |                     |  - build_messages(): history + user turn:    |                       |
   |                     |      [cached retrieved-knowledge block]      |                       |
   |                     |      [image block (base64 PNG)]              |                       |
   |                     |      [user_text]                             |                       |
   |                     |    system prompt = SYSTEM_PROMPT (cache_control: ephemeral)         |
   |                     |  - POST /v1/messages (stream=true) --------->|                       |
   |                     |                          |  SSE: message_start (usage)               |
   |                     |                          |       content_block_delta (text_delta)... |
   |                     |   spawn stream_task: rx.recv() -> emit('analysis_chunk', delta)     |
   | <-- 'analysis_chunk'| <----------------------- text deltas via mpsc(64) channel           |
   | (streaming store     |                          |  ...                                      |
   |  appends)           |                          |       message_delta (final usage)         |
   |                     |                          |       message_stop                        |
   |                     |  - assemble full text; extract_json_tail()  |                       |
   |                     |      = last ```json ... ``` block -> AnalysisJson (validated)        |
   |                     |  - cost::add_usage(model, in, cached, out)  |                       |
   |                     |---------------------------------------------------------------------> | (cost_daily)
   |                     |                          |                  |                       |
   |                     | if analysis present:     |                  |                       |
   |                     |  predictions::insert_from_analysis(symbol, a) --------------------->| (predictions, pending)
   |                     |  spawn: coingecko::get_usd_price(symbol) --->|                       |
   |                     |          record_initial_price(id, price) (fire-and-forget) -------->| (outcome_price_at_ts)
   |                     |                          |                  |                       |
   |                     | chat_store::insert(assistant message, prediction_id) -------------->| (messages)
   |                     | emit('analysis_done', { message_id, analysis })                     |
   | <-- 'analysis_done' |<--------------------------------------------------------------------|
   | (set analysisResult,|                          |                  |                       |
   |  clear streaming,   |                          |                  |                       |
   |  refreshMessages)   |                          |                  |                       |
   |                     | return AnalyzeAck { message_id }                                    |
   | <-- invoke resolves |<--------------------------------------------------------------------|
```

### 2.2 Step-by-step notes (what the code actually does)

1. **Persist the user message** (`chat_store::insert`) with role `user`, the text, and the optional screenshot path.
2. **Resize the screenshot in place** if it exceeds 5 MB or 1568px on the long edge (`image_util::needs_resize` / `resize_in_place`, Lanczos3). This keeps the vision payload within Anthropic limits.
3. **Load history**: the last 10 messages (`chat_store::recent(10)`), excluding the just-inserted user message, mapped to `ChatMessage` turns.
4. **Open the vector store lazily** (`lance_get_or_open`): `AppState.lance` is `Arc<Mutex<Option<LanceStore>>>`; first call opens `data/vec.db` and caches the handle.
5. **Resolve the symbol** in priority order:
   - non-empty `symbol_hint` → uppercased;
   - else, if a screenshot exists, a tiny non-streaming **Haiku vision call** (`extract_symbol_from_image`, `max_tokens: 40`) returns the ticker — **any error falls back to `BTCUSDT`**;
   - else `BTCUSDT`.
6. **RAG retrieval**: build query `"{SYMBOL} chart pattern analysis"`, embed it, and run `LanceStore::search` for the top **k = 6** chunks (texts only are kept).
7. **Build the Anthropic request** (`llm::prompt::build_messages`): chat history, then one `user` turn made of up to three content blocks — a **cached retrieved-knowledge text block** (`cache_control: ephemeral`, only if chunks exist), an **image block** (base64 PNG), and the **user text**. The **system prompt** (`SYSTEM_PROMPT`) is also sent with `cache_control: ephemeral`. `max_tokens` for the main call is **1500**.
8. **Cap check before the call** (`analyze_with_cap`, `llm/client.rs`): it loads `cost::today(db)` and `settings`, then calls `cost::is_over_cap(today.cost_usd, settings.daily_cost_cap_usd)` (defined as `today_cost >= cap`, default cap `$2`). If over cap it returns `AppError::CostCapReached` *before* spending anything.
9. **Stream** (`LlmClient::analyze`): `POST /v1/messages` with `stream: true`. Bytes are buffered and split on `\n\n` SSE event boundaries; each event is parsed by `llm::streaming::parse_event`. `text_delta`s are pushed into an `mpsc::channel::<String>(64)`; a spawned `stream_task` drains the channel and emits each delta as an `analysis_chunk` event. Usage counts come from `message_start` (initial) and `message_delta` (final output tokens); `cache_creation_input_tokens` is folded into `input_tokens`, `cache_read_input_tokens` is tracked as `cached_input_tokens`.
10. **Parse the JSON tail** (`llm::parse::extract_json_tail`): find the **last** ```` ```json ```` fence in the assembled text, parse the body into `AnalysisJson`, and run `validate()` (currently: `probability_up` must be in `[0, 1]`). On any failure this returns `None` — **the recommendation is a JSON tail of free text, not a tool-use block.**
11. **Record cost** (`cost::add_usage`) for the day.
12. **Persist a prediction** if an analysis parsed (`predictions::insert_from_analysis`), then **fire-and-forget** record the current CoinGecko price into `outcome_price_at_ts`.
13. **Persist the assistant message** (full text + `prediction_id`) and **emit `analysis_done`** with `{ message_id, analysis }`.

### `AnalysisJson` schema (the JSON tail)

`src-tauri/src/llm/types.rs`:

| Field | Type | Notes |
|-------|------|-------|
| `action` | `"buy" \| "sell" \| "hold"` | enum `Action` |
| `probability_up` | `f32` in `[0,1]` | validated; out-of-range → tail rejected |
| `horizon` | `"1h" \| "4h" \| "1d" \| "3d" \| "1w"` | enum `Horizon`; `seconds()` maps to 3600..604800 |
| `stop_loss_pct` | `f32?` | advisory only — **not enforced** |
| `take_profit_pct` | `f32?` | advisory only — **not enforced** |
| `key_signals` | `[string]` | optional; **not persisted** to the predictions row (see §4) |
| `citations` | `[{ doc, page? }]` | optional; serialized to `citations_json` on the predictions row |

> **Planned — not yet implemented:** automated SL/TP enforcement and order execution. The spec treats hard SL/TP as non-negotiable, but in this codebase `stop_loss_pct` / `take_profit_pct` are *stored advisory numbers only*. No code path acts on them, and `predictions::resolve_one` does **not** simulate intra-horizon SL/TP hits.

---

## 3. The ingestion path (drop file → store)

Documents become retrievable RAG context through two entry points that converge on the same chunk → embed → store pipeline.

```
                 +------------------------------+
 user drops file |  %USERPROFILE%\CogniTrade\   |        OR    Library tab -> ingest_path(path)
 into library    |  library\  (notify watcher)  |              (ipc::ingest_path, by extension)
                 +---------------+--------------+
                                 |
              EventKind::Create/Modify, 800ms debounce (watcher.rs)
                                 |
                 +---------------v--------------+
                 |  dispatch by extension       |
                 |   .pdf  -> ingest_pdf        |   (no API key needed)
                 |   .png/.jpg/.jpeg/.webp      |   (needs API key; skipped silently if unset)
                 |        -> ingest_image       |
                 +---------------+--------------+
                                 |
        ingest_pdf:  pdf_extract::extract_text_from_mem
        ingest_image: Haiku describes the image -> one dense paragraph
                      (Library-tab route uses settings.model_extract;
                       watcher route HARDCODES claude-haiku-4-5-20251001)
                                 |
                 +---------------v--------------+
                 | chunker::chunk_text(text,    |   ~500 "tokens" target, 50 overlap
                 |   target=500, overlap=50)    |   (1 token ~= 4 chars heuristic;
                 |                              |    backs up to sentence boundary)
                 +---------------+--------------+
                                 |
                 +---------------v--------------+
                 | embeddings::embed(chunks)    |   fastembed BGE-small (BGESmallENV15)
                 |   global OnceLock<Mutex<..>> |   CPU-only, 384-dim, synchronous behind mutex
                 |   downloads model on 1st use |
                 +---------------+--------------+
                                 |
                 +---------------v--------------+
                 | LanceStore (data/vec.db)     |
                 |  delete_by_doc_path(path)    |   (replace-by-path: idempotent re-ingest)
                 |  insert(rows: DocRow)        |   embedding stored as LE f32 BLOB;
                 |   dim validated == EMBED_DIM |   INSERT OR REPLACE into doc_chunks
                 +------------------------------+
```

Key facts:

- **Two ingest entry points.** The `ingest::watcher` (started from `lib.rs` setup) auto-ingests files dropped into the library dir using `notify` with an **800 ms debounce**. The **Library tab** calls `ipc::ingest_path(path)` directly. Both produce an `IngestReport { doc_path, chunks }`.
- **PDFs** (`ingest::ingest_pdf`): `pdf-extract` pulls all text, then `chunk_text(text, 500, 50)`. No API key required.
- **Images** (`ingest::ingest_image`): a **Haiku** call (`max_tokens: 600`) produces a single dense descriptive paragraph; that paragraph is chunked/embedded. **Requires an API key** — in the watcher path, images are **silently skipped** if no key is set.
  - **Which model is used depends on the entry point.** The **Library-tab** route (`ipc::ingest_path`) honors the configured `settings.model_extract`. The **auto-watcher** route (`src-tauri/src/ingest/watcher.rs`, ~line 50) **hardcodes** `let model = "claude-haiku-4-5-20251001"` and **does NOT read `settings.model_extract`**. So if a user changes `model_extract` in Settings, that choice is respected only for manual Library-tab ingest, **not** for files dropped into the watched library directory.
- **Idempotent re-ingest**: before inserting, both paths call `delete_by_doc_path(path)`, so re-dropping a file replaces its chunks rather than duplicating them.
- **Dimension safety**: `LanceStore::insert` rejects any row whose embedding length ≠ `EMBED_DIM` (384) with `AppError::Invalid`.

### The "Lance" store is **not** LanceDB

`lance.rs`, `LanceStore`, and `paths::lancedb()` are **misnomers** — there is no LanceDB dependency. It is plain **SQLite** (`data/vec.db`, WAL mode, single table `doc_chunks`) storing embeddings as little-endian `f32` BLOBs. `LanceStore::search` loads **every row** on each query and computes cosine similarity in Rust, sorts, and truncates to `k`. This is fine at small scale and is the obvious bottleneck if the library grows large.

---

## 4. The prediction self-calibration loop

"Predictions" are the system's self-evaluation / calibration mechanism — the closest thing to the spec's "probabilistic reasoning." Each analysis that yields a valid `AnalysisJson` records a prediction row; a background loop later resolves it against the actual CoinGecko price.

```
  analyze (per request)                 prune::spawn_background loop #2 (every 60s)
  ---------------------                 -------------------------------------------
  insert_from_analysis():               pending_due():
    symbol, horizon_seconds,              SELECT ... WHERE outcome_status='pending'
    predicted_prob_up,                      AND (ts + horizon_seconds) <= now
    recommended_action,                  for each due prediction:
    stop_loss_pct, take_profit_pct,        resolve_one():
    citations_json,                          price = coingecko::get_usd_price(symbol)
    outcome_status = 'pending'               on Ok:  outcome_status='resolved',
                                                     outcome_price_at_horizon = price,
  (fire-and-forget right after):                     outcome_resolved_at = now
    record_initial_price(id, price_now)    on Err: outcome_status='failed',
      -> outcome_price_at_ts                        outcome_resolved_at = now
```

- **Stored at prediction time** (`predictions::insert_from_analysis`): `symbol`, `horizon_seconds` (from `Horizon::seconds()`), `predicted_prob_up`, `recommended_action`, advisory `stop_loss_pct` / `take_profit_pct`, `citations_json` (`serde_json::to_string(&a.citations)`), `outcome_status='pending'`, and (fire-and-forget) `outcome_price_at_ts` = the spot price at the moment of prediction. **`key_signals` is NOT persisted** — the only free-text payload retained on the row is `citations_json`, so anyone auditing the retained calibration data should not expect `key_signals` to be queryable from the `predictions` table.
- **Resolution** (`predictions::resolve_one`): the 60 s loop finds predictions whose `ts + horizon_seconds <= now` and fetches the **current** price as `outcome_price_at_horizon`. Success → `resolved`; if CoinGecko fails (e.g. unlisted symbol) → `failed`.
- **What it does NOT do**: it records *price-at-horizon vs price-at-prediction* only. It does **not** simulate whether SL or TP was hit *during* the horizon, and there is no UI surfacing calibration metrics (Brier score, hit rate) yet.
- **Symbol coverage**: resolution depends on `coingecko::symbol_to_id`, a hardcoded ~12-coin table (BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC), stripping `USDT`/`USD`/`USDC` suffixes. Predictions on unlisted symbols resolve as `failed`.

> **Planned — not yet implemented:** "based on the last 100 similar patterns, 72% chance…" historical pattern-matching. The implemented loop scores the model's *own* probability calls over time; it does not yet derive probabilities from a corpus of past patterns.

---

## 5. Concurrency model

CogniTrade runs on a **single Tokio multi-threaded runtime** (Tauri's `async_runtime`, `tokio` "full"). There is no separate worker process. Concurrency is organized around two long-lived background loops, per-request spawned tasks, and a small number of mutex-guarded shared resources.

### 5.1 Long-lived tasks (spawned once at startup in `lib.rs::setup`)

| Task | Spawned by | Cadence | Work |
|------|-----------|---------|------|
| **Chat prune loop** | `prune::spawn_background` (loop #1) | once at startup, then every 6 h | `chat_store::prune_older_than_secs(7 days)` — delete messages older than 7 days. The loop **prunes first, then sleeps 6 h**, so a prune fires immediately on launch. |
| **Prediction resolution loop** | `prune::spawn_background` (loop #2) | every 60 s (first pass after 60 s) | resolve all `pending` predictions whose horizon has elapsed (`resolve_one`, hits CoinGecko). The loop **sleeps 60 s first, then resolves**, so the first resolution pass is delayed 60 s after launch. |
| **Library file watcher** | `tauri::async_runtime::spawn` wrapping `ingest::watcher::run_watcher` | event-driven, 800 ms debounce | auto-ingest dropped PDFs/images; each ready file is handled in its own spawned task |

The watcher itself runs a **blocking `recv_timeout` loop** on a `std::sync::mpsc` channel fed by `notify`; for each debounced, ready file it `tokio::spawn`s an async ingest task (`ingest_pdf` / `ingest_image`).

### 5.2 Per-request tasks (inside `analyze`)

- **Stream forwarder** (`stream_task`): drains the `mpsc::channel::<String>(64)` of text deltas and emits `analysis_chunk` events. Awaited before the request returns.
- **Price recorder**: a fire-and-forget `tokio::spawn` that fetches the current CoinGecko price and writes `outcome_price_at_ts`. Not awaited; failure is ignored.

### 5.3 Mutex-guarded shared resources

| Resource | Guard | Why |
|----------|-------|-----|
| **Embedding model** | `static OnceLock<Mutex<TextEmbedding>>` (`std::sync::Mutex`, `ingest/embeddings.rs`) | One BGE-small instance, lazily downloaded/initialized via `embeddings::get_or_init`; **all embedding is serialized** behind this mutex. Deliberate, to bound RAM/CPU on the 8 GB target. **This singleton gates both RAG search at analyze time AND ingestion**: the very first embedding call (whether via `retrieval::search` during an `analyze`, or via any ingest) triggers a one-time `BGESmallENV15` model download under this global mutex. On a fresh install the first analysis/ingest therefore has added latency and a **hard network dependency** — with no network it blocks/fails inside the mutex. |
| **Vector DB connection** | `LanceStore { inner: Arc<Mutex<Connection>> }` (`std::sync::Mutex`, `lance.rs`) | Single SQLite connection to `data/vec.db`; inserts/searches/deletes are serialized. |
| **Chat DB connection** | `Db = Arc<Mutex<Connection>>` (`std::sync::Mutex`, `db.rs`) | Single SQLite connection to `data/chat.db`, shared by IPC handlers and both background loops. |
| **Lazy store handle** | `AppState.lance: Arc<tokio::sync::Mutex<Option<LanceStore>>>` (`ipc.rs`) | Async mutex guarding one-time open of the vector store. |

> **Note on blocking work:** embedding (`embeddings::embed`), all SQLite access, and `pdf-extract` are **synchronous/CPU-bound** but are called directly from async tasks (not via `spawn_blocking`). Under load this can briefly occupy a runtime worker thread. This is an accepted trade-off for the small expected workload on the target hardware; it is the obvious place to add `spawn_blocking` if the library or request rate grows.

### 5.4 Two databases, opened separately

- `data/chat.db` (`db::open`, schema [`migrations/001_initial.sql`](../../src-tauri/migrations/001_initial.sql)): tables `messages`, `predictions`, `cost_daily`, `settings`. WAL + `foreign_keys=ON`.
- `data/vec.db` (`lance::open`): single table `doc_chunks`. WAL.

The **Anthropic API key is never in either DB** — it lives in the OS keyring (`keyring` crate, service `cognitrade`, user `anthropic_api_key`; Windows → Credential Manager). Only non-secret settings (`daily_cost_cap_usd`, `model_main`, `model_extract`, `library_path`, `hotkey`) live in the `settings` table.

---

## 6. Process, threading, and the 8 GB RAM budget

**Hardware target (hard constraint):** Ryzen 3400G, 8 GB RAM, 1 TB storage, 450 W PSU. The 8 GB ceiling drives model-size, concurrency, and embedding choices throughout.

- **One OS process.** The WebView2 frontend and the Rust backend live in the same Tauri process; there is no separate model server or sidecar. The heavy LLM (Claude) runs **remotely** at Anthropic, so the local RAM cost of analysis is essentially just the HTTP stream buffer and the base64 image.

### Where the 8 GB is spent (largest local consumers first)

| Consumer | Approx. footprint | Notes |
|----------|-------------------|-------|
| **WebView2 + frontend** | hundreds of MB | The Chromium-based webview is the single biggest baseline cost on Windows. |
| **BGE-small embedding model** | ~tens of MB resident (CPU) | `fastembed` `BGESmallENV15`, loaded **once** behind a global mutex (`embeddings::get_or_init`). **Downloaded on first use** — and that first call gates *both* analyze-time RAG search and ingestion, so the first analysis/ingest after a fresh install incurs the download under the mutex and **requires network**. CPU-only by design — no GPU/large-model RAM. |
| **Per-request image (base64)** | a few MB transient | Screenshot is pre-shrunk to ≤1568px long edge / ≤5 MB before base64 encoding for the vision call. |
| **Vector search working set** | grows with library | `LanceStore::search` loads **all** `doc_chunks` rows (each embedding = 384×4 bytes ≈ 1.5 KB) into memory per query. Linear in corpus size — the first thing to revisit (ANN index / batched scan) if the library gets large. |
| **SQLite connections** | small | Two single connections, WAL mode. |

**Design choices that respect the budget:**

- The large language model is **offloaded to Anthropic** (remote), not run locally — a 70B-class local model is out of the question at 8 GB. The vision model used for symbol extraction and image description is **Haiku** (cheap), with **Sonnet** as the default analysis model.
- Embeddings are computed by a **small CPU model, serialized behind one mutex**, so there is never more than one embedding workload in RAM at a time.
- A **daily cost cap** (`analyze_with_cap`, default `$2`) bounds spend before each main call. Model rates are matched by substring in `cost::rates_for` (`opus` / `haiku` / else → sonnet).

### App shell and storage layout

`lib.rs::run` creates `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` (`paths::Paths`), builds `AppState { db, lance, paths, llm }`, registers a **system tray** and a **`Ctrl+Shift+Space` global hotkey** to toggle the panel, and spawns the watcher + background loops. The frontend is a SvelteKit static SPA (`adapter-static`) with three tabs: **Chat / Library / Settings**.

```
%USERPROFILE%\CogniTrade\
├─ library\       # drop PDFs/images here -> auto-ingested (watcher)
├─ screenshots\   # save_screenshot writes <hint>-<uuid>.png
├─ data\
│   ├─ chat.db    # messages, predictions, cost_daily, settings (WAL)
│   └─ vec.db     # doc_chunks embedding store (WAL) — the "Lance" store
└─ logs\
```

> All paths under `%USERPROFILE%\CogniTrade\` are **user data — never commit them.**

---

## 7. Cross-references

- IPC command catalogue and types: [`src-tauri/src/ipc.rs`](../../src-tauri/src/ipc.rs), [`src/lib/ipc.ts`](../../src/lib/ipc.ts), [`src/lib/types.ts`](../../src/lib/types.ts)
- LLM streaming + prompt + JSON-tail parsing: [`src-tauri/src/llm/`](../../src-tauri/src/llm/)
- RAG store internals: [`src-tauri/src/lance.rs`](../../src-tauri/src/lance.rs), [`src-tauri/src/retrieval.rs`](../../src-tauri/src/retrieval.rs)
- Calibration loop: [`src-tauri/src/predictions.rs`](../../src-tauri/src/predictions.rs), [`src-tauri/src/prune.rs`](../../src-tauri/src/prune.rs)
- Aspirational design (diverges from code): [`docs/superpowers/`](../superpowers/), [`crypto Dhriti.txt`](../../crypto%20Dhriti.txt)

### Build & run (Windows primary)

```powershell
npm install                 # install JS deps (once)
npm run tauri dev           # dev with hot reload
npm run tauri build         # release MSI (WiX) / NSIS installer
npm run check               # svelte-check typecheck

cd src-tauri; cargo test    # backend unit tests
# integration tests are #[ignore]d; run explicitly with -- --ignored:
cargo test --test ingest_integration -- --ignored --nocapture  # downloads real BGE-small -> NEEDS NETWORK
cargo test --test llm_integration   -- --ignored                # uses wiremock mock server -> OFFLINE-capable
```

> The two `#[ignore]`d integration tests differ in their external dependencies:
> - `tests/ingest_integration.rs` exercises the full chunk → embed → store pipeline and **downloads the real BGE-small model on first run** — it needs network. This is the canonical full-pipeline smoke test documented in [`CLAUDE.md`](../../CLAUDE.md).
> - `tests/llm_integration.rs` stands up a **`wiremock`** mock Anthropic server, so it is **offline-capable** and is the one to prefer when there is no network.

Windows toolchain prerequisites: WebView2 runtime, Rust MSVC toolchain, and Visual Studio C++ Build Tools. The Anthropic API key is stored in Windows Credential Manager (keyring). The repository is **not** a git repository.
