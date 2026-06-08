# CogniTrade — Risk Register

> Status: living document. Catalogues technical, financial/safety, and operational
> risks for the **implemented** CogniTrade app, with concrete mitigations and owners.
>
> **Scope note.** CogniTrade is a Tauri 2 desktop app (Rust backend + SvelteKit/TypeScript
> frontend) that is an **advisory crypto-chart analyst** — it reads a chart screenshot,
> retrieves passages from ingested PDFs/images (RAG), and streams back a plain-English
> analysis ending in a structured JSON recommendation. The single IPC entry point is
> `ipc::analyze` (`src-tauri/src/ipc.rs`). **There is no exchange integration, no order
> execution, no automated SL/TP enforcement, and no Minimax engine.** The system prompt
> (`llm::prompt::SYSTEM_PROMPT`, line 25) states verbatim: *"You are an advisor. The user
> places all trades themselves."* This advisor-only system prompt is, today, the **only**
> safety control in the product — there is **no** UI-level "not financial advice"
> disclaimer at first run or in the Chat panel (see F2/C1). Any "autonomous trading" risks
> below are labelled **Planned — not yet implemented** and describe risks that would arise
> *if* such features were ever added.

Related docs: see [`../../CLAUDE.md`](../../CLAUDE.md) for the authoritative architecture
summary, and the aspirational design under [`../superpowers/`](../superpowers/) and the
original brief [`../../crypto Dhriti.txt`](../../crypto%20Dhriti.txt) (both diverge from
the code).

## Hardware context (drives several technical risks)

Hard constraint per `CLAUDE.md`: target machine is a **Ryzen 3400G, 8 GB RAM, 1 TB
storage, 450 W PSU**, expected to run for long sessions. The 8 GB RAM ceiling is the
single most important budget constraint and is referenced by the embedding-model and
vector-store risks below.

## Likelihood / Impact scale

| Level | Likelihood (over ~6 months of normal use) | Impact |
|-------|--------------------------------------------|--------|
| Low | Unlikely / edge case | Minor: degraded UX, recoverable, no data/money loss |
| Medium | Plausible under realistic usage | Notable: feature broken, confusing output, wasted spend |
| High | Expected to occur | Severe: data loss, crash loop, real-money loss, or safety/legal exposure |

Owners are role labels (single-maintainer project): **Backend** (Rust/`src-tauri`),
**Frontend** (`src/`), **Product** (UX/safety/legal copy), **Ops** (the end user running
the app on their PC).

---

## Risk register (summary table)

| # | Risk | Category | Likelihood | Impact | Mitigation | Owner |
|---|------|----------|------------|--------|------------|-------|
| T1 | Brute-force cosine search in `lance::LanceStore::search` rescans every `doc_chunks` row per query; latency grows linearly with the library | Technical | Medium | Medium | Keep libraries small; add ANN index / SQLite VSS or a real vector DB before scaling; cap `top_k`=6 already in place | Backend |
| T2 | 8 GB RAM ceiling vs. `fastembed` BGE-small model + ONNX runtime + two SQLite WAL DBs + WebView2 | Technical | Medium | High | CPU-only single-flight embedding behind a global mutex; model is lazy-loaded; keep model at 384-dim BGE-small; never run a large local LLM | Backend / Ops |
| T3 | Single-user SQLite concurrency: blocking `std::sync::Mutex<Connection>` held across writes; background loops + IPC contend | Technical | Low | Medium | WAL mode enabled; short critical sections; serialize all access through `Db`/`LanceStore` mutex | Backend |
| T4 | LLM JSON-tail parse failure: `extract_json_tail` finds no fenced block, malformed JSON, or out-of-range `probability_up` → no prediction recorded | Technical | Medium | Low | Tolerant by design (`analysis: None`), prose still streamed/stored; strict schema validation; prompt instructs trailing ```json block | Backend / Product |
| T5 | CoinGecko free-tier rate limits / outages break price recording and prediction resolution | Technical | Medium | Low | Fire-and-forget initial price; failed resolution marked `outcome_status='failed'`, never crashes loop; back off / cache (planned) | Backend |
| T6 | Hardcoded ~12-coin `coingecko::symbol_to_id` table: any unlisted symbol errors out (no price, no resolution) | Technical | Medium | Low | Extend the match table; analysis still succeeds (price is best-effort); surface "unsupported symbol" in UI | Backend |
| T7 | Model-download first-run failure: BGE-small downloads on first `embed`; offline/proxy/disk failure breaks all ingestion + RAG | Technical | Medium | High | Document the first-run network requirement; surface a clear error; pre-warm during onboarding (planned) | Backend / Ops |
| F1 | LLM `probability_up` is model-asserted, **not** statistically derived or calibrated | Financial / Safety | High | High | Self-evaluation loop (`predictions`) records realized outcomes for calibration; prompt demands realistic uncertainty; **planned** UI "estimate" labelling | Product / Backend |
| F2 | Advisory output mistaken for financial advice; user acts on a wrong `buy`/`sell` and loses money | Financial / Safety | Medium | High | **Today: advisor-only system prompt only**; user places all trades manually. "Not financial advice" disclaimer is **planned — NOT yet implemented** | Product |
| F3 | Predictions resolve on price-at-horizon only — **no intra-horizon SL/TP simulation** — so reported "accuracy" overstates a real SL/TP strategy | Financial / Safety | Medium | Medium | Document the limitation; do not present resolution stats as backtested P&L; add path-aware resolution (planned) | Backend / Product |
| F4 | **Planned — not yet implemented:** if auto-trading / exchange execution were added, a bad suggestion could execute real orders with no human gate | Financial / Safety | Low (today: N/A) | High | Hard pre-trade human-approval gate, enforced SL/TP, max-drawdown limits would be **mandatory** before any execution code ships | Product / Backend |
| O1 | Anthropic API key handling: stored in OS keyring, but passed through IPC `set_api_key` and used as a header in plaintext HTTPS calls | Operational | Low | High | Key in OS keyring (Windows Credential Manager), never in DB; never logged; `clear_api_key` provided; TLS via rustls; frontend only sees `get_api_key_set() -> bool` | Backend / Ops |
| O2 | Cost runaway vs. the `$2`/day cap: cap checked **before** each main call, not mid-stream; cheap Haiku extract/ingest calls bypass the cap; unknown model IDs silently bill at Sonnet rates | Operational | Medium | Medium | `analyze_with_cap` enforces `daily_cost_cap_usd`; per-day usage tracked in `cost_daily`; prompt caching reduces input cost; document cap + rate-fallback semantics | Backend / Ops |
| O3 | 24/7 stability: background loops (`prune::spawn_background`) + file watcher run indefinitely; an un-handled panic or wedged mutex degrades the long-running app | Operational | Medium | Medium | Loops swallow errors and continue; debounced watcher; tracing logs to `%USERPROFILE%\CogniTrade\logs`; restart via tray | Backend / Ops |
| O4 | Library file-watcher silently skips image ingest when no API key is set | Operational | Medium | Low | Intentional: PDFs need no key, images need Haiku; log + surface "image skipped, set API key" in UI (planned) | Backend / Product |
| O5 | Global hotkey `Ctrl+Shift+Space` can collide with another app's global shortcut on Windows | Operational | Low | Low | Hotkey is user-configurable via `settings.hotkey` (default `Ctrl+Shift+Space`); registration is tolerant of "already registered" | Backend / Ops |
| O6 | `tauri.conf.json` `bundle.targets = "all"` — produced Windows installers depend on the build host's available bundlers, not a pinned set | Operational | Low | Low | Pin `targets` to an explicit `["msi","nsis"]` if a specific installer is required | Ops |
| C1 | Compliance/legal: outputs could be construed as regulated investment advice; **no** disclaimer ships today | Compliance / Legal | Medium | High | Advisor-only system prompt is the only current control; "not financial advice / for research only" disclaimer is **planned — NOT yet implemented**; no execution | Product |

---

## Technical risks (detail)

### T1 — Brute-force cosine scaling
`lance.rs` is a **misnomer**: there is no LanceDB dependency (`Cargo.toml` lists no
`lancedb`/`lance` crate). `LanceStore` is plain SQLite (`data/vec.db`, table `doc_chunks`)
storing embeddings as little-endian `f32` BLOBs. `search()` runs
`SELECT ... FROM doc_chunks` with **no `WHERE`/index**, decodes every BLOB, and computes
`cosine()` in Rust for each row before sorting and truncating to `k`:

```rust
// src-tauri/src/lance.rs — search() loads EVERY row each query
let mut stmt = conn.prepare(
    "SELECT id, doc_path, doc_type, chunk_index, text, page, embedding FROM doc_chunks",
)?;
// ... cosine(&query_emb, &row.embedding) per row, then sort + truncate(k)
```

- **Why it matters:** O(N·384) per query plus full-table BLOB I/O. Fine for a few
  hundred chunks; degrades linearly as users ingest more PDFs. On the 8 GB-RAM target this
  also transiently allocates every embedding into a `Vec<(f32, DocRow)>`.
- **Misnomer extends to the public API.** The naming that misleadingly implies a LanceDB
  dependency is not limited to the module file — it also covers:
  - the path accessor `Paths::lancedb()` (`paths.rs` line 15:
    `pub fn lancedb(&self) -> PathBuf { self.data().join("vec.db") }`), which `ipc.rs`
    calls as `state.paths.lancedb()` to open the store;
  - the type `LanceStore` (`lance.rs` line 18) and the constructor `lance::open()`
    (`lance.rs` line 41).

  A maintainer will grep for these names. Recommend renaming all three when the ANN/index
  migration is done (e.g. `paths::vec_db` / `VecStore` / `vec_store::open`) so the naming
  stops implying a dependency `Cargo.toml` does not contain.
- **Mitigation:** retrieval `top_k` is already capped at 6 (`ipc::analyze` calls
  `retrieval::search(&store, &q, 6)`). Before the library grows large, add an ANN index
  (e.g. `sqlite-vss`/`sqlite-vec`) or migrate to a real vector store. Track chunk count as
  a health metric.
- **Owner:** Backend.

### T2 — 8 GB RAM ceiling vs. `fastembed`
`ingest::embeddings` wraps `fastembed` BGE-small (`BGESmallENV15`, `EMBED_DIM = 384`) as a
lazily-initialized global `OnceLock<Mutex<TextEmbedding>>`. Embedding is **CPU-only and
synchronous behind the mutex** — a deliberate choice for the hardware budget. Co-resident
in 8 GB: the ONNX runtime + model weights, the WebView2 process, two WAL-mode SQLite DBs,
and the Tokio runtime.

- **Mitigation:** keep the model at 384-dim BGE-small; never introduce a large local LLM
  (would violate the budget — see `CLAUDE.md` hard constraints); single-flight embedding
  serializes memory spikes. Monitor RSS during bulk ingestion.
- **Owner:** Backend / Ops.

### T3 — Single-user SQLite concurrency
Both DBs are guarded by a blocking `std::sync::Mutex<Connection>` (`Db`, and
`LanceStore.inner`). The 60 s prediction-resolution loop, the 6 h prune loop, the watcher's
ingest tasks, and foreground `analyze` all contend for these locks. A long write (bulk
insert during ingest) can briefly block reads.

- **Mitigation:** WAL mode is enabled on both DBs (`PRAGMA journal_mode=WAL`); critical
  sections are short; inserts are batched in a single transaction
  (`LanceStore::insert`). Single-user app, so contention is low. Avoid holding the lock
  across `.await` (current code locks, executes, drops within sync helpers).
- **Owner:** Backend.

### T4 — LLM JSON-tail parse failures
The recommendation is a **trailing fenced ```json block of free text, not tool-use**.
`extract_json_tail` (`src-tauri/src/llm/parse.rs`) finds the *last* ```json fence, parses
it into `AnalysisJson`, and runs `validate()` (rejects `probability_up` outside `[0,1]`).
On any failure it returns `None`.

```rust
// parse.rs — returns None on missing fence, bad JSON, or failed validate()
let parsed: AnalysisJson = serde_json::from_str(body).ok()?;
parsed.validate().ok()?;
```

- **Consequence:** `out.analysis` is `None` → **no prediction row is inserted** and
  `analysis_done` carries a null analysis. The streamed prose is still shown and the
  assistant message is still persisted, so the user is never blocked — but the
  self-calibration loop silently misses that turn.
- **Mitigation:** tolerant-by-design (no crash); strict schema + range validation; the
  system prompt mandates the trailing ```json block and a `[0,1]` probability. Consider a
  one-shot "repair" reprompt when parsing fails (planned).
- **Owner:** Backend / Product.

### T5 — CoinGecko rate limits / outages
`coingecko::get_usd_price` hits the **free** `simple/price` endpoint with a fresh
`reqwest::Client` per call and no API key. Used in two places: best-effort initial price
recording (`tokio::spawn` fire-and-forget in `analyze`) and the 60 s resolution loop
(`predictions::resolve_one`).

- **Consequence:** on HTTP error the prediction is marked `outcome_status='failed'`
  (resolution loop) or the initial price is simply not recorded (analyze). Neither path
  crashes; analysis output is unaffected.
- **Mitigation:** failures are isolated and logged; resolution loop `continue`s on error.
  Add request throttling / a shared client / short-TTL price cache and exponential backoff
  before scaling (planned).
- **Owner:** Backend.

### T6 — 12-coin table / unlisted symbols
`coingecko::symbol_to_id` is a hardcoded match over twelve tickers (BTC, ETH, SOL, BNB,
XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC), stripping `USDT`/`USD`/`USDC` suffixes.
Anything else returns `AppError::Invalid("unknown symbol for CoinGecko: <symbol>")`
(`coingecko.rs` line 32: `format!("unknown symbol for CoinGecko: {}", symbol)`).

- **Consequence:** for an unsupported symbol, the *analysis still runs* (price is
  best-effort), but no initial price is recorded and resolution will mark the prediction
  `failed`. Symbol resolution itself falls back to `BTCUSDT` when nothing is extractable.
- **Mitigation:** extend the match table to add coins; surface "unsupported symbol — price
  tracking disabled" in the UI rather than failing silently (planned).
- **Owner:** Backend.

### T7 — Model-download first-run failure
The BGE-small model is downloaded **on first use** of `embed` (the `OnceLock` init in
`ingest::embeddings::get_or_init`). On a constrained PC behind a proxy/firewall, offline,
or with insufficient disk, this download fails — breaking **all** ingestion and therefore
all RAG retrieval (search returns empty, analysis loses its knowledge grounding).

- **Mitigation:** document the one-time network requirement (the integration test note in
  `CLAUDE.md` already flags "downloads BGE-small on first run"); surface a clear actionable
  error instead of a generic `Internal`; pre-warm the model during onboarding (planned).
- **Owner:** Backend / Ops.

---

## Financial / safety risks (detail)

### F1 — Probabilities are asserted, not calibrated
`AnalysisJson.probability_up` comes straight from the model's JSON tail. It is **not**
derived from historical frequency and is **not** calibrated at emission time. The system
prompt asks the model to "be calibrated… not advocacy," but that is a soft instruction.

> **Field-name note.** The model's JSON-tail field `probability_up` (validated to `[0,1]`
> in `AnalysisJson::validate`) is persisted as the `predictions` table column
> `predicted_prob_up` via `predictions::insert_from_analysis` (`predictions.rs` line 34:
> `a.probability_up as f64`). These are the **same value** at different layers — the
> JSON-tail name (`probability_up`) and the DB column / `Prediction` struct field
> (`predicted_prob_up`) are not two different quantities.

- **What does exist:** the **self-evaluation loop**. Every analysis inserts a `prediction`
  (`predictions::insert_from_analysis`) with `predicted_prob_up`, horizon, SL/TP, and
  `outcome_status='pending'`. The 60 s loop resolves due predictions by recording
  `outcome_price_at_horizon` vs. `outcome_price_at_ts`. Over time this yields data to
  *measure* calibration — but the app does not yet feed that back to adjust future
  probabilities, nor surface a calibration curve.
- **Mitigation:** present probabilities as **model estimates**, not guarantees (UI labelling
  is **planned — not yet implemented**, see C1); expose realized-vs-predicted stats once
  enough predictions resolve; never describe output as a derived probability. Aligns with
  the `CLAUDE.md` requirement that probabilities be "derived, not invented" — currently only
  *measurable*, not yet derived.
- **Owner:** Product / Backend.

### F2 — Advisory output mistaken for advice
A confident `"action": "buy"` with a probability and SL/TP reads like a trade
instruction. A user could act on it and lose money on a wrong call.

- **What exists today:** the **advisor-only system prompt** (`llm::prompt::SYSTEM_PROMPT`
  line 25: *"You are an advisor. The user places all trades themselves."*) and the fact
  that **no execution path exists** — the user must manually act on every suggestion. This
  is the **only** safety control currently shipped.
- **Planned — NOT yet implemented:** a persistent, non-dismissible "not financial advice"
  disclaimer in the Chat UI. **No such disclaimer exists in the frontend today** — a search
  of `src/` (`ChatPanel.svelte`, `MessageBubble.svelte`, `routes/+page.svelte`,
  `SettingsView.svelte`, and all components) finds **zero** occurrences of "advice",
  "disclaimer", "estimate", "research/educational", or any warning copy. Do not represent
  the disclaimer as a present control until it ships.
- **Open action item:** add the disclaimer to the Chat panel and to first-run onboarding
  (see C1).
- **Owner:** Product.

### F3 — Resolution ignores intra-horizon SL/TP
`predictions::resolve_one` records only the **price at the horizon timestamp**. It does
**not** simulate whether `stop_loss_pct`/`take_profit_pct` would have been hit *during* the
horizon. Therefore any "hit rate" computed from these rows overstates the performance of a
real SL/TP-bracketed strategy (which would have exited early on an SL touch).

- **Mitigation:** never present resolution stats as backtested P&L; clearly label them as
  "price moved up/down by horizon." Add path-aware resolution using interval OHLC
  (planned). Note this is also why F4's "enforced SL/TP" is **not** satisfied today.
- **Owner:** Backend / Product.

### F4 — Auto-trading (Planned — not yet implemented)
There is **no** order execution, exchange integration (Binance/Kraken), or automated SL/TP
enforcement in the codebase. The original spec (`crypto Dhriti.txt`) envisions these; they
do not exist.

- **Risk *if* added:** a miscalibrated suggestion (F1) or a parse glitch (T4) could trigger
  a real order; without a path-aware engine (F3) SL/TP could be bypassed — a direct
  money-loss and a violation of the spec's non-negotiable safety constraints.
- **Mandatory mitigations before any execution code ships** (per `CLAUDE.md` hard
  constraints): (1) a pre-trade **human-approval gate** that NEVER executes without explicit
  user confirmation of action/qty/limit/SL/TP/rationale; (2) **enforced** SL/TP with no
  bypass path; (3) position sizing and max-drawdown protection. Until then this row stays
  open and N/A.
- **Owner:** Product / Backend.

---

## Operational risks (detail)

### O1 — API key handling
The Anthropic API key is stored in the **OS keyring** (`keyring` crate, service
`cognitrade`, user `anthropic_api_key`) — on Windows this is the **Credential Manager** —
and **never** in the DB (`settings.rs`). It is read on demand via the internal helper
`require_api_key()` and sent as the `x-api-key` header over HTTPS (rustls-TLS) to
`api.anthropic.com`.

- **IPC surface vs. internal helpers.** `require_api_key()` (`settings.rs` line 90) is an
  **internal** helper — it is **not** a `#[tauri::command]`. The only key-related IPC
  commands exposed to the frontend are:
  - `set_api_key(value: String)` (`ipc.rs` line ~92),
  - `clear_api_key()` (`ipc.rs` line 99),
  - `get_api_key_set() -> bool` (`ipc.rs` line 88).

  The frontend **never** receives the key back — it only ever checks
  `get_api_key_set() -> bool`.
- **Residual exposure:** the key transits the IPC boundary in `set_api_key(value: String)`
  and lives in process memory while a request is in flight; a malicious local process or a
  memory dump could read it. Errors carry no key material (`AppError` variants are
  string/enum, never embed the header).
- **Mitigation:** keyring storage; `clear_api_key` to revoke; never log the key; TLS for
  all calls; advise users to scope/rotate the key.
- **Owner:** Backend / Ops.

### O2 — Cost runaway vs. the $2 cap
`analyze_with_cap` (`llm::client`) checks the daily cap **before** the main streaming call:

```rust
let today = cost::today(db)?;
if cost::is_over_cap(today.cost_usd, s.daily_cost_cap_usd) {
    return Err(AppError::CostCapReached);
}
```

Cap semantics and gaps:
- The cap (`settings.daily_cost_cap_usd`, default **$2**) is a **pre-call** gate, not a
  mid-stream kill switch. A single `analyze` (`max_tokens: 1500`) can push spend *over* the
  cap; the *next* call is then blocked. So the effective ceiling is roughly cap + one
  call's cost.
- **Helper Haiku calls bypass the gate.** `extract_symbol_from_image` (vision symbol
  extraction) and `ingest_image` (image description) call Anthropic **directly via
  `reqwest`**, not through `analyze_with_cap`. Their token usage is **not** added to
  `cost_daily` and they run even after the cap is hit. Bulk image ingestion is the main
  uncapped-spend path.
- **Pricing is a substring match with a silent Sonnet fallback.** `cost::rates_for`
  (`cost.rs` lines 15–17) matches by `model.contains(...)`: `"opus"` → Opus rates,
  `"haiku"` → Haiku rates, **else → Sonnet rates**. Current rates: Sonnet 4.6
  $3/$0.30/$15, Opus 4.7 $15/$1.50/$75, Haiku 4.5 $1/$0.10/$5 per Mtok
  (input/cached/output). Because the fallback arm has no error path, **any** model ID that
  contains neither `opus` nor `haiku` — a future, renamed, or mistyped model — is **silently
  billed at Sonnet rates**. So cost accounting can be silently *wrong* (not merely stale) if
  a model ID changes, with no warning emitted.
- **Mitigation:** prompt caching (`cache_control: ephemeral` on system prompt + retrieved
  block) cuts repeat input cost; `cost_today` IPC surfaces spend in Settings; document that
  the cap covers only the main analysis call. Route Haiku helper calls through cost
  accounting + the cap, and make `rates_for` fail loudly (or warn) on an unmatched model ID
  instead of silently defaulting to Sonnet (planned).
- **Owner:** Backend / Ops.

### O3 — 24/7 stability
`prune::spawn_background` launches two infinite Tokio loops (chat prune every 6 h for
messages older than 7 days; prediction resolution every 60 s). `ingest::watcher` runs an
infinite `notify` loop with an 800 ms debounce. These run for the lifetime of the app.

- **Resilience already present:** both loops swallow per-iteration errors (`Err(_) =>
  continue` / `let _ = …`) and keep looping; the watcher only acts after debounce and skips
  non-files. Logs go to `%USERPROFILE%\CogniTrade\logs` (tracing).
- **Residual risk:** a panic inside a spawned task, or a poisoned `Mutex` (a panic while a
  DB lock is held), could wedge a subsystem without taking down the UI — failing silently.
- **Mitigation:** keep critical sections panic-free; consider catching/`unwrap_or` on lock
  poisoning; expose a tray "restart" affordance; monitor log growth. Restart via the system
  tray (`tray.rs`) if a loop wedges.
- **Owner:** Backend / Ops.

### O4 — Watcher silently skips images without a key
`ingest::watcher::run_watcher` ingests PDFs with no key, but for images it does
`match crate::settings::require_api_key() { Ok(k) => k, Err(_) => return }` — i.e. it
**silently returns** (no log, no UI notice) when no key is set. A user dropping chart
images into the library before configuring a key will see nothing happen.

- **Mitigation:** intentional behavior (images need a Haiku description call), but should
  emit a log line and a UI hint "image not ingested — set an API key" (planned). PDFs are
  unaffected.
- **Owner:** Backend / Product.

### O5 — Global hotkey collision
`lib.rs run()` registers a global shortcut to toggle the panel
(`tauri_plugin_global_shortcut`). The default is `Ctrl+Shift+Space` (`settings.rs` line 25:
`hotkey: "Ctrl+Shift+Space".into()`). On Windows this can collide with another application's
global shortcut, in which case CogniTrade's toggle hotkey may not fire (or the other app's
may not).

- **Mitigation:** the hotkey is **user-configurable** via `settings.hotkey` — change it in
  Settings if it conflicts. Registration is already tolerant of an "already registered"
  state from a prior crashed run (`lib.rs` lines 51–62, logs and continues rather than
  panicking). Document the default and the rebind path in onboarding.
- **Owner:** Backend / Ops.

### O6 — Windows installer bundle targets not pinned
`tauri.conf.json` sets `bundle.targets = "all"` (line 36). This produces **whatever
bundlers are available on the build host**, rather than an explicit, reproducible set. The
Windows installer assumptions elsewhere in CogniTrade docs (MSI/WiX, NSIS) are therefore not
guaranteed by config — the produced installer set varies with the build environment.

- **Mitigation:** pin `bundle.targets` to an explicit `["msi", "nsis"]` (or just `["msi"]`)
  if a specific Windows installer is required for release. Building an MSI still requires the
  WiX toolset and the Rust MSVC toolchain on the build host.
- **Owner:** Ops.

---

## Compliance / legal note

### C1 — Not financial advice
CogniTrade produces opinions about crypto charts. Depending on jurisdiction, presenting
buy/sell suggestions with probabilities and stop-loss/take-profit levels could be construed
as **regulated investment advice**, even though the app is advisory-only and executes
nothing.

- **What protects the user today:** **only** the advisor-only system prompt
  (`llm::prompt::SYSTEM_PROMPT` line 25: *"You are an advisor. The user places all trades
  themselves."*). There is **no** UI-level "not financial advice" disclaimer — neither at
  first run nor in the Chat panel — anywhere in the frontend (`src/`). Residual legal
  exposure is therefore **higher than it would be with a shipped disclaimer**.
- **Position to hold:** CogniTrade is intended as a **research/educational tool**, not a
  financial adviser, broker, or portfolio manager. It does not execute trades, custody
  funds, or guarantee outcomes. The user makes and places all trading decisions themselves.
- **Required mitigations (Planned — NOT yet implemented):**
  - Add a prominent, persistent **"Not financial advice — for research/educational purposes
    only"** disclaimer in the Chat UI **and** at first run. (Open action item; tracked in
    F2.)
  - Make clear that probabilities are **model estimates** (see F1), not guarantees, and
    that resolution stats are **not** backtested P&L (see F3).
  - Re-evaluate this note before adding any execution feature (F4), which would materially
    change the regulatory posture.
- **Owner:** Product.

---

## Review cadence

Re-review this register when any of the following change: the vector store (T1/T3 — and the
`lance.rs` / `LanceStore` / `Paths::lancedb()` naming if migrated), the embedding model or
hardware target (T2/T7), the model IDs or cost rates / `rates_for` fallback (O2), the
CoinGecko integration (T5/T6), the global hotkey or bundle targets (O5/O6), whether the
**"not financial advice" disclaimer ships** (F2/C1), or — critically — if any
**execution/auto-trading** capability is proposed (F4 + C1 must be revisited before such
code is written).
