# Vision & Scope — Spec vs. Implementation Reconciliation

**Project:** CogniTrade
**Document owner:** Engineering
**Status:** Authoritative scope boundary for v1
**Last updated:** 2026-06-09
**Primary platform:** Windows (WebView2 + Rust MSVC toolchain). Note: `src-tauri/tauri.conf.json` currently sets `bundle.targets` to `"all"`, so the build is not pinned to Windows; the keyring backend is OS-dependent (Windows Credential Manager on the Windows target).

---

## 1. Why this document exists

CogniTrade began as an ambitious brief: an *autonomous* crypto-trading bot that reads charts, learns from PDFs, computes probabilities, and **executes trades** on Binance/Kraken under hard stop-loss/take-profit enforcement (see [`crypto Dhriti.txt`](../../crypto%20Dhriti.txt) and the design notes under [`docs/superpowers/`](../superpowers/)).

The application that actually exists is **deliberately narrower and safer**: an **advisory chart analyst**. It reads a chart screenshot with Claude vision, grounds its reasoning in documents you ingest (RAG), streams a plain-English analysis, and ends with a structured JSON recommendation. It **does not connect to any exchange, place any order, or enforce any stop-loss**. The system prompt is explicit about this posture:

```text
You are an advisor. The user places all trades themselves.
```
— `src-tauri/src/llm/prompt.rs`, `SYSTEM_PROMPT`

This document reconciles the two. It is the source of truth for **what CogniTrade is, what it is not, and where the boundary sits for v1**. Where the brief describes a capability that is not built, this document labels it **Planned — not yet implemented** and never presents it as existing.

> **One-line positioning:** CogniTrade is an *advisor, not an auto-trader*. Every order is placed by a human, by hand, on their own exchange.

---

## 2. Product vision

### 2.1 The "advisor, not auto-trader" positioning

CogniTrade is a **decision-support tool for a single trader on a single constrained desktop PC** (target: Ryzen 3400G, 8 GB RAM, 1 TB storage, 450 W PSU). The 8 GB RAM ceiling is a hard constraint that drives every model-size, concurrency, and embedding choice.

The user workflow is:

1. The user captures or drops a chart **screenshot** into the app (or pastes one in the Chat tab).
2. CogniTrade reads the image with Claude vision, retrieves relevant passages from the user's ingested library (RAG), and **streams** a plain-English analysis back.
3. The analysis ends in a fenced ```json``` recommendation. Only `action` (buy/sell/hold), `probability_up`, and `horizon` are **required**; `stop_loss_pct`, `take_profit_pct`, `key_signals`, and `citations` are **optional** (`#[serde(default …)]` in `src-tauri/src/llm/types.rs`), so the model may omit SL/TP entirely and the JSON still validates.
4. The user reads the rationale and **decides for themselves** whether to act. CogniTrade never touches an exchange.

The brief's "pre-trade human approval" requirement is satisfied in the strongest possible form: **there is no execution path to approve at all.** The human is not approving an automated order — the human *is* the only actor that can place an order.

### 2.2 Why advisory is the right v1

The brief's most dangerous requirement — *"the bot must enforce [SL/TP] without exception"* combined with live order execution — is precisely the requirement that is unsafe to ship first. An execution engine that can place and close real-money positions, but has never been validated against historical data or simulated fills, is a liability, not a feature. CogniTrade v1 intentionally defers all execution until the safety scaffolding (paper trading + backtesting) exists. See [§5 Guardrails](#guardrails).

---

## 3. Scope

### 3.1 In scope — implemented today

These capabilities are present in the codebase and exercised by the single IPC command `ipc::analyze` and its supporting modules.

- **Vision chart reading.** A chart screenshot is base64-encoded and sent to the analysis model. A small Haiku vision call (`extract_symbol_from_image` in `src-tauri/src/ipc.rs`) resolves the trading symbol when the user gives no hint; it falls back to `BTCUSDT`.
- **RAG over a user library.** Query text is embedded (fastembed BGE-small, 384-dim) and matched by brute-force cosine similarity over `doc_chunks` (`retrieval::search` → `lance::LanceStore::search`). The top-k passages are injected into the prompt as a cached "Retrieved knowledge" block, and the model is instructed to cite them by document name/page.
- **Document ingestion.** PDFs (`ingest::ingest_pdf` → `chunker::chunk_text`, ≈500-word chunks / 50-word overlap) and images (`ingest::ingest_image`, where Haiku describes the image and the *description* is embedded). A filesystem watcher (`ingest::watcher`, `notify`, 800 ms debounce) auto-ingests files dropped into the library directory.
- **Streaming plain-English analysis.** SSE deltas from the Anthropic API are forwarded to the frontend via the Tauri event `analysis_chunk`; completion is signalled by `analysis_done`. Frontend wiring is in `src/lib/stores/chat.ts`.
- **Structured recommendation.** The trailing ```json``` block is parsed (`llm::parse::extract_json_tail`) into `AnalysisJson`. Required fields: `action`, `probability_up ∈ [0,1]`, `horizon`. Optional fields (may be absent/empty): `stop_loss_pct`, `take_profit_pct`, `key_signals`, `citations`. This is a **JSON tail of free text, not tool-use**. Parsing is gated by `AnalysisJson::validate`: if validation fails (e.g. `probability_up` outside `[0,1]`), `extract_json_tail` returns `None` (`parsed.validate().ok()?`), so the recommendation is **dropped entirely** — no `predictions` row is written — rather than corrected.
- **Self-calibration ("predictions") loop.** Each analysis writes a `predictions` row (`outcome_status='pending'`). A background loop (`prune::spawn_background`, every 60 s) resolves predictions whose `ts + horizon_seconds` has elapsed by polling CoinGecko (`predictions::resolve_one`), recording price-at-prediction vs price-at-horizon. Horizons: `1h / 4h / 1d / 3d / 1w`.
- **Live spot price.** CoinGecko free `simple/price` endpoint via a hardcoded ~12-coin symbol→id table (`coingecko.rs`: BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC). Unlisted symbols error.
- **Cost control.** Per-day token usage is priced and capped (`cost.rs`); `analyze_with_cap` enforces `settings.daily_cost_cap_usd` (default **$2**) **before** each main call, returning `AppError::CostCapReached` if exceeded.
- **Secrets & settings.** The Anthropic API key lives in the OS keyring (Windows Credential Manager on the Windows target; service `cognitrade`, user `anthropic_api_key`), never in the DB. Non-secret settings live in the `settings` table.
- **Desktop shell.** Tauri 2 app with a 3-tab SvelteKit SPA (Chat / Library / Settings — `src/routes/+page.svelte`), a system tray, and a `Ctrl+Shift+Space` global hotkey to toggle the panel.

### 3.2 Out of scope — today (and for v1)

Explicitly **not built** and **not planned for v1**:

- **No exchange integration.** There is no Binance, Kraken, or any other exchange client anywhere in the codebase.
- **No order execution.** No code path can place, modify, or cancel an order.
- **No automated stop-loss / take-profit enforcement.** `stop_loss_pct` / `take_profit_pct` are *advisory numbers in the recommendation JSON*. Nothing monitors price to trigger them. The prediction-resolution loop records price at the horizon only — it does **not** simulate intra-horizon SL/TP hits (`predictions::resolve_one`).
- **No Minimax decision engine.** The brief's "RAG + Minimax" engine does not exist; recommendations come from a single LLM call.
- **No position sizing / drawdown / portfolio risk management.** The recommendation includes no quantity or limit price, and the app holds no notion of a portfolio.
- **No numerical indicator pipeline.** RSI/MACD/Bollinger values are not computed; the model reads indicators *visually* from the screenshot if they are drawn on it.
- **No multi-user, no cloud/VPS offload.** Single-user desktop only.

<a id="future-scope"></a>
### 3.3 Candidate scope — future versions, Planned and not yet implemented

Listed to mark the boundary, **not** as commitments. None of these exist today:

- **Planned — not yet implemented:** Backtesting harness over historical candles.
- **Planned — not yet implemented:** Paper-trading / simulated-fill engine (the gate to any real execution; see [§5](#guardrails)).
- **Planned — not yet implemented:** Numerical indicator computation (RSI/MACD/Bollinger) to complement visual reading.
- **Planned — not yet implemented:** Minimax / multi-step decision engine.
- **Planned — not yet implemented:** Exchange read-only integration (balances/candles), then — far later, behind explicit gates — order execution and automated SL/TP.
- **Planned — not yet implemented:** Replacing the brute-force cosine vector search with an indexed store as the library grows (the `lance.rs` / `paths::lancedb()` naming is a misnomer; it is plain SQLite, not LanceDB).

---

## 4. Spec ↔ implementation reconciliation table

Status legend: **Implemented** = present and working in code · **Partial** = a related but narrower mechanism exists · **Planned** = not in the codebase.

| # | Spec capability (from `crypto Dhriti.txt`) | Status | Where in code / Why deferred |
|---|---|---|---|
| 1 | Read live charts, candlesticks, indicators | **Partial** | Visual reading of a *screenshot* via Claude vision in `ipc::analyze`; symbol via `extract_symbol_from_image`. No live data feed and **no numerical RSI/MACD/Bollinger computation** — indicators are read only if drawn on the image. |
| 2 | RAG self-learning from user PDFs/research | **Implemented** | `ingest::ingest_pdf` / `ingest_image` → `chunker::chunk_text` → fastembed BGE-small → `doc_chunks`; retrieved via `retrieval::search` and injected as a cached prompt block. Auto-ingest by `ingest::watcher`. |
| 3a | Suggestion: action (buy/sell/hold) | **Implemented** | `AnalysisJson.action` parsed from the JSON tail (`llm::types`, `llm::parse::extract_json_tail`). |
| 3b | Suggestion: quantity & limit price | **Planned** | Not in `AnalysisJson` and not produced. Deferred: requires portfolio/position-sizing context the advisory app intentionally lacks. |
| 3c | Plain-English technical + retrieved-knowledge rationale | **Implemented** | Streamed analysis text (`analysis_chunk` event); system prompt requires citing retrieved passages by doc/page. |
| 3d | Probabilistic reasoning ("72% chance…") | **Partial** | The model emits `probability_up ∈ [0,1]`; a self-calibration loop (`predictions` + `prune::spawn_background` + `predictions::resolve_one`) records realized outcomes to evaluate calibration over time. It does **not** yet derive probabilities from a back-tested historical pattern base. |
| 3e | SL/TP rationale | **Implemented (as advice)** | `stop_loss_pct` / `take_profit_pct` are **optional** recommendation fields; when present they are stored on the prediction row. The model may omit them and still produce a valid analysis. **Advisory only** — see item 5. |
| 4 | Fixed SL/TP **enforced without exception** | **Planned** | No enforcement exists; no execution to enforce against. The resolution loop records horizon price, not intra-horizon SL/TP triggers. Deferred until paper trading + backtesting exist (see [§5](#guardrails)). |
| 5 | Pre-trade human approval before execution | **Implemented (by design)** | There is **no execution path**; the human places every trade manually. The strongest form of "approval." |
| 6 | Run 24/7 on 8 GB-RAM desktop | **Implemented** | Tauri desktop app; CPU-only embeddings behind a single global mutex (`OnceLock<Mutex<TextEmbedding>>`); background prune/resolution loops; tray + hotkey for always-on use. |
| 7 | RAG + **Minimax** decision engine | **Planned** | No Minimax engine; a single LLM call produces the recommendation. |
| 8 | Exchange integration (Binance/Kraken) | **Planned** | No exchange client of any kind in the codebase. |
| 9 | Position sizing / max-drawdown protection | **Planned** | No portfolio model; out of scope for the advisory v1. |
| 10 | Cost / resource discipline | **Implemented** | `cost.rs` daily-cost tracking + `analyze_with_cap` hard cap (default `$2/day`) returning `CostCapReached`. |

---

<a id="guardrails"></a>
## 5. Guardrails — financial-safety rationale

CogniTrade stays **strictly advisory** until two pieces of safety scaffolding exist and are validated. This is a deliberate engineering and ethical stance, not an oversight.

### 5.1 The two gates before any execution

1. **Backtesting (Planned — not yet implemented).** Before any recommendation policy or SL/TP rule is trusted with real money, it must be replayed against historical candle data so its win rate, drawdown, and SL/TP-hit behaviour are *measured*, not assumed. Today the system has no historical replay — `predictions::resolve_one` records only a single price snapshot at the horizon.
2. **Paper trading / simulated fills (Planned — not yet implemented).** Before live orders, the system must trade against a simulated order book / fill model so slippage, partial fills, and SL/TP triggering are validated end-to-end *without* capital at risk.

**Until both gates exist and pass, no execution code ships.** The advisory boundary is the safety mechanism.

### 5.2 Why the brief's "enforce SL/TP without exception" is *not* implemented yet

The brief demands hard SL/TP enforcement. Implementing enforcement *without* an exchange and *without* validated fill simulation would be worse than not having it: it would create a false sense of protection. A stop-loss that has never been tested against gaps, partial fills, or exchange downtime is a hazard. So `stop_loss_pct` / `take_profit_pct` are surfaced as **advice the human applies on their own exchange**, and the code is honest about that. Any future code path that could place or auto-close an order while bypassing validated SL/TP handling must be treated as a bug.

### 5.3 Operational guardrails that *do* exist today

- **No execution surface.** The app physically cannot place a trade — the strongest possible failsafe.
- **Daily spend cap.** `analyze_with_cap` blocks calls once `daily_cost_cap_usd` (default `$2`) is reached.
- **Secret hygiene.** API key in the OS keyring (Windows Credential Manager on the Windows target), never in `chat.db`.
- **Calibration honesty.** `probability_up` must be in `[0,1]` (`AnalysisJson::validate`), and the prompt demands calibrated, non-advocacy probabilities. `validate()` also acts as a hard parsing gate: an out-of-range probability makes `extract_json_tail` return `None`, so the structured recommendation is silently discarded and **no prediction is recorded** rather than stored with a bad number. The predictions loop then measures whether the model is actually calibrated on the recommendations that do pass.
- **Local-only data.** User library, screenshots, DBs, and logs live under `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` and are never committed.

---

## 6. Stakeholders

| Stakeholder | Interest in CogniTrade |
|---|---|
| **Primary user / trader** (single-seat desktop operator) | Gets grounded, document-cited chart analysis and a structured recommendation to inform a manual trade. Bears all execution risk personally. |
| **Project owner / sponsor** | Wants the brief's vision realized *safely and incrementally*; accepts the advisory-first boundary. |
| **Engineering** | Owns the Rust/Tauri backend and SvelteKit frontend; owns this scope boundary and the [§3.3 future-scope](#future-scope) gating. |
| **Anthropic API** (external dependency) | Vision + analysis (`model_main`, default `claude-sonnet-4-6`) and extraction (`model_extract`, `claude-haiku-4-5-20251001`). |
| **CoinGecko** (external dependency) | Free spot price for the ~12 supported coins and prediction resolution. |

---

## 7. Success definition

CogniTrade v1 is successful when, on the target 8 GB-RAM desktop:

1. **It runs 24/7 within budget.** Stays resident (tray + `Ctrl+Shift+Space`), and daily API spend respects `daily_cost_cap_usd` via `analyze_with_cap`.
2. **It produces grounded, cited analysis.** A chart screenshot yields a streamed plain-English analysis that cites retrieved library passages by document/page and ends in a valid `AnalysisJson` (with `probability_up ∈ [0,1]`).
3. **It never trades.** No exchange call, no order, no auto-SL/TP — verifiable by the absence of any execution code path. The advisory boundary holds.
4. **It self-evaluates.** The predictions loop resolves due predictions against CoinGecko and accumulates the data needed to judge probability calibration.
5. **It stays within the RAM ceiling.** Model choice, the single-mutex CPU embedder, and brute-force-but-small vector search keep memory under the 8 GB budget during normal operation.

**Non-goals for v1 success:** profitability of suggested trades, exchange connectivity, automated risk enforcement. Those are explicitly deferred behind the [§5](#guardrails) gates.

---

## 8. Related documents

- Original brief (aspirational, diverges from code): [`crypto Dhriti.txt`](../../crypto%20Dhriti.txt)
- Design notes / phased plan (aspirational): [`docs/superpowers/`](../superpowers/)
- Codebase guidance / architecture overview: [`CLAUDE.md`](../../CLAUDE.md)

> **Reminder for all contributors:** when a feature from the brief is referenced in code, docs, or UI but is not built, label it **"Planned — not yet implemented."** Never present an aspirational capability as if it exists. CogniTrade is an advisor; the human places every trade.
