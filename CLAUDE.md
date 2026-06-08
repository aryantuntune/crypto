# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

**CogniTrade** — a Tauri 2 desktop app (Rust backend + SvelteKit/TypeScript frontend) that acts as an **advisory** crypto-chart analyst. The user pastes/drops a chart screenshot; the backend reads it with Claude (vision), retrieves relevant passages from ingested PDFs/images (RAG), and streams back a plain-English analysis ending in a structured JSON recommendation (action, probability, horizon, SL/TP).

The original product spec (`crypto Dhriti.txt`) and design/plan under `docs/superpowers/` describe a far more ambitious autonomous trading bot (exchange execution on Binance/Kraken, a Minimax decision engine, hard SL/TP enforcement). **The implemented app does none of that** — there is no exchange integration, no order execution, and no Minimax engine. The system prompt is explicit: *"You are an advisor. The user places all trades themselves."* Don't go hunting for trade-execution code; it doesn't exist. Treat the spec as aspirational background, not a description of the codebase.

## Commands

- `npm install` — install JS deps (run once)
- `npm run tauri dev` — run the app (dev) with hot reload
- `npm run tauri build` — produce a release installer (per `tauri.conf.json` target)
- `npm run check` — Svelte/TypeScript typecheck (`svelte-check`)
- `cd src-tauri && cargo test` — run all backend Rust unit tests
- `cd src-tauri && cargo test <module>::tests` — run one module's tests (e.g. `cost::tests`)
- Integration tests in `src-tauri/tests/` are gated and hit the network/model — run explicitly:
  - `cargo test --test ingest_integration -- --ignored --nocapture` (downloads BGE-small embedding model on first run)
  - `cargo test --test llm_integration -- --ignored` (needs a live API key)

## Architecture (the parts that span files)

### Request flow: `analyze`
The whole app is essentially one IPC command, `ipc::analyze` (`src-tauri/src/ipc.rs`):
1. Persist the user message (`chat_store`).
2. Resolve the trading **symbol**: user hint → else extract from the screenshot via a tiny Haiku vision call (`extract_symbol_from_image`) → else fall back to `BTCUSDT`.
3. RAG: embed a query, brute-force cosine search over `doc_chunks` (`retrieval::search` → `lance::LanceStore::search`), take top-k chunk texts.
4. Build the Anthropic request (`llm::prompt::build_messages`): chat history, then a user turn = retrieved-knowledge block (cached) + image + user text. The system prompt is also `cache_control: ephemeral`.
5. Stream the response (`llm::client::analyze` → SSE parser in `llm::streaming`). Text deltas are forwarded to the frontend over the Tauri event `analysis_chunk`.
6. Parse the trailing fenced ```json block (`llm::parse::extract_json_tail`) into `AnalysisJson`. **The recommendation is a JSON tail of free text, not tool-use.**
7. Persist a `prediction` row from the analysis and record the current price (CoinGecko, fire-and-forget). Persist the assistant message. Emit `analysis_done` with the parsed analysis.

Frontend wiring lives in `src/lib/stores/chat.ts` — it `listen`s for `analysis_chunk`/`analysis_done` and drives the Svelte stores; `src/lib/ipc.ts` is the typed wrapper over every `invoke`.

### "Predictions" = self-evaluation / calibration loop
Each analysis stores a `prediction` (symbol, horizon, predicted_prob_up, SL/TP, `outcome_status='pending'`). A background loop (`prune::spawn_background`, every 60s) finds predictions whose `ts + horizon_seconds` has passed and resolves them by polling CoinGecko price (`predictions::resolve_one`). This is the "probabilistic reasoning" mechanism — it records price-at-horizon vs price-at-prediction; it does **not** simulate SL/TP hits intra-horizon.

### Two SQLite databases, opened separately
- `data/chat.db` (`db::open`, schema in `migrations/001_initial.sql`): `messages`, `predictions`, `cost_daily`, `settings`.
- `data/vec.db` (`lance::open`): the vector store, single table `doc_chunks`.

### The "Lance" store is not LanceDB
`lance.rs`, `LanceStore`, and `paths::lancedb()` are **misnomers** — there is no LanceDB dependency. It's plain SQLite storing embeddings as little-endian `f32` BLOBs, and `search` loads *every* row and computes cosine similarity in Rust on each query. Fine at small scale; this is the obvious bottleneck if the library grows. Embedding dim is fixed at 384 (`EMBED_DIM`); inserts validate against it.

### Embeddings
`ingest::embeddings` wraps `fastembed` BGE-small (384-dim) as a lazily-initialized global singleton (`OnceLock<Mutex<TextEmbedding>>`). First use downloads the model. All embedding is CPU and synchronous behind the mutex — deliberate, given the 8GB-RAM hardware target.

### Document ingestion
- `ingest::ingest_pdf` — `pdf-extract` → `chunker::chunk_text` (≈500-word chunks, 50 overlap) → embed → replace-by-`doc_path` then insert.
- `ingest::ingest_image` — Haiku describes the image → chunk/embed the description.
- `ingest::watcher` auto-ingests files dropped into the library dir (`notify`, 800ms debounce). PDFs ingest directly; images need an API key (skipped silently if unset). Started from `lib.rs` setup.

### Cost control
`cost.rs` tracks per-day token usage in `cost_daily` and prices it with hardcoded per-model rates (`rates_for` matches on substrings `opus`/`haiku`/else→sonnet). `analyze_with_cap` checks the daily USD cap (`settings.daily_cost_cap_usd`, default $2) **before** each main call and returns `CostCapReached` if exceeded. If you change model IDs, update `rates_for`.

### Models & secrets
- `settings.model_main` (default `claude-sonnet-4-6`) — the analysis call. `settings.model_extract` (`claude-haiku-4-5-20251001`) — vision symbol extraction and image description.
- The Anthropic API key lives in the **OS keyring** (`keyring` crate, service `cognitrade`), never in the DB. Settings (non-secret) live in the `settings` table.

### App shell
`lib.rs::run` builds `AppState` (both DBs, an `Arc<Mutex<Option<LanceStore>>>` opened lazily, `Paths`, `LlmClient`), creates `~/CogniTrade/{library,screenshots,data,logs}`, spawns the watcher + background loops, sets up the system tray and a `Ctrl+Shift+Space` global hotkey to toggle the panel. The frontend is a SvelteKit static SPA (`adapter-static`) with three tabs: Chat / Library / Settings (`src/routes/+page.svelte`).

### Symbol → price mapping
`coingecko.rs` maps tickers to CoinGecko ids via a **hardcoded ~12-coin table** (`symbol_to_id`), stripping `USDT`/`USD`/`USDC` suffixes. Unlisted symbols error out — extend the table to add coins.

## Where things live

- Rust backend: `src-tauri/src/` (modules registered in `lib.rs`); SQL in `src-tauri/migrations/`; IPC capabilities in `src-tauri/capabilities/`.
- Frontend: `src/routes/` (pages), `src/lib/` (`ipc.ts`, `types.ts`, `stores/`, `components/`).
- Design intent: `docs/superpowers/specs/` and `docs/superpowers/plans/` (the plan is organized by Phase 0–5, matching the module build order). Original brief: `crypto Dhriti.txt`.
- User data (NEVER commit): `~/CogniTrade/{library,screenshots,data,logs}` (`%USERPROFILE%\CogniTrade\…` on Windows).

## Conventions

- Errors: one `AppError` enum (`error.rs`) with a `Result<T>` alias; IPC commands return it directly (serialized to the frontend). Add variants there rather than stringly-typing.
- Each backend module keeps its own `#[cfg(test)]` unit tests inline (see `cost.rs`, `predictions.rs`, `paths.rs`). Follow that pattern; use `tempfile` for DB-backed tests and `wiremock` to stub Anthropic/CoinGecko HTTP.
- This is **not** a git repository. If asked to commit, offer to `git init` first.
- When touching the model: this app uses the Claude API. Verify model IDs / pricing against the current API rather than memory before changing `settings.rs` defaults or `cost::rates_for`.
