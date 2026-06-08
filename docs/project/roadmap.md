# CogniTrade — Roadmap & Milestones

> **Status legend** used throughout this document:
> - **Implemented** — the code exists today and runs.
> - **Planned — not yet implemented** — described in the spec (`crypto Dhriti.txt`) or the design docs under [`../superpowers/`](../superpowers/), but **no code exists**. Do not treat these as shipped features.

This roadmap lays out the path from the **current advisory app** to a polished, signed Windows release (M1–M3), and then — only if the project chooses to pursue it — toward a **real-execution** capability that is still **human-approved per trade** (M4). Note that even M4 as scoped is *not* lights-out autonomous: it mandates mandatory pre-trade approval and enforced SL/TP, so it is semi-autonomous at most. The original spec's fully autonomous, 24/7-trading vision and its **Minimax engine** remain explicitly **out of scope** of this roadmap. Every claim below is grounded in the actual source under `src-tauri/src/` and `src/`.

Cross-references:
- Engineering reality / architecture: [`../../CLAUDE.md`](../../CLAUDE.md)
- Aspirational design doc: [`../superpowers/specs/2026-05-05-cognitrade-design.md`](../superpowers/specs/2026-05-05-cognitrade-design.md)
- Aspirational build plan (Phase 0–5): [`../superpowers/plans/2026-05-05-cognitrade-v1.md`](../superpowers/plans/2026-05-05-cognitrade-v1.md)

---

## 1. Current state summary

CogniTrade today is a **Tauri 2 desktop app** — a Rust backend (`src-tauri/`) plus a SvelteKit/TypeScript static SPA frontend (`src/`, Svelte 5, Tailwind 3, Vite 6, `adapter-static`). It is an **advisory crypto-chart analyst**, not an autonomous trader. The system prompt (`llm/prompt.rs`, `SYSTEM_PROMPT`) is explicit:

> *"You are an advisor. The user places all trades themselves."*

### What works today (Implemented)

- **Single-command analysis pipeline** (`ipc::analyze`): persist the user message → resolve the symbol (user hint → else Haiku vision extraction from the screenshot via `extract_symbol_from_image` → else fall back to `BTCUSDT`) → RAG retrieval → build the Anthropic request (`llm::prompt::build_messages`) → stream SSE deltas to the frontend via the Tauri event `analysis_chunk` → parse the trailing fenced ```json block (`llm::parse::extract_json_tail`) into `AnalysisJson` → store a prediction row + record current price → store the assistant message → emit `analysis_done`. **The recommendation is a JSON tail of free text, not tool-use.**
- **RAG over user documents**: PDF ingest (`ingest::ingest_pdf` — `pdf-extract` → `chunker::chunk_text` ≈500-token/50-overlap → embed → replace-by-`doc_path`) and image ingest (`ingest::ingest_image` — Haiku describes the image, then chunk/embed the description). A `notify`-based watcher (`ingest::watcher`, 800ms debounce) auto-ingests files dropped into `%USERPROFILE%\CogniTrade\library`.
- **Embeddings**: `fastembed` BGE-small (`BGESmallENV15`), 384-dim (`EMBED_DIM`), a lazy global `OnceLock<Mutex<TextEmbedding>>` singleton, CPU-only and synchronous behind the mutex (deliberate for the 8GB-RAM target). Model downloads on first use.
- **Vector store** (`lance.rs` / `LanceStore`): **plain SQLite**, not LanceDB — embeddings stored as little-endian `f32` BLOBs in `doc_chunks`; `search()` loads **every** row and computes cosine similarity in Rust on each query.
- **Self-calibration loop** ("predictions"): each analysis writes a `predictions` row (symbol, horizon, `predicted_prob_up`, SL/TP, `outcome_status='pending'`). The current `outcome_status` enum has exactly three states: `pending` (initial; default in `migrations/001_initial.sql`), `resolved` (price successfully fetched at the horizon), and `failed` (the CoinGecko price fetch errored). `prune::spawn_background` runs two loops — chat prune (rows older than 7d, every 6h) and prediction resolution (every 60s) via `predictions::resolve_one`, which polls CoinGecko for the price at `ts + horizon_seconds`. It records price-at-horizon vs price-at-prediction; it **does not simulate intra-horizon SL/TP hits**. Horizons supported: 1h / 4h / 1d / 3d / 1w.
- **Cost control** (`cost.rs`): per-day token usage in `cost_daily`, priced by `rates_for` (substring match `opus`/`haiku`/else→sonnet). The rates are **hardcoded to specific model versions** (Sonnet 4.6 $3/$0.30/$15, Opus 4.7 $15/$1.50/$75, Haiku 4.5 $1/$0.10/$5 per Mtok input/cached/output). `analyze_with_cap` enforces `settings.daily_cost_cap_usd` (default **$2**) **before** each main call, returning `AppError::CostCapReached`.
- **Secrets & settings**: Anthropic API key in the OS keyring (`keyring` crate, service `cognitrade`, user `anthropic_api_key`) → Windows Credential Manager; non-secret settings in the `settings` table. The settings-module round-trip functions are `settings::set_api_key` / `settings::get_api_key` (returns `Option<String>`) / `settings::clear_api_key`; the IPC commands the UI actually calls are `set_api_key`, `get_api_key_set` (returns `bool`, *not* the key), and `clear_api_key` in `ipc.rs`, which delegate to the settings-module functions.
- **App shell** (`lib.rs::run`): builds `AppState { db, lance, paths, llm }`, creates `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}`, spawns the watcher + background loops, installs a system tray and a global hotkey to toggle the panel. **The hotkey is hardcoded** in `lib.rs::run` — `Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)` (`Ctrl+Shift+Space`) — and the persisted `settings.hotkey` field is **never read** (it is dead/unwired; changing it has no effect). Frontend is a 3-tab shell — **Chat / Library / Settings** (`src/routes/+page.svelte`); `src/lib/stores/chat.ts` listens for `analysis_chunk`/`analysis_done`.
- **Two SQLite DBs** (WAL): `data/chat.db` (`messages`, `predictions`, `cost_daily`, `settings`; `migrations/001_initial.sql`) and `data/vec.db` (`doc_chunks`).
- **Models**: `settings.model_main` default `claude-sonnet-4-6` (analysis); `settings.model_extract` `claude-haiku-4-5-20251001` (vision symbol extraction + image description).

### What does NOT exist today (Planned — not yet implemented)

- **No exchange integration** (Binance / Kraken), **no order execution**, **no automated trade placement**.
- **No automated SL/TP enforcement** — SL/TP are advisory percentages inside the JSON recommendation only.
- **No Minimax decision engine** (named in the spec; not present in code).
- **No real technical-indicator computation** — RSI / MACD / Bollinger Bands are *read off the chart by the model*; the backend never computes them from a price series. The prompt (`PromptInputs` in `llm/prompt.rs`) carries only the **chart image** (base64 PNG), the **retrieved text chunks**, and a **single CoinGecko spot price** — there is **no OHLCV series at all** today, so every indicator value in an analysis is a visual estimate from the screenshot.
- **No approximate-nearest-neighbour index** — retrieval is brute-force cosine over every row.
- **No code signing, no installer hardening, no auto-update.**

### Known gaps that gate everything downstream

| Gap | Where | Impact |
|---|---|---|
| Brute-force vector search loads all rows per query | `lance.rs::search` | Scales poorly as the library grows; M2 target. |
| Hardcoded ~12-coin symbol table | `coingecko.rs::symbol_to_id` (BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC) | Unlisted symbols error; price/resolution fails for them. |
| Prediction resolution polls a *spot* price, never intra-horizon highs/lows | `predictions::resolve_one` | Cannot evaluate whether SL/TP would have hit; M3 target. |
| Tauri capabilities are minimal | `capabilities/default.json` (only `core:default`, `opener:default`) | Any new IPC/plugin surface needs explicit allowlisting. |
| Bundle targets `"all"`, unsigned | `tauri.conf.json` | No signed, Windows-focused release artefact; M1 target. |
| `settings.hotkey` is persisted but **dead/unwired** | `lib.rs::run` hardcodes `Ctrl+Shift+Space`; never reads `settings.hotkey` | Editing the hotkey setting silently does nothing; M1 should wire it. |
| Cost rates are hardcoded and substring-matched | `cost.rs::rates_for` (rates keyed to Sonnet 4.6 / Opus 4.7 / Haiku 4.5) | Any change to `settings.model_main`/`model_extract` to a newer model with different pricing — but the same `opus`/`haiku`/neither substring — will **silently mis-price** cost tracking until `rates_for` is updated. Relevant to M1 settings validation and any model bump. |

---

## 2. Milestones

The first three milestones (M1–M3) keep CogniTrade strictly **advisory** and ship real, safe value on Windows. M4 is the high-risk, optional crossing into real-money territory and is **gated behind heavy testing**.

### Milestone M1 — Polish the current advisory app to a shippable, signed Windows build

**Objective.** Turn the working dev app into a release-quality, code-signed Windows artefact that a non-developer can install and run, with no behavioural changes to the advisory flow.

**Scope (Implemented features only — this is hardening, not new capability).**

- Target Windows as the **primary platform**: produce an **MSI (WiX)** and/or **NSIS** installer via `npm run tauri build`. Narrow `tauri.conf.json` `bundle.targets` from `"all"` to the Windows installer(s) actually shipped.
- **Code signing**: configure Authenticode signing for the installer and the `.exe` (signing cert + `tauri.conf.json` `bundle.windows` settings). Document the WebView2 runtime requirement for end users.
- **First-run UX**: clear empty states for missing API key (probed via the `ipc::get_api_key_set` command, which returns a `bool`) and empty library; surface `AppError::NoApiKey` / `AppError::CostCapReached` as friendly messages in the Chat/Settings tabs rather than raw error strings.
- **Settings hardening**: validate `daily_cost_cap_usd`, `model_main`, and `model_extract` on save (`ipc::save_settings` → `settings::save`). **Wire `settings.hotkey` to the global-shortcut registration** in `lib.rs::run` and validate it on save — today the hotkey is hardcoded (`Ctrl+Shift+Space`) and the `settings.hotkey` field is **unused / dead**, so the setting must first be made functional before validating it. When `model_main`/`model_extract` change, also account for the `cost.rs::rates_for` mis-pricing gap (above). Confirm keyring round-trips on Windows Credential Manager via the IPC commands `set_api_key` / `get_api_key_set` / `clear_api_key`, which delegate to `settings::set_api_key` / `settings::get_api_key` / `settings::clear_api_key`.
- **Logging/telemetry-lite**: ensure `tracing` writes to `%USERPROFILE%\CogniTrade\logs` so end-user issues are diagnosable; rotate/cap log size.
- **Stability pass**: confirm the hardcoded `Ctrl+Shift+Space` hotkey re-registration tolerance (`lib.rs` already warns rather than panics), tray show/hide, and graceful behaviour when the BGE-small download is in progress on first ingest.
- **CI / reproducible build**: document and (ideally) automate the Windows toolchain (Rust MSVC, Visual Studio C++ Build Tools, WebView2) so the build is repeatable.

**Key tasks.**

1. Pin and document the Windows build toolchain; produce a clean signed installer locally.
2. Acquire/configure an Authenticode certificate; wire signing into the bundle step.
3. Constrain `bundle.targets`; add Windows-specific bundle metadata (publisher, upgrade GUID).
4. Wire `settings.hotkey` into `lib.rs::run`'s `Shortcut` registration (today it is hardcoded and the field is unread), validate it on save, and verify keyring round-trips through the `ipc.rs` API-key commands on Windows Credential Manager.
5. Polish error surfaces in `src/lib/components/*.svelte` for `NoApiKey`, `CostCapReached`, network failures.
6. Add file logging + rotation under `logs/`.
7. Manual install/uninstall smoke test on a clean Windows VM (no dev tools present, WebView2 absent then present).

**Exit criteria.**

- A **signed** MSI/NSIS installer installs on a clean Windows 10/11 machine, launches, and completes one full `analyze` round-trip (screenshot → streamed analysis → JSON recommendation → prediction row written).
- `npm run check` and `cd src-tauri && cargo test` both pass.
- Uninstall removes program files but **preserves** `%USERPROFILE%\CogniTrade\` user data.
- No raw `AppError` strings leak to the UI for the common failure paths.

**Rough effort: 2–3 weeks.**

---

### Milestone M2 — Real indicators + better retrieval

**Objective.** Replace two "the model guesses it" approximations with computed, deterministic signals: (a) backend-computed technical indicators feeding the prompt, and (b) a proper vector index so retrieval scales.

> The original spec lists RSI / MACD / Bollinger Bands and "both numerical indicator values AND visual chart reading" as in-scope. Today only the **visual** reading exists: the backend passes the model only the chart image, the retrieved text chunks, and a single CoinGecko spot price — **no OHLCV / numeric price series at all** — so the model has no choice but to estimate indicators from the screenshot. M2 adds the **numerical** half. Until M2 lands, treat indicator values in any analysis as **model-read from the chart, not computed** — i.e. Planned — not yet implemented at the backend level.

**Scope.**

- **Price/candle ingestion**: add a market-data module that fetches OHLCV candles for the resolved symbol. Extend the `coingecko.rs` mapping (or add a candle source) beyond the hardcoded ~12-coin table so indicator computation has data to work with.
- **Indicator computation (Rust)**: compute **RSI, MACD, Bollinger Bands** from the candle series in the backend, and inject the numeric values into `PromptInputs` (a new field alongside `retrieved_chunks`) so the model reasons over *computed* indicators, not just the chart image. Update `SYSTEM_PROMPT` to consume them and cite them.
- **Proper vector index**: replace the brute-force scan in `lance.rs::search` with an approximate-nearest-neighbour index (e.g. an HNSW/IVF library, or a real embedded vector store) while keeping the 384-dim `EMBED_DIM` contract and the existing `doc_chunks` schema/migration path. Keep CPU-only and within the **8GB-RAM** budget — benchmark memory before committing to a library.
- **Retrieval quality**: make `retrieval::search` `top_k` configurable, add a similarity floor so irrelevant chunks aren't injected, and surface which documents were cited back in the UI.

**Key tasks.**

1. Add a candle-data fetcher + a small symbol→source mapping with graceful "no data" handling.
2. Implement and unit-test RSI/MACD/BB (inline `#[cfg(test)]` per the repo convention; deterministic fixtures).
3. Thread computed indicators through `ipc::analyze` → `PromptInputs` → `build_messages`; update the system prompt + JSON schema notes.
4. Prototype and benchmark an ANN index under the RAM ceiling; migrate `LanceStore::search` behind the same trait surface.
5. Backfill/re-index existing `doc_chunks` on upgrade.

**Exit criteria.**

- For a supported symbol, an analysis includes **backend-computed** RSI/MACD/BB values, and the rationale references them.
- Retrieval over a library of **10k+ chunks** returns top-k in well under the previous brute-force latency, with steady-state RAM still inside the 8GB target (benchmarked and documented).
- New indicator and retrieval code has unit tests; `cargo test` and the `ingest_integration` smoke test (`-- --ignored`) pass.

**Rough effort: 4–6 weeks.**

---

### Milestone M3 — Paper-trading + backtesting harness (no real money)

**Objective.** Turn the existing "predictions" self-calibration loop into a genuine **paper-trading and backtesting** capability that can simulate SL/TP outcomes — **without ever touching an exchange or real funds.**

> This is the safe rehearsal for M4. It directly addresses today's biggest evaluation gap: `predictions::resolve_one` only samples a single spot price at the horizon and **does not simulate intra-horizon SL/TP hits**. M3 fixes that in simulation only.

**Scope.**

- **Intra-horizon simulation**: extend resolution to walk the candle path between `ts` and `ts + horizon_seconds` and determine whether `stop_loss_pct` or `take_profit_pct` would have been hit first — recording the realistic outcome instead of a single endpoint price. Today the `outcome_status` enum is only `pending` / `resolved` / `failed` (see Section 1); this milestone **adds net-new states** (e.g. `tp_hit`, `sl_hit`, `expired`) plus the path data needed to justify them.
- **Paper portfolio**: a virtual balance and position ledger (new SQLite tables in `chat.db`, additive migration) that applies each *approved* suggestion as a simulated fill at the limit price, then marks-to-market and closes on simulated SL/TP/horizon. No order ever leaves the machine.
- **Backtesting harness**: replay historical candles against the analysis/recommendation logic to produce aggregate calibration stats — hit rate vs `predicted_prob_up`, win/loss, simulated P&L, max drawdown. This is the data behind honest, *derived* probability statements (the spec's "based on the last N similar patterns…" claim).
- **Calibration UI**: a panel (Library or a new tab) showing prediction accuracy and paper-portfolio performance, reading from the resolved `predictions` rows.

**Key tasks.**

1. Add a candle-history fetch/caching layer (reuse M2's data module) sufficient for intra-horizon and backtest replay.
2. Implement intra-horizon SL/TP simulation in `predictions` with new outcome states + tests against synthetic price paths.
3. Add paper-portfolio tables + ledger logic (additive migration, e.g. `002_paper.sql`); enforce that fills are simulation-only.
4. Build the backtest runner (offline, batch) and the calibration/P&L aggregation queries.
5. Frontend: calibration + paper-portfolio views; wire to new IPC commands.

**Exit criteria.**

- Predictions resolve with **realistic SL/TP-aware** outcomes derived from the candle path, not a single spot price.
- A paper portfolio can run end-to-end (suggestion → simulated fill → simulated close) with a correct running balance and **zero** network calls to any exchange.
- The backtest harness produces a calibration report (predicted-prob bucket vs realized up-rate) and simulated drawdown over a historical window.
- All new logic is unit-tested; migrations apply cleanly over an existing `chat.db`.

**Rough effort: 5–7 weeks.**

---

### Milestone M4 — Exchange integration + pre-trade approval loop + real SL/TP enforcement

> **GATING REQUIREMENT.** M4 introduces real money and is **out of scope unless M1–M3 are complete and M3's paper/backtest results are reviewed and accepted.** It must not begin until the simulation harness has demonstrated correct SL/TP behaviour over an extended run. Everything in M4 is currently **Planned — not yet implemented.**
>
> **Scope note — not autonomous.** Even when shipped, M4 is **execution-with-approval**, not autonomous trading: every order requires a recorded per-trade human approval and enforced SL/TP, so M4 is *semi-autonomous at most*. The spec's fully autonomous, lights-out 24/7 trading and its **Minimax decision engine** are **not part of this roadmap.**

**Objective.** Add real exchange connectivity (the spec names **Binance** and **Kraken**) under three non-negotiable safety constraints from [`../../CLAUDE.md`](../../CLAUDE.md):

1. **Pre-trade human approval** — the bot must NEVER execute a trade without first sending a full suggestion (action, quantity, limit price, plain-English rationale, retrieved-knowledge justification, probability, SL/TP) and receiving explicit approval. This is a workflow requirement, not a toggle.
2. **Stop-loss / take-profit enforcement** — user-set SL%/TP% per trade must be enforced **without exception**; any code path that could bypass them is a bug.
3. **Max-drawdown / position-sizing risk controls** applied before any order is submitted.

**Scope.**

- **Exchange adapter** (read-first): authenticated balances, open orders, and live price/candle feed for Binance and Kraken, with API secrets stored in the OS keyring alongside the existing `anthropic_api_key` entry (never in the DB). New Tauri capability entries in `capabilities/default.json` for any added IPC surface.
- **Pre-trade approval loop**: every suggestion is presented in the UI as an explicit approve/reject card; **no order is submitted without a recorded approval event.** Reuse the existing `AnalysisJson` recommendation (action, quantity to be added, limit price, SL/TP, probability, citations) as the suggestion payload.
- **Real SL/TP enforcement**: place and *maintain* protective stop/limit orders on the exchange (or a supervised local enforcement loop) so the user's SL%/TP% cannot be silently bypassed. Reconcile against exchange state continuously.
- **Risk management**: position sizing, per-trade and aggregate exposure limits, and **max-drawdown protection** that halts new orders when breached.
- **Kill switch & audit trail**: a one-click "halt all trading / cancel open orders" control, and an immutable log of every suggestion, approval, submission, fill, and SL/TP action.
- **Heavy testing gate**: exchange **testnet/sandbox** first; staged rollout with hard per-day notional caps; extensive integration tests (mock exchange via `wiremock`, plus testnet smoke tests) before any mainnet key is accepted.

**Key tasks.**

1. Build read-only exchange adapters (Binance, Kraken) + keyring-backed secret storage + capability allowlisting.
2. Implement the approval-event data model and UI; make order submission *physically impossible* without a stored approval.
3. Implement protective-order placement + a reconciliation/enforcement loop for SL/TP.
4. Implement position sizing, exposure limits, and max-drawdown halt.
5. Implement the kill switch + full audit log.
6. Testnet validation, then a tightly capped, opt-in mainnet pilot.

**Exit criteria.**

- No order can be submitted without a persisted, user-initiated approval event (verified by tests that attempt to bypass it and fail).
- SL/TP orders are demonstrably placed and maintained on a testnet; forced scenarios show SL/TP firing as configured.
- Max-drawdown halt and kill switch are tested and effective.
- A full testnet run completes with a complete, tamper-evident audit trail; mainnet remains gated behind explicit per-day caps and an opt-in.

**Rough effort: 10–16+ weeks (high uncertainty; security- and reliability-bound, not feature-bound).**

---

## 3. Timeline

Indicative, sequential schedule. M1–M3 are committed advisory work; **M4 is conditional** on M3 sign-off. Effort ranges are calendar weeks for a small team; treat as planning estimates, not commitments.

| Milestone | Theme | Money at risk | Effort | Cumulative (earliest) |
|---|---|---|---|---|
| **M1** | Signed Windows release of the advisory app | None | 2–3 wks | ~3 wks |
| **M2** | Computed indicators + ANN retrieval | None | 4–6 wks | ~9 wks |
| **M3** | Paper-trading + backtesting (SL/TP simulation) | None (simulated) | 5–7 wks | ~16 wks |
| **M4** | Exchange + approval loop + real SL/TP (**gated**) | **Real** | 10–16+ wks | ~32 wks |

### Gantt-style view (each `#` ≈ 1 week, earliest-start)

```
Week:        1    5    10   15   20   25   30   35
M1  Release  ###
M2  Indic/RAG    ######
M3  Paper/BT           #######
M4  Exchange*                 ##################...
                              ^ gated on M3 acceptance
```

`*` M4 does not begin until M1–M3 ship **and** M3's paper-trading / backtest results are reviewed and accepted. Until then, every M4 capability remains **Planned — not yet implemented**, and CogniTrade stays strictly advisory.

---

## 4. Constraints that bind every milestone

From [`../../CLAUDE.md`](../../CLAUDE.md) — these are hard and override convenience:

- **8GB RAM ceiling** (Ryzen 3400G, 8GB RAM, 1TB storage, 450W PSU). Every design choice — embedding model size, ANN index, candle caching, concurrency — must be benchmarked against it. The CPU-only, single-mutex embedding singleton and the synchronous design exist *because* of this budget; do not casually relax it.
- **Windows is the primary platform.** Installers are MSI (WiX) / NSIS; secrets live in Windows Credential Manager via `keyring`; user data lives under `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}`; the runtime requires WebView2, and builds require the Rust MSVC toolchain + Visual Studio C++ Build Tools.
- **Advisory until proven otherwise.** Through M3, the app must keep the `llm/prompt.rs` guarantee — *"You are an advisor. The user places all trades themselves."* Real execution (M4) only arrives behind human approval, enforced SL/TP, risk limits, and heavy testing.
- **This is not a git repository.** If versioning/CI is introduced as part of M1, initialise it explicitly first.
