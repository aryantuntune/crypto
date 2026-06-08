# Gap Analysis & Completion Plan

This document answers three questions precisely: **what is missing**, **what it takes to finish**, and **how long**. It is grounded in the actual source under [`../../src-tauri/src/`](../../src-tauri/src/) and [`../../src/`](../../src/) — not in the aspirational spec. Wherever a capability comes from the spec but no code exists, it is labelled **Planned — not yet implemented**.

> **What CogniTrade is today.** A Tauri 2 desktop app (Rust backend + SvelteKit/TypeScript static SPA) that acts as an **advisory** crypto-chart analyst. The user pastes/drops a chart screenshot; the backend reads it with Claude vision, retrieves passages from ingested PDFs/images (RAG), and streams back a plain-English analysis ending in a structured JSON recommendation. The system prompt (`llm/prompt.rs`, `SYSTEM_PROMPT`) is explicit: *"You are an advisor. The user places all trades themselves."*
>
> **What it is NOT.** There is **no exchange integration** (Binance/Kraken), **no order execution**, **no Minimax decision engine**, and **no automated stop-loss / take-profit enforcement**. The original brief (`crypto Dhriti.txt`) and the design docs under [`../superpowers/`](../superpowers/) describe a far more ambitious autonomous trading bot; treat them as aspirational background.

Related docs:

- Engineering reality / architecture: [`../../CLAUDE.md`](../../CLAUDE.md)
- Roadmap & milestones: [`../project/roadmap.md`](../project/roadmap.md)
- Build & release (Windows): [`../development/build-and-release.md`](../development/build-and-release.md)
- Windows dev setup: [`../development/windows-setup.md`](../development/windows-setup.md)
- Testing: [`../development/testing.md`](../development/testing.md)
- IPC API reference: [`../architecture/ipc-api-reference.md`](../architecture/ipc-api-reference.md)
- Aspirational design (diverges from code): [`../superpowers/specs/2026-05-05-cognitrade-design.md`](../superpowers/specs/2026-05-05-cognitrade-design.md)

---

## 1. Capability matrix

The "Spec requirement" column is taken from `crypto Dhriti.txt` and the `docs/superpowers/` design docs. "Implemented?" is judged strictly against current code.

| Spec requirement | Implemented? | Gap | Effort |
|---|---|---|---|
| Market data ingestion — live prices | **Partial** | `coingecko::get_usd_price` returns a single USD spot price from the CoinGecko free `simple/price` endpoint, gated by a **hardcoded ~12-coin** `symbol_to_id` table (BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC; `coingecko.rs`). No candlesticks, no streaming. The same table gates **both** the live-price call and the prediction self-calibration loop: an analyzed symbol outside the 12 coins (e.g. a ticker resolved from a screenshot) makes `get_usd_price` error, so `record_initial_price` is skipped and `resolve_one` marks the prediction `failed`. See the footnote under §B2. | S (more coins) → M (OHLC feed) |
| Technical indicators — RSI, MACD, Bollinger Bands (numeric) | **No** — Planned, not yet implemented | No indicator computation anywhere in the backend. The LLM is asked to *read* indicators off the chart image; no values are computed in Rust. | M |
| Visual chart reading | **Yes** | The screenshot is sent to Claude vision in `ipc::analyze` (`build_messages`); a tiny Haiku call (`extract_symbol_from_image`) reads the ticker. This is the core working capability. | — |
| RAG-based self-learning (ingest PDFs, chunk + embed, retrieve at reason time) | **Yes** | `ingest::ingest_pdf` / `ingest_image` → `chunker::chunk_text` (≈500 words / 50 overlap) → `fastembed` BGE-small (384-dim) → `lance` store. `retrieval::search` brute-force cosine top-6 feeds the prompt; retrieved text appears in the rationale. | — (works; scale caveat below) |
| Retrieved knowledge shown in human-facing rationale | **Yes** | Retrieved chunks are inserted as a cached block in `build_messages`; `AnalysisJson.citations` carries doc/page back to the UI (`PredictionCard.svelte`). | — |
| Probabilistic reasoning — "based on the last N similar patterns, 72% chance…" derived from history | **No** — Planned, not yet implemented | `AnalysisJson.probability_up` is a number **the LLM emits**, not a frequency derived from historical pattern matching. The `predictions` table is a *self-calibration* log (predicted vs. realized at horizon), not a pattern-probability engine, and it is not fed back into reasoning. | L |
| Pre-trade suggestion bundle (action, qty, limit price, technical + knowledge justification, probability, SL/TP rationale) | **Partial** | `AnalysisJson` carries `action`, `probability_up`, `horizon`, `stop_loss_pct`, `take_profit_pct`, `key_signals`, `citations` — but **no quantity and no limit price** (the app never sizes or prices an order). | M |
| Pre-trade human approval before any trade | **N/A by design** | The app never trades, so there is nothing to approve. To honour the spec's hard constraint, an approval loop must be built **with** execution (Track B). | L (with execution) |
| Stop-loss / take-profit **enforcement** | **No** — Planned, not yet implemented | SL/TP are advisory percentages in `AnalysisJson`. `predictions::resolve_one` records price-at-horizon vs. price-at-prediction; it **does not simulate intra-horizon SL/TP hits** and cannot enforce anything. | L |
| Exchange integration — Binance / Kraken | **No** — Planned, not yet implemented | No exchange client, no API-key handling for exchanges, no order types. | XL |
| Order execution | **No** — Planned, not yet implemented | No execution path exists anywhere. | XL |
| Minimax decision engine | **No** — Planned, not yet implemented | No such module. | L–XL |
| Risk management — position sizing, max-drawdown protection | **No** — Planned, not yet implemented | No sizing logic, no drawdown tracking. The only "limit" today is `daily_cost_cap_usd` (an **API spend** cap, not a trading-risk cap). | L |
| Cost control (LLM spend) | **Yes** | `cost.rs` tracks per-day tokens in `cost_daily`; `analyze_with_cap` enforces `settings.daily_cost_cap_usd` (default **$2**) before each main call, returning `AppError::CostCapReached`. | — |
| Secrets handling | **Yes** | Anthropic API key in OS keyring (`keyring`, service `cognitrade`, user `anthropic_api_key`) → Windows Credential Manager; never in the DB. | — |
| 24/7 local operation on 8 GB RAM | **Mostly** | App runs continuously with a tray + `Ctrl+Shift+Space` hotkey; background loops (`prune::spawn_background`) prune chat (>7d, every 6h) and resolve predictions (every 60s). CPU-only BGE-small chosen for the RAM budget. **Scale caveat:** vector search is brute-force (loads every row per query). | — (S to harden) |
| Signed, installable Windows release | **No** | `tauri.conf.json` has `bundle.targets: "all"` and **no** `bundle.windows` block, so there is **no code-signing config** and no WiX/NSIS tuning — builds produce an *unsigned* installer. (WebView2 is **not** part of this gap: with no `webviewInstallMode` set, Tauri defaults to `downloadBootstrapper`, which already installs WebView2 online if missing — see §A4.) | M (Track A) |

Effort key: **S** ≈ ≤1 day · **M** ≈ a few days · **L** ≈ 1–3 weeks · **XL** ≈ 1+ month (per item, solo).

---

## 2. Track A — Polish the current advisory app to a shippable, signed Windows release

**Goal:** take the working advisory app and make it a product a non-developer can install, trust, and use without hitting raw error strings or empty screens. **No new trading capability.** This is the recommended near-term track.

**Realistic estimate: ~1–2 weeks for a focused solo developer.** This assumes the existing pipeline already works end-to-end (it does) and that a code-signing certificate is available (procurement can add calendar time outside this estimate — see the caveat).

### A1. Error & empty states (frontend) — M

Today the UI shows almost nothing when things go wrong. `AppError` is serialized to the frontend (`error.rs`), but components don't render it consistently.

- Failed `analyze` (network failure, `CostCapReached`, `NoApiKey`): the rejection is handled in `lib/stores/chat.ts` `sendAnalyze`, which `ChatPanel.svelte` `send()` awaits **without its own try/catch**. Today the store's `catch` only appends a raw `\n\n[error: …]` string to the `streaming` store, so the user sees a stray text fragment rather than a real error surface. Fix the store + component together: have `sendAnalyze` surface the rejected `AppError` (not a stringified blob) and render it as a ChatPanel inline banner. `CostCapReached` in particular needs a friendly "daily $X cap reached" banner.
- Empty chat: render a first-run placeholder ("Paste a chart with Ctrl+V and hit Analyze") when `$messages` is empty.
- `LibraryView.svelte`: it lists `$ingestStatus` only for files touched this session — there is no view of what is already in the library. Add an empty state and (ideally) a "documents indexed: N" count.
- `SettingsView.svelte`: no validation feedback when saving an invalid key, no confirmation toast on save.

### A2. Settings UX — M

`SettingsView.svelte` is functional but minimal.

- The **main-model `<select>` is hardcoded** to `claude-sonnet-4-6` / `claude-opus-4-7`; keep it in sync with `settings.rs` defaults and `cost::rates_for`, or drive it from a shared constant.
- Expose `library_path` override and `hotkey` (both exist in `Settings` but have no UI).
- Show the live daily spend (`$cost.cost_usd`, returned by the `cost_today` IPC command) against the cap with a progress indicator. The store already renders `$cost.cost_usd`; note `cost_today` is the command name, while `cost_usd` is the field on the returned `DailyCost`.
- Add a "test API key" action that makes one cheap call so users learn the key works before their first analysis.

### A3. First-run model-download UX — M

`fastembed` BGE-small downloads on **first embedding use** (lazy `OnceLock<Mutex<TextEmbedding>>` in `ingest::embeddings`), behind a mutex, synchronously. Today the first ingest or first analysis silently blocks while the model downloads.

- Add a visible "downloading embedding model (one-time)…" state, ideally triggered at first run rather than on the user's first action.
- Handle the offline / download-failure case with a retry, instead of a stuck spinner.

### A4. WebView2 bundling decision — S

The app requires the WebView2 runtime. `tauri.conf.json` sets no `bundle.windows.webviewInstallMode`, so Tauri applies its **default**, `downloadBootstrapper`: the installer ships a small bootstrapper that downloads and installs WebView2 online at install time **if it is missing**. So WebView2 is already handled for online installs out of the box — this is not an unfilled gap.

The only real decision is whether you need offline/air-gapped installs. The full option set (set under `bundle.windows.webviewInstallMode`) is `downloadBootstrapper` (default) | `embedBootstrapper` | `offlineInstaller` | `skip`:

- For **online** installs, the default `downloadBootstrapper` is adequate — no config change needed.
- For **offline/air-gapped** targets, explicitly choose `offlineInstaller` (embeds the full ~150 MB runtime) or `embedBootstrapper`.
- Use `skip` **only** when WebView2 presence is guaranteed (the app fails to launch if it is absent).

This matches the option table in [`../development/build-and-release.md`](../development/build-and-release.md) §7.

### A5. Installer + code signing — M (plus possible procurement lead time)

- Add a `bundle.windows` block to `tauri.conf.json`: pick MSI (WiX) and/or NSIS explicitly rather than `targets: "all"`.
- **Code signing is not configured at all today.** An unsigned installer triggers SmartScreen warnings. Add `bundle.windows.certificateThumbprint` (+ `digestAlgorithm`, `timestampUrl`) or wire `signCommand`. **Caveat:** obtaining an OV/EV certificate is an external dependency that can add days–weeks of calendar time and is outside the engineering estimate.

### A6. Packaging hygiene — S

- `productName` is `cognitrade-app` and `version` is `0.1.0` in `tauri.conf.json`; set a real product name and version scheme for release.
- Confirm tray icon and installer icons are production assets (`icons/` is referenced).
- Verify user data lives under `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` and is never bundled/committed.

### A7. Basic end-to-end test — M

Current automated coverage: inline `#[cfg(test)]` unit tests plus **two `#[ignore]`d integration tests** (`tests/ingest_integration.rs`, `tests/llm_integration.rs`) that hit the network/model. There is **no** UI-level e2e.

- Add at least one happy-path e2e: launch → set key → paste a fixture screenshot → assert a streamed analysis + a `predictions` row. WebDriver (`tauri-driver`) is the standard route.
- Wire the `#[ignore]`d integration tests into a manual/nightly job so they don't rot (`cargo test --test ingest_integration -- --ignored --nocapture`).

### A8. Replace or document the brute-force vector search — S to M

`lance.rs` is a **misnomer** — it is plain SQLite, and `LanceStore::search` loads **every** `doc_chunks` row and computes cosine in Rust on each query. This is fine for a handful of PDFs and bad as the library grows.

- **Minimum (S):** document the limit honestly in user-facing docs and cap practical library size; it already validates `EMBED_DIM` (384) on insert.
- **Better (M):** add an ANN index (e.g. `sqlite-vec`/`usearch`) or actually adopt LanceDB to retire the misnomer. Evaluate against the **8 GB RAM** hard constraint before adding any in-memory index.

### Track A — exit criteria ("done")

- A signed `.msi`/`.exe` installs cleanly on a fresh Windows 10/11 box (WebView2 present), launches to the tray, and the `Ctrl+Shift+Space` hotkey opens the panel.
- A first-time user can: enter an API key, see the one-time model download complete, drop a PDF into the library, paste a chart, and receive a streamed analysis with a recommendation card and citations — **with no raw error strings**; cost cap, missing key, and offline cases all show friendly messages.
- `cargo test` is green; the e2e happy path passes; the `#[ignore]`d integration tests are runnable on demand.
- Build & release steps reproduce from [`../development/build-and-release.md`](../development/build-and-release.md).

---

## 3. Track B — Build the full autonomous-trading spec

**Goal:** the system in `crypto Dhriti.txt` — a bot that ingests market data, computes real indicators, derives genuine probabilities from history, proposes sized trades for human approval, executes them on Binance/Kraken, and enforces SL/TP with hard risk limits.

**Realistic estimate: ~3–6 months for a solo developer**, and that range is wide on purpose (see §5). Real-money execution dominates the risk and the schedule.

> ⚠️ **Safety-critical caveat.** Anything that places real orders can lose real money via bugs, partial fills, API outages, race conditions, or model error. **No real-money execution should ship until the system has run extensively in paper-trading/backtesting and passed a security review.** The spec's hard constraints — **pre-trade human approval** and **SL/TP enforcement without exception** — must be treated as non-negotiable invariants, not features. Build the safety machinery (approval loop, SL/TP engine, kill-switch) *before*, not after, the execution path.

### B1. Real indicator computation (RSI / MACD / Bollinger Bands) — M

Compute indicators in Rust from OHLC data rather than asking the LLM to eyeball them. Needs an OHLC feed first (CoinGecko free tier gives spot price only; OHLC/candles require a different endpoint or exchange data). This is the cheapest, lowest-risk Track-B item and is worth doing even within an advisory product.

### B2. Genuine historical-pattern probability engine — L

Replace the LLM-guessed `probability_up` with a derived frequency: define a similarity over recent pattern windows, retrieve the N most similar historical instances, and compute the empirical outcome distribution. The existing `predictions` self-calibration log is the seed for measuring whether these numbers are honest. Probabilities must be **derived, not invented** (CLAUDE.md hard constraint).

> **Data-coverage caveat for self-calibration.** The `predictions` log only accrues usable data for the 12 coins in the `coingecko.rs` `symbol_to_id` table. For any analyzed symbol outside that table, `coingecko::get_usd_price` errors, so `predictions::record_initial_price` is never called (`ipc.rs`) and `predictions::resolve_one` marks the row `failed` instead of `resolved` — it records no price-at-prediction and no price-at-horizon. Before this log can validate any probability engine, the supported-symbol set must be widened (or unlisted symbols handled explicitly); otherwise calibration is silently restricted to those 12 coins.

### B3. Pre-trade approval loop — L

A durable workflow: suggestion (action, **quantity**, **limit price**, technical + knowledge justification, probability, SL/TP rationale) → user notification → explicit approve/reject → only then proceed. Today `AnalysisJson` has no quantity/limit-price fields; the data model and IPC surface must grow. **No order may be placed without recorded approval.**

### B4. Exchange integration (Binance / Kraken) — XL

Authenticated REST/WebSocket clients, secret handling for exchange keys (extend the keyring pattern, never the DB), order placement/cancel/status, partial-fill and reject handling, rate limits, clock-skew/nonce handling, idempotency. Each exchange is its own substantial effort.

### B5. Real SL/TP enforcement engine — L

A loop that, once in a position, monitors price against the user's SL%/TP% and acts **without exception**. This is the single most safety-critical component. Today `resolve_one` only samples price at the horizon and cannot enforce anything intra-horizon. Needs reliable price streaming, exchange-side stop orders where available, a local watchdog as backstop, and a global kill-switch.

### B6. Position sizing & max-drawdown limits — L

Real risk management: position sizing rules, per-trade and portfolio caps, and a max-drawdown circuit breaker that halts trading. Distinct from the existing `daily_cost_cap_usd` (which limits **API spend**, not trading risk).

### B7. Paper-trading + backtesting harness — L

A simulated-execution mode and a historical backtester are prerequisites for trusting any of the above. This must come **before** live trading and should accumulate a track record measured against the §B2 probability engine.

### B8. Security review — M

Threat-model the secret storage (exchange + Anthropic keys), the approval loop (can it be bypassed?), the SL/TP engine (can any code path skip it?), IPC surface, and update/supply-chain. Treat any path that can place an order without approval or bypass SL/TP as a release blocker. See `/security-review`.

### Track B — exit criteria ("done")

- Indicators are computed from real OHLC and reconcile with a reference (TradingView/exchange) within tolerance.
- Probabilities are empirically derived and demonstrably calibrated against the `predictions` log over a meaningful sample.
- **Every** order is preceded by a recorded human approval; there is an automated test proving an un-approved order cannot be placed.
- SL/TP enforcement is proven in fault-injection tests (dropped connection, API error, restart mid-position) with a working kill-switch.
- The system has run in **paper-trading** for a sustained period with results matching backtests, and has passed a security review, **before** any real-money switch is even exposed in the UI.

---

## 4. Sequencing recommendation

1. **Do Track A first, in full.** It ships a usable, signed product on top of code that already works, and it de-risks distribution (signing, WebView2, installer) once and for all. ~1–2 weeks.
2. **Then, if Track B is pursued, do it in safety-first order:** B1 (indicators) → B7 (paper-trading/backtest harness) → B2 (probability engine) → B6 (risk limits) → B3 (approval loop) → B5 (SL/TP engine) → B4 (exchange clients, paper first) → B8 (security review) → only then a gated real-money toggle. Build the guardrails before the gun.
3. **Never interleave a real-money execution path into a release that hasn't passed B7 + B8.** A half-built executor is more dangerous than no executor.

The advisory app (Track A) and the autonomous bot (Track B) are effectively **two different products** sharing a RAG/vision core. Decide deliberately whether Track B is in scope before investing in it; the spec's autonomous vision is large and safety-critical, and shipping the advisory product does not require it.

---

## 5. Honesty about the estimates

These numbers are planning aids, not commitments.

- **Track A (~1–2 weeks)** is the more reliable estimate because it polishes working code. The biggest schedule risk is **external**: obtaining a code-signing certificate (§A5) can add days–weeks of calendar time outside engineering control. The vector-search rewrite (§A8) can be deferred to "document the limit," shrinking scope.
- **Track B (~3–6 months solo)** is inherently uncertain. Exchange integration (B4) and the SL/TP engine (B5) are where solo projects routinely overrun: edge cases (partial fills, outages, restarts mid-position) and the paper-trading soak time (B7) are real calendar time you cannot compress safely. A genuinely calibrated probability engine (B2) could take far longer if the first approaches don't validate.
- The range is wide deliberately. Treat any single point estimate inside it with suspicion until B1/B7 are done and the unknowns have been retired by working code.
