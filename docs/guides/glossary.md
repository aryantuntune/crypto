# Glossary

A reference for the domain, AI/RAG, and codebase terms used across CogniTrade. Terms are grouped by category. Where a term names something the original spec describes but the code does **not** implement, it is labelled **Planned — not yet implemented**.

> Scope reminder: CogniTrade is an **advisory** crypto-chart analyst, not an autonomous trader. The system prompt is explicit — *"You are an advisor. The user places all trades themselves."* There is no exchange integration, no order execution, and no automated stop-loss/take-profit enforcement. See [Advisory vs. autonomous](#advisory-vs-autonomous).

Related docs:
- [../architecture/data-model.md](../architecture/data-model.md) — table schemas (`messages`, `predictions`, `cost_daily`, `settings`, `doc_chunks`)
- [../architecture/ipc-api-reference.md](../architecture/ipc-api-reference.md) — the `analyze` / `ingest_path` IPC commands
- [../architecture/tech-stack.md](../architecture/tech-stack.md) — Tauri / Rust / Svelte stack
- [../project/vision-and-scope.md](../project/vision-and-scope.md) — what is in and out of scope

---

## Trading terms

These describe what the user reads off a chart and what the model is asked to reason about. **Important:** CogniTrade does not compute any of these indicators itself. It sends the chart **screenshot** to Claude (vision), and the model reads whatever indicators are already drawn on the image. There is no numerical indicator engine in the codebase.

| Term | Definition | Where it appears |
| --- | --- | --- |
| **RSI** (Relative Strength Index) | A momentum oscillator (0–100) measuring the speed/magnitude of recent price moves; readings above ~70 suggest overbought, below ~30 oversold. Read visually from the chart by the model. | Model reasoning; the system prompt asks it to "identify … key indicators". |
| **MACD** (Moving Average Convergence Divergence) | A trend/momentum indicator built from the difference of two EMAs plus a signal line; crossovers and histogram flips are common signals. Read visually from the chart. | Model reasoning. |
| **Bollinger Bands** | A volatility envelope: a moving average with bands at ±N standard deviations. Price touching/exceeding a band hints at over-extension or breakout. Read visually from the chart. | Model reasoning. |
| **Candlestick** | The OHLC (open/high/low/close) bar shown for each timeframe interval on the chart. The model reads candlestick patterns from the screenshot. | Chart image sent to the model. |
| **Stop-loss (SL)** | A protective exit level below entry (for a long) that caps downside. In CogniTrade it is expressed as a **percentage** (`stop_loss_pct`) inside the model's recommendation. **Advisory only** — the app never places or enforces a stop. | `AnalysisJson.stop_loss_pct`; `predictions.stop_loss_pct`. |
| **Take-profit (TP)** | A target exit level above entry that locks in gains. Expressed as a percentage (`take_profit_pct`). **Advisory only** — never enforced. | `AnalysisJson.take_profit_pct`; `predictions.take_profit_pct`. |
| **Limit price** | The price at which a limit order would execute. The spec lists it as part of a trade suggestion, but the **implemented** `AnalysisJson` schema does not include a limit-price field, and no orders are placed. | **Planned — not yet implemented** as a structured field. |
| **Horizon** | The forward time window over which the model's directional call is meant to hold, and the window after which a prediction is later self-scored. One of `1h`, `4h`, `1d`, `3d`, `1w`. Converted to seconds by `Horizon::seconds()` (e.g. `4h` → 14 400 s). | `AnalysisJson.horizon`; `predictions.horizon_seconds`. |

### Automated SL/TP enforcement — Planned, not implemented

The spec calls for "hard SL/TP enforcement." The code stores `stop_loss_pct` / `take_profit_pct` as advisory numbers only. The prediction-resolution loop (`predictions::resolve_one`) records the **current CoinGecko spot price at the time the prediction becomes due** (shortly after `ts + horizon_seconds`, since the resolution loop only fires every 60 s) into `outcome_price_at_horizon`, versus `outcome_price_at_ts` captured at prediction time. It does **not** simulate whether SL or TP would have been hit intra-horizon. **Planned — not yet implemented.**

---

## RAG / AI terms

RAG = Retrieval-Augmented Generation: before asking the model to analyse the chart, CogniTrade retrieves relevant passages from the user's ingested documents and injects them into the prompt so the answer is grounded in (and can cite) that material.

| Term | Definition | Where it appears |
| --- | --- | --- |
| **Embedding** | A fixed-length vector that represents the meaning of a text chunk so that similar texts land near each other. CogniTrade embeddings are **384-dimensional** (`EMBED_DIM = 384`). The raw `Vec<f32>` is produced by `ingest::embeddings::embed`, then serialized to a little-endian `f32` BLOB by `lance.rs::embedding_to_bytes` (decoded by `bytes_to_embedding`) for storage in `doc_chunks.embedding`. The length is validated against `EMBED_DIM` on insert. | `ingest::embeddings::embed`; `lance.rs::embedding_to_bytes`. |
| **Chunk** | A slice of a source document, sized for embedding. `chunker::chunk_text(text, target_tokens, overlap_tokens)` is called as `chunk_text(full_text, 500, 50)`, producing ~**500-token** chunks with **50-token overlap**. Tokens are approximated as **4 chars/token**, so chunks are ~**2000 chars** with ~200-char overlap, backed up to a sentence boundary (`.`/`!`/`?`/newline) where possible. Each chunk becomes one `doc_chunks` row. | `ingest::chunker::chunk_text`. |
| **Cosine similarity** | The similarity score (dot product of normalized vectors) used to rank chunks against the **query embedding**. Note the query that gets embedded during `analyze` is the derived string `"{SYMBOL} chart pattern analysis"`, not the user's raw text. Computed in Rust by the `cosine()` function. Search loads **every** chunk row and scores it on each query (brute force), returning the top `k = 6`. | `lance.rs::cosine`, `LanceStore::search`. |
| **BGE-small** | The embedding model — `BGESmallENV15` from the `fastembed` crate (BAAI BGE-small-en-v1.5, 384-dim, CPU-only). Loaded lazily as a global `OnceLock<Mutex<TextEmbedding>>` singleton; the model files **download on first use**. CPU-only and synchronous behind a mutex — a deliberate fit for the 8 GB-RAM target. | `ingest::embeddings`. |
| **Prompt caching** | Anthropic's ephemeral cache, marked via `cache_control: {"type":"ephemeral"}`. CogniTrade caches the **retrieved-knowledge block** (the joined chunks) and the **system prompt**, so repeated/large context is billed at the cheaper cached-input rate. | `llm::prompt` (`CacheControl::ephemeral`, `build_messages`); `cost::rates_for` cached rate. |
| **JSON tail** | The convention that the model ends its free-text reply with a single fenced <code>```json</code> block matching the `AnalysisJson` schema. `extract_json_tail` finds the **last** such fence, parses it, and validates it. **This is plain text parsing, not Anthropic tool-use.** If no valid block is found, the recommendation is simply absent. | `llm::parse::extract_json_tail`. |

### Probabilistic / historical pattern matching — partially implemented

The spec promises statements like "based on the last 100 similar patterns, price has a 72% chance…". In the code, `probability_up` is a single float the **model** supplies in the JSON tail; it is not derived from a database of historical pattern matches. The only empirical loop is prediction self-scoring (see [prediction](#prediction)). A true historical-pattern probability engine is **Planned — not yet implemented**.

---

## Codebase terms

| Term | Definition | Where it lives |
| --- | --- | --- |
| **`AnalysisJson`** | The Rust struct (and matching TS interface) for the model's structured recommendation: `action` (buy/sell/hold), `probability_up` (0–1), `horizon`, optional `stop_loss_pct` / `take_profit_pct`, `key_signals[]`, `citations[]`. `stop_loss_pct` / `take_profit_pct` are `Option<f32>` (`#[serde(default, skip_serializing_if = "Option::is_none")]` — serialized only when present); `key_signals` / `citations` are `#[serde(default)]` and default to empty arrays. `validate()` only range-checks `probability_up` (rejects values outside `[0,1]`), so an analysis with no SL/TP still parses, even though `SYSTEM_PROMPT` always asks for them. | `src-tauri/src/llm/types.rs`; `src/lib/types.ts`. |
| **prediction** | A persisted self-evaluation record written from each `AnalysisJson` (`predictions` table). Fields include `symbol`, `horizon_seconds`, `predicted_prob_up`, `recommended_action`, SL/TP pcts, `outcome_status` (`pending` → `resolved`/`failed`), and the recorded prices. A background loop resolves due predictions by polling CoinGecko. This is the app's calibration/self-learning loop — **not** a trade order. | `src-tauri/src/predictions.rs`; `prune::spawn_background`. |
| **`LanceStore`** | The vector store type — **a misnomer.** Despite the name and `paths::lancedb()`, there is **no LanceDB dependency**; it is plain **SQLite** holding embeddings as `f32` BLOBs, with brute-force cosine search in Rust. Lives in its own DB file (`data/vec.db`). | `src-tauri/src/lance.rs`. |
| **`doc_chunks`** | The single table in the vector DB (`data/vec.db`): `id`, `doc_path`, `doc_type`, `chunk_index`, `text`, `page`, `embedding` (BLOB). One row per embedded chunk. Re-ingesting a document deletes its old rows by `doc_path` then re-inserts. | `lance.rs` SCHEMA; `LanceStore::delete_by_doc_path`/`insert`. |
| **cost cap** | The daily USD spend ceiling, `settings.daily_cost_cap_usd` (default **$2.00**). `analyze_with_cap` checks today's accumulated cost **before** each main analysis call and returns `AppError::CostCapReached` if the cap is met or exceeded. Per-day usage is tracked in `cost_daily`; prices come from `cost::rates_for` (substring match: `opus` / `haiku` / else → `sonnet`). | `src-tauri/src/cost.rs`; `llm::client::analyze_with_cap`. |
| **`AppState`** | The shared Tauri application state built in `lib.rs::run` and injected into every IPC command: `db` (chat DB handle), `lance` (`Arc<Mutex<Option<LanceStore>>>`, opened lazily), `paths` (the `~/CogniTrade` directory layout), and `llm` (the Anthropic client). | `src-tauri/src/lib.rs`. |

### The two databases

CogniTrade opens **two** separate SQLite databases (both in WAL mode), under `%USERPROFILE%\CogniTrade\data\`:

| File | Opened by | Tables |
| --- | --- | --- |
| `data\chat.db` | `db::open` (schema `migrations/001_initial.sql`) | `messages`, `predictions`, `cost_daily`, `settings` |
| `data\vec.db` | `lance::open` | `doc_chunks` |

### Other codebase names you'll meet

| Term | Meaning |
| --- | --- |
| **`analyze`** | The single core IPC command (`ipc.rs`). Flow: persist user message → resolve symbol → RAG retrieve → build Anthropic request → stream SSE (emitting `analysis_chunk` events) → parse JSON tail → store prediction + price → emit `analysis_done`. RAG retrieval uses a **fixed `k = 6`** (`retrieval::search(&store, &q, 6)`, `ipc.rs` line 171). Note the embedded query is **not** the user's free text but a synthesized string `"{SYMBOL} chart pattern analysis"` (`ipc.rs` line 170). |
| **`analysis_chunk` / `analysis_done`** | Tauri events the backend emits during/after `analyze`. The frontend chat store (`src/lib/stores/chat.ts`) listens for both to stream text and finalize the recommendation. |
| **`extract_symbol_from_image`** | The Haiku vision call that infers the trading symbol from the screenshot when the user gives no hint; falls back to `BTCUSDT`. |
| **`model_main` / `model_extract`** | Settings: the analysis model (default `claude-sonnet-4-6`) and the small vision/extraction model (`claude-haiku-4-5-20251001`). |
| **keyring** | The Anthropic API key is stored in the **OS keyring** (service `cognitrade`, user `anthropic_api_key`) — on Windows this is **Windows Credential Manager**. It is **never** written to the database. |
| **`AppError`** | The single error enum (`error.rs`) returned by IPC commands and serialized to the frontend. Notable variants: `Anthropic`, `Invalid`, `Keyring`, `NoApiKey`, `CostCapReached`, `Internal`. |
| **watcher** | `ingest::watcher` — auto-ingests files dropped into the library folder via `notify` (800 ms debounce). PDFs ingest with no API key; image ingest needs a key and is skipped silently if unset. |

---

## Project terms

### CogniTrade

The product name for this app: a Tauri 2 desktop application (Rust backend + SvelteKit/TypeScript frontend) that acts as a personal, **advisory** crypto-chart analyst. The user pastes or drops a chart screenshot; the backend reads it with Claude vision, retrieves relevant passages from ingested PDFs/images (RAG), and streams back a plain-English analysis ending in the structured `AnalysisJson` recommendation. The name also appears as the keyring service (`cognitrade`), the user-data directory (`%USERPROFILE%\CogniTrade`), and the opening word of the system prompt.

### Advisory vs. autonomous

This distinction is the single most important thing to keep straight about CogniTrade.

- **Advisory (what is built):** The app *suggests*. It produces an analysis and a recommendation (`buy`/`sell`/`hold` with a probability, horizon, and advisory SL/TP percentages). The human reads it and decides. The system prompt states: *"You are an advisor. The user places all trades themselves."*
- **Autonomous (what the original spec imagined — NOT built):** The spec (`crypto Dhriti.txt`, and `docs/superpowers/`) describes an autonomous trading bot with exchange execution (Binance/Kraken), a Minimax decision engine, position sizing, and hard SL/TP enforcement.

**None of the autonomous capabilities exist in the codebase.** There is no exchange integration, no order execution, no Minimax engine, and no automated SL/TP enforcement. Treat every autonomous feature as **Planned — not yet implemented** background, not a description of current behaviour. See [../project/vision-and-scope.md](../project/vision-and-scope.md) and the [trading terms](#trading-terms) notes above.
