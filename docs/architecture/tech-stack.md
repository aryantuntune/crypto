# Tech Stack & Dependencies

This document enumerates and justifies the technology choices for **CogniTrade**, an
advisory crypto-chart analysis desktop app. It is the authoritative reference for *what*
we depend on and *why* each pin was chosen. Versions below are taken verbatim from
[`src-tauri/Cargo.toml`](../../src-tauri/Cargo.toml) and
[`package.json`](../../package.json).

> **Scope reminder.** CogniTrade is an **advisory** chart analyst. It has **no exchange
> integration, no order execution, no automated stop-loss/take-profit enforcement, and no
> Minimax engine.** The system prompt tells the model: *"You are an advisor. The user
> places all trades themselves."* Anything in the original spec
> ([`crypto Dhriti.txt`](../../crypto%20Dhriti.txt)) describing autonomous trading,
> Binance/Kraken integration, or a Minimax decision engine is **Planned — not yet
> implemented** and is not reflected anywhere in the dependency set below.

---

## 1. Platform shape

| Layer    | Technology                                                                 |
| -------- | -------------------------------------------------------------------------- |
| Shell    | Tauri 2 (system WebView2, Rust core)                                        |
| Backend  | Rust (edition 2021), async on Tokio                                        |
| Frontend | SvelteKit (Svelte 5) + TypeScript, Tailwind 3, built by Vite 6 as a static SPA |
| Storage  | Two local SQLite files (`data/chat.db`, `data/vec.db`), WAL mode           |
| Secrets  | OS keyring (Windows Credential Manager)                                     |
| LLM      | Anthropic Messages API over HTTPS (streaming SSE)                          |
| Market data | CoinGecko free `simple/price` endpoint                                  |

Primary target is **Windows** (Ryzen 3400G, 8 GB RAM, 1 TB storage, 450 W PSU). The 8 GB
RAM ceiling is a hard constraint and is the single biggest driver of the choices on this
page. See [hardware-budget.md](./hardware-budget.md) for the budget breakdown.

---

## 2. Backend crates (`src-tauri/Cargo.toml`)

### 2.1 Runtime dependencies

| Crate | Version (pin) | Features | Role in CogniTrade |
| ----- | ------------- | -------- | ------------------ |
| `tauri` | `2` | `tray-icon` | App shell, window management, IPC `#[tauri::command]` bridge, system tray. See [`lib.rs`](../../src-tauri/src/lib.rs). |
| `tauri-plugin-opener` | `2` | — | Open files/URLs (e.g. the library folder) in the OS default handler. |
| `tauri-plugin-global-shortcut` | `2` | — | Registers `Ctrl+Shift+Space` to toggle the panel ([`lib.rs`](../../src-tauri/src/lib.rs)). |
| `tauri-plugin-dialog` | `2` | — | Native file pickers for PDF/image ingestion from the Library tab. |
| `tauri-plugin-fs` | `2` | — | Filesystem access for the frontend. Enabled via plugin **registration** in [`lib.rs`](../../src-tauri/src/lib.rs) (`Builder::plugin(...)`), **not** via per-permission capability scoping — see the note below. |
| `serde` | `1` | `derive` | (De)serialization of IPC payloads, settings, `AnalysisJson`. |
| `serde_json` | `1` | — | JSON for Anthropic requests, SSE deltas, and the trailing `json` tail parsed by `extract_json_tail`. |
| `tokio` | `1` | `full` | Async runtime: streaming HTTP, background prune/resolve loops, the library watcher. |
| `rusqlite` | `0.31` | `bundled` | Both SQLite databases. **`bundled`** compiles SQLite from source — no system SQLite dependency. See §3.2. |
| `keyring` | `2` | — | Stores the Anthropic API key in the OS credential store. **Never** in any DB. See §3.4. |
| `reqwest` | `0.12` | `json`, `rustls-tls`, `stream` (`default-features = false`) | HTTP client for Anthropic + CoinGecko. **rustls — no OpenSSL.** See §3.1. |
| `thiserror` | `1` | — | Derives the single `AppError` enum ([`error.rs`](../../src-tauri/src/error.rs)). |
| `anyhow` | `1` | — | Ergonomic error propagation in non-IPC internal paths. |
| `chrono` | `0.4` | `serde` | Timestamps for messages, predictions, prune horizons, daily cost rollups. |
| `uuid` | `1` | `v4` | Primary keys for messages and `doc_chunks` rows. |
| `sha2` | `0.10` | — | Backs `ingest::file_hash()` (SHA-256 of a file's bytes). **Currently unused** — see note below. |
| `hex` | `0.4` | — | Hex-encodes the digest produced by `file_hash()`. Unused for the same reason. |
| `notify` | `6` | — | Watches the library directory; auto-ingests dropped files with an 800 ms debounce ([`ingest/watcher.rs`](../../src-tauri/src/ingest/watcher.rs)). |
| `pdf-extract` | `0.7` | — | Pure-Rust PDF text extraction in `ingest_pdf`. See risk note §6. |
| `image` | `0.25` | — | Decode/normalize screenshots and ingested images ([`image_util.rs`](../../src-tauri/src/image_util.rs)). |
| `fastembed` | `4` | — | Local CPU embeddings via BGE-small (384-dim). See §3.3. |
| `futures` | `0.3` | — | Stream combinators for the SSE byte stream. |
| `async-trait` | `0.1` | — | Async methods in trait abstractions (e.g. the LLM client). |
| `tracing` | `0.1` | — | Structured logging. |
| `tracing-subscriber` | `0.3` | `env-filter` | Log formatting + `RUST_LOG`-style filtering, written under `~/CogniTrade/logs`. |
| `dirs` | `5` | — | Resolves `%USERPROFILE%` to build the `~/CogniTrade/{library,screenshots,data,logs}` tree ([`paths.rs`](../../src-tauri/src/paths.rs)). |
| `base64` | `0.22` | — | Base64-encodes images for the Anthropic vision payload. |

> **`sha2` / `hex` are present but not yet wired up.** The only consumer is
> `fn file_hash()` in [`ingest/mod.rs`](../../src-tauri/src/ingest/mod.rs), which computes a
> SHA-256 hex digest of a file. That function is **not called anywhere** in the codebase —
> there is currently **no dedup or change-detection** logic. Content-hash-based dedup of
> ingested files is **Planned — not yet implemented**.

> **`tauri-plugin-fs` is not capability-scoped today.** The current
> [`capabilities/default.json`](../../src-tauri/capabilities/default.json) grants only
> `core:default` and `opener:default`. It contains **no** `fs:`, `dialog:`, or
> `global-shortcut:` permissions. The fs / dialog / global-shortcut plugins are made
> available purely by being **registered** in [`lib.rs`](../../src-tauri/src/lib.rs); they
> are **not** restricted by per-permission entries in that capability file. Tightening the
> capability set is **Planned — not yet implemented**.

### 2.2 Build dependency

| Crate | Version | Role |
| ----- | ------- | ---- |
| `tauri-build` | `2` | Generates Tauri context/codegen at build time (`build.rs`). |

### 2.3 Dev dependencies

| Crate | Version | Role |
| ----- | ------- | ---- |
| `tempfile` | `3` | Throwaway dirs/DBs in unit and integration tests. |
| `wiremock` | `0.6` | Mocks the Anthropic API in `tests/llm_integration.rs`. |
| `tokio` | `1` (`full`, `test-util`) | Async test harness + paused-time helpers. |
| `pretty_assertions` | `1` | Readable diffs in assertions. |

**Both** integration tests in [`src-tauri/tests/`](../../src-tauri/tests/) —
`ingest_integration.rs` and `llm_integration.rs` — are marked `#[ignore]`. A bare
`cargo test` runs **only** unit + non-ignored tests and **silently skips both** of them.
Run each explicitly with `-- --ignored`:

```powershell
cd src-tauri
cargo test                                                       # unit + non-ignored only (skips BOTH integration tests)
cargo test --test ingest_integration -- --ignored --nocapture    # downloads BGE-small on first run
cargo test --test llm_integration   -- --ignored --nocapture     # wiremock-backed LLM streaming test
```

---

## 3. Key decisions & justifications

### 3.1 `rustls-tls` — no OpenSSL

`reqwest` is declared with `default-features = false` and the explicit feature set
`["json", "rustls-tls", "stream"]`. This is deliberate:

- **No OpenSSL build dependency.** OpenSSL via `native-tls` would require either a system
  OpenSSL install or the `vendored` C build on Windows, complicating the MSVC toolchain
  and CI. rustls is pure Rust and links cleanly under MSVC.
- **`stream`** is required because [`llm/streaming.rs`](../../src-tauri/src/llm/streaming.rs)
  consumes the Anthropic response as a byte stream and parses SSE incrementally, forwarding
  text deltas to the frontend via the `analysis_chunk` Tauri event.
- **`json`** covers CoinGecko and non-streaming request/response bodies.

Disabling default features also trims the dependency tree — relevant to keeping
compile/link footprint modest on the target hardware.

### 3.2 `rusqlite` bundled — and why SQLite over a server DB

`rusqlite` uses the **`bundled`** feature, so SQLite is compiled into the binary. There is
no external database process, no connection string, no service to install or keep alive.

Why SQLite (not Postgres/MySQL/etc.):

- **Single-user desktop app.** There is exactly one local user and one process. A
  client/server RDBMS would add an idle daemon, memory overhead, and an install step — all
  hostile to the 8 GB budget and the "just works after MSI install" goal.
- **Embedded + zero-ops.** `bundled` means no version-matching against a system SQLite.
- **WAL mode** (`PRAGMA journal_mode=WAL`) gives concurrent readers alongside the
  background prune/resolve writers without locking the UI.

CogniTrade uses **two** SQLite files:

| File | Opened by | Tables |
| ---- | --------- | ------ |
| `data/chat.db` | [`db.rs`](../../src-tauri/src/db.rs) (`db::open`) | `messages`, `predictions`, `cost_daily`, `settings` — see [`migrations/001_initial.sql`](../../src-tauri/migrations/001_initial.sql) |
| `data/vec.db`  | [`lance.rs`](../../src-tauri/src/lance.rs) (`lance::open`) | `doc_chunks` (embeddings as BLOBs) |

> **Naming caveat:** the vector store module is named `lance.rs` / `LanceStore` and the
> path helper is `paths::lancedb()`, but **there is no LanceDB dependency**. It is plain
> `rusqlite`. See §6.

### 3.3 `fastembed` + BGE-small — local, CPU-only embeddings

RAG retrieval needs an embedding model. [`ingest/embeddings.rs`](../../src-tauri/src/ingest/embeddings.rs)
uses `fastembed` with `EmbeddingModel::BGESmallENV15`, producing **384-dimensional**
vectors (`EMBED_DIM = 384`, validated on every insert in `LanceStore::insert`).

Why this choice on a 8 GB / no-GPU machine:

- **BGE-small is tiny.** A ~33M-parameter ONNX model runs comfortably on CPU within the
  RAM budget. A larger embedder (or a local LLM-based embedder) would blow the budget.
- **Local & offline after first download.** The model is a lazily-initialized global
  singleton — `static MODEL: OnceLock<Mutex<TextEmbedding>>` — downloaded on first use,
  then reused. No per-call provider cost, no network round-trip per chunk.
- **Synchronous behind a `Mutex`.** Embedding is CPU-bound and serialized, which keeps peak
  memory predictable (one inference at a time) rather than fanning out parallel inferences
  that would spike RAM.

The vector dimension is pinned at 384 throughout the store, but the two code paths handle a
**dimension mismatch differently**:

- **Write path — `LanceStore::insert()`** rejects any embedding whose length `!= EMBED_DIM`
  with **`AppError::Invalid`** (a hard error).
- **Read path — `LanceStore::search()`** does **not** error: if `query_emb.len() != EMBED_DIM`
  it **silently returns an empty `Vec`** (no results), short-circuiting before it scans the
  table.

### 3.4 `keyring` — secrets in the OS credential store

The Anthropic API key is held in the OS keyring (`service = "cognitrade"`,
`user = "anthropic_api_key"`), **never** in `chat.db` or any config file. On Windows this
maps to **Windows Credential Manager**. Non-secret settings (model ids, daily cost cap,
etc.) live in the `settings` table; only the key is privileged.

### 3.5 `cost.rs` — model rate card and the daily spend cap

[`cost.rs`](../../src-tauri/src/cost.rs) tracks token usage per calendar day in the
`cost_daily` table and enforces a hard daily spend ceiling before each main analysis call.

**Rate selection — `rates_for(model: &str)`** picks a rate card by **substring match** on
the model id (not an exact id lookup):

| Substring matched | Model (as documented in `settings.rs`) | Input | Cached input | Output |
| ----------------- | --------------------------------------- | ----- | ------------ | ------ |
| contains `opus`   | Opus 4.7 (`claude-opus-4-7`)            | $15.00 | $1.50  | $75.00 |
| contains `haiku`  | Haiku 4.5 (`claude-haiku-4-5-20251001`) | $1.00  | $0.10  | $5.00  |
| anything else     | Sonnet 4.6 (`claude-sonnet-4-6`, the **default** `model_main`) | $3.00 | $0.30 | $15.00 |

Rates are per **million tokens** (`cost_usd()` divides by `1_000_000`). The `else` branch
means any unrecognized id is billed at **Sonnet** rates by default.

**Cap enforcement — `analyze_with_cap()`** ([`llm/client.rs`](../../src-tauri/src/llm/client.rs))
reads today's accumulated `cost_usd` and, *before* issuing the main Anthropic call, checks
it against `settings.daily_cost_cap_usd` (**default `$2.00`**, see [`settings.rs`](../../src-tauri/src/settings.rs)).
If today's spend is already over the cap it returns **`AppError::CostCapReached`** and the
analysis is refused. This is a pre-call guard, so a single in-flight call can still push the
running total slightly past the cap; it bounds *how often* analyses start, not the exact
total.

### 3.6 CoinGecko for market data

[`coingecko.rs`](../../src-tauri/src/coingecko.rs) calls the **free**
`api.coingecko.com/.../simple/price` endpoint. It is used to (a) record the
price-at-prediction during `analyze`, and (b) resolve predictions at their horizon in the
background loop.

Why CoinGecko (vs. an exchange API):

- **No API key, no account, no order-book scope.** CogniTrade is advisory and never
  trades, so it only needs a spot USD price. CoinGecko's free tier covers that.
- **No exchange integration by design.** Binance/Kraken integration is **Planned — not yet
  implemented**; pulling in an exchange SDK would imply order capabilities we explicitly do
  not have.

Limitation: the symbol→id table is **hardcoded to ~12 coins** (BTC, ETH, SOL, BNB, XRP,
ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC). It strips `USDT`/`USD`/`USDC` suffixes; any
unlisted symbol returns `AppError::Invalid`.

### 3.7 Tauri 2 over Electron — the memory argument

CogniTrade ships as a **Tauri 2** app, not Electron. On an 8 GB machine running 24/7 this
is decisive:

| | Tauri 2 | Electron |
| --- | --- | --- |
| Renderer | System **WebView2** (shared with OS) | Bundles its own Chromium |
| Backend | Native Rust, no Node runtime | Node.js main process |
| Idle RAM | Low (no embedded browser) | High (Chromium + Node baseline) |
| Installer size | Small | Large (Chromium payload) |

Bundling Chromium *and* a Node runtime per Electron app would consume a large, fixed slice
of the 8 GB budget before CogniTrade does any work (embedding model, SQLite caches,
in-flight LLM streams). Tauri reuses the OS WebView2 and runs business logic in Rust, which
also lets the embedding/vector/HTTP work live in one efficient process. See
[hardware-budget.md](./hardware-budget.md).

---

## 4. Frontend dependencies (`package.json`)

The UI is a **static SPA**: SvelteKit with `@sveltejs/adapter-static`, built by Vite to
`../build`, which Tauri serves as `frontendDist`. The shell is a 3-tab panel
(Chat / Library / Settings) — see [`routes/+page.svelte`](../../src/routes/+page.svelte) and
the chat store in [`lib/stores/chat.ts`](../../src/lib/stores/chat.ts) that listens for the
`analysis_chunk` / `analysis_done` Tauri events.

### 4.1 Runtime dependencies

| Package | Version | Role |
| ------- | ------- | ---- |
| `@tauri-apps/api` | `^2` | Core IPC: `invoke`, event listeners — wrapped in [`lib/ipc.ts`](../../src/lib/ipc.ts). |
| `@tauri-apps/plugin-dialog` | `^2.7.1` | JS side of the native file picker. |
| `@tauri-apps/plugin-fs` | `^2.5.1` | JS side of the filesystem plugin. |
| `@tauri-apps/plugin-global-shortcut` | `^2.3.1` | JS side of the global hotkey plugin. |
| `@tauri-apps/plugin-opener` | `^2` | JS side of the opener plugin. |

The Tauri plugin versions are paired with their Rust counterparts in §2.1.

### 4.2 Dev dependencies

| Package | Version | Role |
| ------- | ------- | ---- |
| `@sveltejs/kit` | `^2.9.0` | App framework / routing. |
| `svelte` | `^5.0.0` | **Svelte 5** (runes). |
| `@sveltejs/adapter-static` | `^3.0.6` | Builds a static SPA — required for embedding in Tauri. |
| `@sveltejs/vite-plugin-svelte` | `^5.0.0` | Svelte integration for Vite. |
| `vite` | `^6.0.3` | Dev server (port 1420) + production build. |
| `@tauri-apps/cli` | `^2` | `npm run tauri dev` / `build`. |
| `typescript` | `~5.6.2` | Typed IPC wrappers ([`lib/types.ts`](../../src/lib/types.ts)). |
| `svelte-check` | `^4.0.0` | Type/lint check via `npm run check`. |
| `tailwindcss` | `^3.4.19` | Utility-first styling (**Tailwind 3**). |
| `autoprefixer` | `^10.5.0` | PostCSS vendor prefixing. |
| `postcss` | `^8.5.14` | CSS pipeline for Tailwind. |

Config: [`svelte.config.js`](../../svelte.config.js) (static adapter),
[`vite.config.js`](../../vite.config.js) (port 1420, Tauri-aware),
[`tsconfig.json`](../../tsconfig.json).

---

## 5. Build tooling & toolchain matrix

CogniTrade builds on **Windows** as the primary platform and ships an MSI/NSIS installer
via Tauri's bundler (`bundle.targets = "all"` in
[`tauri.conf.json`](../../src-tauri/tauri.conf.json)).

### 5.1 Commands

```powershell
npm install                # install JS deps (once after clone)
npm run tauri dev          # dev build, hot reload (Vite on :1420 + cargo)
npm run tauri build        # release MSI (WiX) / NSIS installer
npm run check              # svelte-check (sync + type check)

cd src-tauri
cargo test                 # backend unit + non-ignored tests (BOTH integration tests are SKIPPED)
cargo test --test ingest_integration -- --ignored --nocapture   # full ingest smoke test (downloads BGE-small on first run)
cargo test --test llm_integration   -- --ignored --nocapture    # wiremock-backed LLM streaming test
```

> Both `tests/ingest_integration.rs` and `tests/llm_integration.rs` are `#[ignore]`d, so a
> bare `cargo test` will not exercise them — pass `-- --ignored` per the commands above.

> This repository is **not** a git repository. `git init` is required before any
> version-control operations.

### 5.2 Toolchain matrix (Windows primary)

| Component | Requirement | Why |
| --------- | ----------- | --- |
| Rust toolchain | **`stable-x86_64-pc-windows-msvc`** | Tauri on Windows links against MSVC; `rusqlite` bundled and `fastembed` native deps need the MSVC C/C++ toolchain. |
| Visual Studio C++ Build Tools | Installed (MSVC + Windows SDK) | C compiler/linker for `rusqlite bundled` and other `-sys`/native crates. |
| WebView2 Runtime | Installed (evergreen) | Tauri renders the SvelteKit UI in the system WebView2. |
| Node.js | LTS (supports Vite 6 / SvelteKit 2) | Builds the frontend; `@tauri-apps/cli` orchestrates dev/build. |
| Edition | Rust 2021 | Per `Cargo.toml`. |

First `cargo test`/run that touches embeddings will **download BGE-small** over the
network before any embedding succeeds.

---

## 6. Dependency-risk notes

These are known sharp edges in the current dependency set. They are not blockers, but
maintainers must keep them in mind.

1. **`lance` is a misnomer — there is no LanceDB.**
   The module [`lance.rs`](../../src-tauri/src/lance.rs), the type `LanceStore`, and
   `paths::lancedb()` all *sound* like they wrap LanceDB. They do not. The store is plain
   `rusqlite` with embeddings serialized as **little-endian `f32` BLOBs**, and
   `LanceStore::search()` performs a **full table scan**: it loads *every* `doc_chunks` row
   and computes cosine similarity in Rust on each query (`O(n·d)` per search, single-threaded
   under a `Mutex`). This is fine for a small personal library but will not scale to large
   corpora, and the naming will mislead anyone expecting an ANN index. *Risk:* silent
   performance cliff and confusing onboarding. *Mitigation:* rename, or introduce a real
   ANN index if the library grows.

2. **`pdf-extract 0.7` maturity.**
   `pdf-extract` at `0.7` is a pre-1.0, text-only extractor. It can return empty/garbled
   text on scanned PDFs (no OCR), unusual encodings, or heavily-laid-out documents. Because
   ingest chunks and embeds whatever text it returns, extraction failures degrade RAG
   silently rather than loudly. *Mitigation:* validate non-empty extraction and surface a
   warning when a PDF yields little/no text.

3. **`fastembed` first-run network dependency.**
   The BGE-small model is downloaded on first use. A machine that is offline on first run
   (or behind a proxy) will fail embedding/ingestion with `AppError::Internal("fastembed
   init: …")`. *Mitigation:* document the first-run download; consider pre-seeding the model
   cache in the installer.

4. **Hardcoded CoinGecko coin table.**
   Only ~12 symbols resolve ([`coingecko.rs`](../../src-tauri/src/coingecko.rs)); everything
   else errors. Adding coins is a code change, not config. The free endpoint is also
   rate-limited, which the 60 s prediction-resolution loop must respect.

5. **Broad version pins (`"1"`, `"2"`).**
   Several crates are pinned only to a major (`serde = "1"`, `tauri = "2"`). Cargo will pull
   the newest compatible minor/patch, so builds are not fully reproducible without a
   committed `Cargo.lock`. *Mitigation:* commit and rely on `Cargo.lock` for reproducible
   release builds.

6. **`sha2` / `hex` carried but unused.**
   These crates back `ingest::file_hash()`, which has **no callers**. The intended use
   (content-hash dedup / change detection of ingested files) is **Planned — not yet
   implemented**. *Mitigation:* either wire up dedup or drop the dependencies until needed.

7. **`capabilities/default.json` is not least-privilege.**
   The fs / dialog / global-shortcut plugins are reachable because they are registered in
   `lib.rs`, but `default.json` only lists `core:default` and `opener:default` — it does
   **not** scope those plugins per-permission. Hardening the capability set is **Planned —
   not yet implemented**.

---

## See also

- [hardware-budget.md](./hardware-budget.md) — the 8 GB RAM budget that drives these choices.
- [`migrations/001_initial.sql`](../../src-tauri/migrations/001_initial.sql) — `chat.db` schema.
- [`crypto Dhriti.txt`](../../crypto%20Dhriti.txt) — original spec (aspirational; diverges from the implemented advisory app).
</content>
</invoke>
