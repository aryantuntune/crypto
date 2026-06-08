# CogniTrade — Product Requirements Document

> Status: **v0.1.0**, advisory app. Last revised 2026-06-09.
> Scope of this document: what the *implemented* CogniTrade app is, the requirements it meets, and which requirements from the original brief remain **Planned — not yet implemented**.
>
> Authoritative source for "what the code does": [`../../CLAUDE.md`](../../CLAUDE.md). Aspirational background: [`../../crypto Dhriti.txt`](../../crypto%20Dhriti.txt), [`../superpowers/specs/2026-05-05-cognitrade-design.md`](../superpowers/specs/2026-05-05-cognitrade-design.md), [`../superpowers/plans/2026-05-05-cognitrade-v1.md`](../superpowers/plans/2026-05-05-cognitrade-v1.md).

---

## 1. Problem statement

A solo crypto trader who trusts their own judgment for *placing* orders still wants a second opinion before pulling the trigger: a structured read of the chart in front of them, grounded in the trading literature they have already collected (PDFs of books, research notes, screenshots of setups), with an explicit, calibrated probability and a sensible stop-loss / take-profit band.

Cloud "AI trading" products either (a) demand custody/exchange API keys and execute autonomously — which the user does not want — or (b) run as web SaaS that ships the user's screenshots and private trading library to a third party. Neither fits a privacy-conscious trader running a *modest, always-on desktop PC* (the hardware target below) who wants the analysis to live locally, stay cheap, and never act without them.

**CogniTrade** is the answer: a local-first Tauri desktop app that reads a chart screenshot with Claude vision, retrieves relevant passages from the user's own ingested documents (RAG), and streams back a plain-English analysis ending in a structured recommendation. It **advises only**. The user places every trade themselves.

## 2. Target user

A single **technically literate solo crypto trader** who:

- Runs the app 24/7 on a constrained desktop (Ryzen 3400G class, **8 GB RAM**, 1 TB disk, 450 W PSU — see [§7](#7-non-functional-requirements)).
- Is comfortable supplying their own **Anthropic API key** and watching a daily spend cap.
- Has a personal library of trading PDFs / chart images they want the advisor to reason over.
- Wants suggestions, not automation — they keep their exchange credentials to themselves.
- Primarily on **Windows** (primary platform; data lives under `%USERPROFILE%\CogniTrade`).

## 3. Goals / non-goals

### Goals

- **G1.** Turn a chart screenshot into a clear, plain-English analysis ending in a machine-readable recommendation (`action`, `probability_up`, `horizon`, optional `stop_loss_pct` / `take_profit_pct`, `key_signals`, `citations`).
- **G2.** Ground that analysis in the user's *own* documents via retrieval (RAG over ingested PDFs and images), with citations surfaced in the recommendation.
- **G3.** Self-calibrate: record every recommendation as a *prediction* and later resolve it against real price so the user can judge the advisor's accuracy over time.
- **G4.** Stay cheap and predictable: a hard **daily USD cost cap** enforced before every paid call.
- **G5.** Stay local-first and private: the API key in the OS keyring, all user data on disk, no telemetry, no third-party SaaS.
- **G6.** Be ambient: a tray app togglable by a global hotkey, always-on, low idle footprint within the 8 GB budget.

### Non-goals (explicit)

- **NG1. No autonomous execution.** The app never places, modifies, or cancels an order. The system prompt states verbatim: *"You are an advisor. The user places all trades themselves."*
- **NG2. No exchange integration.** No Binance, no Kraken, no exchange API keys, no order book, no balances. (Named in the original spec; **Planned — not yet implemented**.)
- **NG3. No automated SL/TP enforcement.** SL/TP percentages are *advisory numbers in the recommendation only*; nothing in the app monitors price to trigger or enforce them.
- **NG4. No Minimax / game-tree decision engine** (named in the spec; **Planned — not yet implemented**).
- **NG5. No real local indicator computation.** RSI/MACD/Bollinger values are read by the vision model off the chart image; the app does not compute indicators from candle data.
- **NG6. Not a multi-user / server product.** Single user, single desktop.

## 4. Functional requirements — IMPLEMENTED

These are present in the codebase today. Each maps to source modules in the traceability table ([§9](#9-requirements-traceability)).

### FR-1 — Chart analysis (vision)
The single core command [`ipc::analyze`](../../src-tauri/src/ipc.rs) accepts `{ user_text, screenshot_path?, symbol_hint? }`. The screenshot is base64-encoded and sent as an image content block to the Anthropic Messages API (`model_main`, default `claude-sonnet-4-6`). The response **streams** to the frontend over the Tauri event `analysis_chunk`; completion fires `analysis_done`. Oversized screenshots are downscaled first (`image_util::needs_resize` / `resize_in_place`).

> **Caveat — image media type:** the image `media_type` is currently **hardcoded to `image/png`** (in both `extract_symbol_from_image` and `llm::prompt::build_messages`) with no format sniffing, so a JPEG/WebP screenshot is mislabeled to the Anthropic API.

### FR-2 — Symbol resolution
Resolved in priority order: explicit `symbol_hint` → else a tiny non-streaming **Haiku vision call** (`extract_symbol_from_image`, `model_extract` = `claude-haiku-4-5-20251001`) that reads the ticker off the image → else fallback `BTCUSDT`.

The fallback chain is more subtle than a clean cascade. `extract_symbol_from_image` returns `AppError::Invalid` on `UNKNOWN`/empty output or any HTTP error; the `BTCUSDT` default only happens because the caller swallows that error with `.unwrap_or_else(|_| "BTCUSDT".into())`. Crucially, image-based extraction first calls `settings::require_api_key()` — so **with no API key set and no `symbol_hint`, `analyze` errors with `NoApiKey` before any fallback is reached** (the `BTCUSDT` default applies only when there is *no screenshot at all*).

### FR-3 — RAG over the user's PDFs and images
Retrieval embeds the query `"<SYMBOL> chart pattern analysis"` and runs a **top-6 brute-force cosine** search over `doc_chunks` (`retrieval::search` → `lance::LanceStore::search`). Retrieved passages are injected as a cached text block in the user turn (`llm::prompt::build_messages`). The system prompt instructs the model to cite documents by name/page; citations appear in the structured recommendation.

> **Note on the vector store:** `lance.rs` / `LanceStore` / `paths::lancedb()` are **misnomers** — there is no LanceDB. It is plain **SQLite** (`data/vec.db`) storing embeddings as little-endian `f32` BLOBs; `search()` loads *every* row and computes cosine in Rust per query. Embedding dimension is fixed at **384** (`EMBED_DIM`), validated on insert.

### FR-4 — Document ingestion
- **PDF** (`ingest::ingest_pdf`): `pdf-extract` → `chunker::chunk_text` (~500 words, 50 overlap) → embed → delete-by-path then insert. No API key required.
- **Image** (`ingest::ingest_image`): a Haiku call describes the image, then the *description* is chunked / embedded. Requires an API key.
- **Auto-ingest watcher** (`ingest::watcher`): files dropped into the library dir are ingested via `notify` with an 800 ms debounce. PDFs ingest directly; images are skipped silently if no API key is set.
- Manual trigger: IPC `ingest_path` (returns the chunk count).

### FR-5 — Embeddings (local, CPU)
`ingest::embeddings` wraps **fastembed BGE-small (`BGESmallENV15`, 384-dim)** as a lazy global `OnceLock<Mutex<TextEmbedding>>` singleton. The model downloads on first use. Embedding is CPU-only and synchronous behind the mutex — a deliberate choice for the 8 GB RAM target.

### FR-6 — Structured probability / horizon recommendation
The model is instructed to end its prose with a fenced ` ```json ` block. `llm::parse::extract_json_tail` extracts the **last** such block and parses it into `AnalysisJson` (`llm::types`): `action` ∈ {buy, sell, hold}, `probability_up` ∈ [0,1] (validated), `horizon` ∈ {1h, 4h, 1d, 3d, 1w}, optional `stop_loss_pct` / `take_profit_pct`, `key_signals[]`, `citations[]`. **The recommendation is a JSON tail of free text — not tool-use.**

### FR-7 — Prediction self-calibration loop
Every analysis with a parsed recommendation persists a `predictions` row (`predictions::insert_from_analysis`) and records the current CoinGecko price fire-and-forget (`record_initial_price`). A background loop (`prune::spawn_background`, every **60 s**) finds predictions whose `ts + horizon_seconds` has passed and resolves them (`predictions::resolve_one`) by polling CoinGecko for the price *at horizon*. It records price-at-prediction vs price-at-horizon and sets `outcome_status` to `resolved` (or `failed` if the price fetch errors).

> It does **not** simulate intra-horizon SL/TP hits — only the endpoint price is compared.

### FR-8 — Live price lookup
`coingecko::get_usd_price` maps tickers via a hardcoded ~12-coin table (BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC), stripping `USDT`/`USD`/`USDC` suffixes, then hits CoinGecko's free `simple/price` endpoint. **Unlisted symbols error out.**

### FR-9 — Daily cost cap
`cost.rs` accumulates per-day token usage in `cost_daily` and prices it with hardcoded rates (`rates_for`: substring `opus`/`haiku`/else→sonnet; Sonnet 4.6 $3/$0.30/$15, Opus 4.7 $15/$1.50/$75, Haiku 4.5 $1/$0.10/$5 per Mtok input/cached/output). `analyze_with_cap` checks `settings.daily_cost_cap_usd` (default **$2**) **before** each main analysis call and returns `AppError::CostCapReached` if the day's spend is at or over cap (`is_over_cap`: `today_cost >= cap`). Prompt caching (`cache_control: ephemeral` on the system prompt and the retrieved-knowledge block) reduces repeat input cost.

> **Cap scope:** only the **main `model_main` analysis call** flows through `analyze_with_cap`. The auxiliary **Haiku `model_extract` calls** (symbol extraction in `analyze`, image description in `ingest_image`) are **not** cost-capped, so the day's spend can exceed the cap by the cost of those Haiku calls.

### FR-10 — Tray + global hotkey shell
`lib.rs::run` builds a frameless, always-on-top, taskbar-skipping window (`tauri.conf.json`), a **system tray** (`tray.rs`: Toggle Panel / Quit, left-click toggles), and a **`Ctrl+Shift+Space`** global hotkey that toggles the panel and re-anchors it to the active monitor's top-right.

### FR-11 — Settings & secret management
- Non-secret settings (`daily_cost_cap_usd`, `model_main`, `model_extract`, `library_path`, `hotkey`) live in the `settings` SQLite table (`settings::load` / `save`).
- The **Anthropic API key lives in the OS keyring** (`keyring` crate, service `cognitrade`, user `anthropic_api_key`) — **never in the DB**. On Windows this is the **Windows Credential Manager**.
- IPC: `get_settings`, `save_settings`, `get_api_key_set`, `set_api_key`, `clear_api_key`, `cost_today`, `save_screenshot`, `list_recent_messages`.

### FR-12 — Chat history persistence & pruning
User and assistant messages persist in `chat.db` (`messages`, with optional `image_path` and `prediction_id`). The chat panel loads recent history via the IPC command `list_recent_messages` (→ `chat_store::recent`, exposed as `api.listRecentMessages` in `src/lib/ipc.ts`); a background loop prunes messages older than **7 days** every 6 h (`chat_store::prune_older_than_secs`).

### FR-13 — Three-tab frontend
SvelteKit static SPA (Svelte 5, Tailwind 3, Vite 6, `adapter-static`) with tabs **Chat / Library / Settings** (`src/routes/+page.svelte`). `src/lib/ipc.ts` typed-wraps every `invoke`; `src/lib/stores/chat.ts` listens for `analysis_chunk` / `analysis_done`.

## 5. Functional requirements — PLANNED (not yet implemented)

These appear in the original brief / design docs but **do not exist in the code**. They must never be described as working. Their spec anchors live in the design and plan docs: exchange execution, the Minimax engine, and binding SL/TP enforcement / pre-trade approval are described in [`../superpowers/specs/2026-05-05-cognitrade-design.md`](../superpowers/specs/2026-05-05-cognitrade-design.md) and the phased rollout in [`../superpowers/plans/2026-05-05-cognitrade-v1.md`](../superpowers/plans/2026-05-05-cognitrade-v1.md).

| ID | Requirement (from spec) | Status | Notes |
|----|-------------------------|--------|-------|
| PL-1 | **Exchange execution** on Binance / Kraken (place/cancel orders, balances) | Planned — not yet implemented | No exchange SDK, no order code, no exchange keys anywhere. Conflicts with non-goal **NG1/NG2** until a human-approval execution layer is designed. |
| PL-2 | **Minimax decision engine** alongside RAG | Planned — not yet implemented | No game-tree/decision module exists. Recommendation comes solely from the LLM JSON tail. |
| PL-3 | **Automated SL/TP enforcement** (monitor price, trigger stop/target) | Planned — not yet implemented | SL/TP are advisory numbers only; the resolution loop compares endpoint price, never enforces. |
| PL-4 | **Real local indicator computation** (RSI/MACD/Bollinger from candle data) | Planned — not yet implemented | Indicators are read visually off the chart by the model; no numeric TA pipeline exists. |
| PL-5 | **Pre-trade human-approval workflow** as a binding gate before execution | Planned — not yet implemented | Moot while there is no execution path; the app is advisory end-to-end. |
| PL-6 | **Broader symbol coverage / live candlesticks** | Planned — not yet implemented | Only ~12 coins are mapped; only spot USD price (not OHLCV) is fetched. |
| PL-7 | **Approximate-nearest-neighbour vector index** (true LanceDB / ANN) | Planned — not yet implemented | Current store is brute-force cosine over all rows in SQLite; the obvious scaling bottleneck. |

## 6. Acceptance criteria

| # | Given / When / Then |
|---|---------------------|
| AC-1 | **Given** an API key is set and a chart screenshot, **when** the user submits via the Chat tab, **then** analysis text streams in via `analysis_chunk` and the turn ends with `analysis_done` carrying a parsed `AnalysisJson` (or `null` if the model omitted/garbled the JSON tail). |
| AC-2 | **Given** a parsed recommendation, **then** a `predictions` row is written with `outcome_status='pending'`, and `outcome_price_at_ts` is populated when CoinGecko resolves the symbol. |
| AC-3 | **Given** a pending prediction whose `ts + horizon_seconds` has elapsed, **when** the 60 s loop runs, **then** it becomes `resolved` (with `outcome_price_at_horizon`) or `failed` (price fetch error). |
| AC-4 | **Given** today's accumulated cost ≥ `daily_cost_cap_usd` (`is_over_cap` uses `>=`), **when** a new analysis is requested, **then** `analyze` returns `CostCapReached` and **no** paid **main** (`model_main`) call is made. (Auxiliary Haiku `model_extract` calls are not gated by the cap.) |
| AC-5 | **Given** a PDF dropped into the library dir, **then** within ~1 s it is chunked, embedded, and queryable; **given** an image with **no** API key set, **then** it is skipped silently (no crash). |
| AC-6 | **Given** retrieved chunks exist, **when** building the request, **then** a single cached retrieved-knowledge block precedes the image and user text, and `extract_json_tail` rejects any JSON with `probability_up` outside [0,1]. |
| AC-7 | **Given** the app is running, **then** `Ctrl+Shift+Space` (and tray left-click) toggle the panel; closing the panel does not exit the process. |
| AC-8 | **Given** an API key is saved, **then** it is stored in the OS keyring and **never** written to `chat.db`; `get_api_key_set` reflects presence without returning the secret. |
| AC-9 | **Given** an unmapped ticker (e.g. `XYZ`), **then** price lookup returns an `Invalid` error and prediction resolution marks the row `failed` rather than crashing the loop. |
| AC-10 | **Verification:** `cd src-tauri && cargo test` passes all unit tests; gated network/model integration tests (`--ignored`) pass when an API key and connectivity are available. |

## 7. Non-functional requirements

| ID | Requirement | How the implementation meets it |
|----|-------------|---------------------------------|
| NFR-1 | **8 GB RAM ceiling (hard constraint)** | CPU-only BGE-small embeddings behind a single mutex; no local LLM; SQLite stores; no in-memory ANN index. Heavy reasoning is offloaded to the Anthropic API, not run locally. |
| NFR-2 | **24/7 operation** | Tray-resident frameless window; background loops (prune 6 h, prediction resolution 60 s) run continuously; watcher auto-ingests. |
| NFR-3 | **Privacy / local-first** | All user data under `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}`; API key in OS keyring; no telemetry. The only outbound calls are to Anthropic and CoinGecko. |
| NFR-4 | **Cost ceiling** | Per-day cap enforced pre-call (FR-9); prompt caching on system + retrieved-knowledge blocks; cheap Haiku used for symbol extraction / image description. |
| NFR-5 | **Hardware target** | Ryzen 3400G, 8 GB RAM, 1 TB storage, 450 W PSU — drives model-size, concurrency, and embedding choices. |
| NFR-6 | **Windows-primary packaging** | `npm run tauri build` with `bundle.targets` set to `"all"` in `tauri.conf.json` (not an explicit msi/nsis list) — on Windows this yields both an **MSI (WiX)** and an **NSIS** installer; requires WebView2 runtime, Rust MSVC toolchain, and Visual Studio C++ Build Tools. |
| NFR-7 | **Resilience** | Single `AppError` enum (`error.rs`) serialized to the frontend; background loops swallow per-item errors and continue; hotkey registration tolerates "already registered" from a prior crash. |
| NFR-8 | **Data durability** | Both SQLite DBs (`chat.db`, `vec.db`) run in WAL mode. |

## 8. Architecture summary (implemented)

```
Frontend (SvelteKit SPA, src/)
  routes/+page.svelte  ── Chat / Library / Settings tabs
  lib/ipc.ts           ── typed invoke wrappers
  lib/stores/chat.ts   ── listens: analysis_chunk, analysis_done
        │  Tauri IPC + events
        ▼
Backend (Rust, src-tauri/src/)
  ipc.rs::analyze ──┬─ chat_store (persist user msg)
                    ├─ symbol: hint │ Haiku vision │ BTCUSDT
                    ├─ retrieval.search → lance (SQLite brute-force cosine)
                    ├─ llm::prompt::build_messages (history + cached RAG + image + text)
                    ├─ llm::client::analyze_with_cap → cost cap → Anthropic SSE stream
                    ├─ llm::parse::extract_json_tail → AnalysisJson
                    └─ predictions.insert + coingecko price
  prune::spawn_background ── chat prune (6h) + prediction resolution (60s)
  ingest::watcher        ── auto-ingest library dir (PDF/image)
  keyring                ── Anthropic API key (Windows Credential Manager)

Data: %USERPROFILE%\CogniTrade\{library, screenshots, data\{chat.db, vec.db}, logs}
```

## 9. Requirements traceability

| Req | Source module(s) / file(s) | Key symbols |
|-----|----------------------------|-------------|
| FR-1 Chart analysis | [`src-tauri/src/ipc.rs`](../../src-tauri/src/ipc.rs), [`llm/client.rs`](../../src-tauri/src/llm/client.rs), [`llm/streaming.rs`](../../src-tauri/src/llm/streaming.rs), [`image_util.rs`](../../src-tauri/src/image_util.rs) | `analyze`, `LlmClient::analyze`, `analysis_chunk`/`analysis_done` events, `needs_resize` |
| FR-2 Symbol resolution | [`ipc.rs`](../../src-tauri/src/ipc.rs) | `extract_symbol_from_image`, `model_extract` |
| FR-3 RAG retrieval | [`retrieval.rs`](../../src-tauri/src/retrieval.rs), [`lance.rs`](../../src-tauri/src/lance.rs), [`llm/prompt.rs`](../../src-tauri/src/llm/prompt.rs) | `search`, `LanceStore::search`, `build_messages` |
| FR-4 Ingestion | [`ingest/mod.rs`](../../src-tauri/src/ingest/mod.rs), [`ingest/pdf.rs`](../../src-tauri/src/ingest/pdf.rs), [`ingest/image.rs`](../../src-tauri/src/ingest/image.rs), [`ingest/chunker.rs`](../../src-tauri/src/ingest/chunker.rs), [`ingest/watcher.rs`](../../src-tauri/src/ingest/watcher.rs) | `ingest_pdf`, `ingest_image`, `chunk_text`, `run_watcher`, `ingest_path` |
| FR-5 Embeddings | [`ingest/embeddings.rs`](../../src-tauri/src/ingest/embeddings.rs) | `embed`, `EMBED_DIM`, `BGESmallENV15` |
| FR-6 Structured recommendation | [`llm/parse.rs`](../../src-tauri/src/llm/parse.rs), [`llm/types.rs`](../../src-tauri/src/llm/types.rs), [`llm/prompt.rs`](../../src-tauri/src/llm/prompt.rs) | `extract_json_tail`, `AnalysisJson`, `SYSTEM_PROMPT` |
| FR-7 Prediction calibration | [`predictions.rs`](../../src-tauri/src/predictions.rs), [`prune.rs`](../../src-tauri/src/prune.rs) | `insert_from_analysis`, `pending_due`, `resolve_one`, `spawn_background` |
| FR-8 Price lookup | [`coingecko.rs`](../../src-tauri/src/coingecko.rs) | `symbol_to_id`, `get_usd_price` |
| FR-9 Cost cap | [`cost.rs`](../../src-tauri/src/cost.rs), [`llm/client.rs`](../../src-tauri/src/llm/client.rs) | `rates_for`, `add_usage`, `is_over_cap`, `analyze_with_cap` |
| FR-10 Tray + hotkey | [`lib.rs`](../../src-tauri/src/lib.rs), [`tray.rs`](../../src-tauri/src/tray.rs), [`tauri.conf.json`](../../src-tauri/tauri.conf.json) | `init_tray`, `toggle_panel`, `Ctrl+Shift+Space` |
| FR-11 Settings & secrets | [`settings.rs`](../../src-tauri/src/settings.rs), [`ipc.rs`](../../src-tauri/src/ipc.rs) | `Settings`, `get_api_key`/`set_api_key` (keyring), IPC commands |
| FR-12 History & pruning | [`chat_store.rs`](../../src-tauri/src/chat_store.rs), [`ipc.rs`](../../src-tauri/src/ipc.rs), [`migrations/001_initial.sql`](../../src-tauri/migrations/001_initial.sql) | `insert`, `recent`, `list_recent_messages` (IPC), `prune_older_than_secs`, `messages` table |
| FR-13 Frontend tabs | [`src/routes/+page.svelte`](../../src/routes/+page.svelte), [`src/lib/ipc.ts`](../../src/lib/ipc.ts), [`src/lib/stores/chat.ts`](../../src/lib/stores/chat.ts) | tab shell, `api`, event listeners |
| NFR-1/5 RAM & hardware | [`embeddings.rs`](../../src-tauri/src/ingest/embeddings.rs), [`lance.rs`](../../src-tauri/src/lance.rs) | CPU singleton, SQLite store |
| NFR-3 Privacy | [`paths.rs`](../../src-tauri/src/paths.rs), [`settings.rs`](../../src-tauri/src/settings.rs) | `Paths`, keyring |
| NFR-4 Cost ceiling | [`cost.rs`](../../src-tauri/src/cost.rs) | `analyze_with_cap` |
| NFR-7 Resilience | [`error.rs`](../../src-tauri/src/error.rs), [`prune.rs`](../../src-tauri/src/prune.rs) | `AppError`, loop error-swallowing |
| PL-1…PL-7 Planned | — (no module) | See [§5](#5-functional-requirements--planned-not-yet-implemented) |

## 10. Build & verify (Windows)

```powershell
npm install                    # JS deps (run once)
npm run tauri dev              # dev with hot reload
npm run check                  # svelte-check typecheck
cd src-tauri ; cargo test      # backend unit tests
# gated network/model tests:
cargo test --test ingest_integration -- --ignored --nocapture   # downloads BGE-small on first run
cargo test --test llm_integration   -- --ignored                # needs a live API key
npm run tauri build            # bundle.targets="all" → on Windows: MSI (WiX) + NSIS installer
```

User data (never commit): `%USERPROFILE%\CogniTrade\{library, screenshots, data, logs}`. The repo is **not** a git repository.

---

### Open questions / future PRD revisions
- If exchange execution (PL-1) is ever pursued, the binding **pre-trade human-approval gate** (PL-5) and **SL/TP enforcement** (PL-3) become hard requirements, not options — see the original brief.
- Vector search (PL-7) will need an ANN index before the library grows large, given the current full-scan cosine.
