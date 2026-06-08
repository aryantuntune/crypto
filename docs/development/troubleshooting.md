# Troubleshooting

Fixes for the most common build, run, and runtime problems with **CogniTrade** — the
Tauri 2 advisory crypto-chart analyst (Rust backend + SvelteKit/TypeScript frontend).

> **Scope reminder.** CogniTrade is an **advisor**, not an autonomous trader. The single
> analysis path (`ipc::analyze`) reads a chart screenshot with Claude vision, runs RAG over
> ingested PDFs/images, streams a plain-English answer, and parses a trailing JSON
> recommendation. There is **no exchange integration, no order execution, and no automated
> stop-loss/take-profit enforcement** in the code. Symptoms below describe what the code
> *actually* does. Anything marked **"Planned — not yet implemented"** does not exist in the
> current source.

Related docs:

- [Windows Developer Setup](./windows-setup.md) — toolchain prerequisites.
- [Build & Release](./build-and-release.md) — dev/build commands, MSI/NSIS packaging.
- [Tech Stack](../architecture/tech-stack.md) — dependency rationale.

Windows is the primary platform; examples use PowerShell/`cmd` paths
(`%USERPROFILE%\CogniTrade`) and the MSVC toolchain.

---

## 1. Build problems

These occur during `npm run tauri dev` or `npm run tauri build` (which both trigger
`cargo build` in `src-tauri/`).

| Symptom | Cause | Fix |
| --- | --- | --- |
| `error: linker 'link.exe' not found` (or `error: Microsoft Visual C++ ... not found`) during the Rust compile | The MSVC linker is missing. The `bundled` `rusqlite`, `fastembed`, and other native crates need a C/C++ toolchain. The default Rust target on Windows is `x86_64-pc-windows-msvc`. | Install **Visual Studio C++ Build Tools** with the *Desktop development with C++* workload (provides `link.exe`). Then confirm with `rustup default stable-x86_64-pc-windows-msvc` and re-run from a fresh **Developer PowerShell** so the MSVC environment is on `PATH`. See [Windows Setup](./windows-setup.md). |
| App builds but fails to launch with a WebView2 / "WebView2 runtime not found" error, or a blank window | Tauri 2 renders the SvelteKit SPA inside the system **WebView2** runtime; it is not bundled by default in dev. | Install the **Microsoft Edge WebView2 Runtime** (Evergreen Bootstrapper). On most Windows 11 machines it is already present. The MSI/NSIS installer produced by `npm run tauri build` can bootstrap it for end users, but `npm run tauri dev` requires it to be installed manually. |
| Confusion over TLS: errors mentioning OpenSSL, or `openssl-sys` failing to find OpenSSL, or unexpected linkage to a system TLS library | CogniTrade pins `reqwest` to **rustls**, not OpenSSL. In `src-tauri/Cargo.toml`: `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }`. There is **no** OpenSSL dependency. | If a build error mentions OpenSSL, a *transitive* dependency pulled in `native-tls`/`openssl-sys` — do not install OpenSSL to "fix" it. Run `cargo tree -i openssl-sys` (and `-i native-tls`) in `src-tauri/` to find the offender, and ensure no feature re-enabled `default-features` on `reqwest`. The intended config needs no system TLS libraries. |
| First `cargo build` takes many minutes (sometimes 10+) and looks "stuck" | Large native dependency tree compiled from source: `tauri`, `rusqlite` (`bundled` — compiles SQLite from C), `fastembed` (ONNX/embedding stack), `pdf-extract`, `image`. This is normal on the first build and on the **Ryzen 3400G / 8 GB RAM** hardware target. | Let it finish — it is not hung. Subsequent builds are incremental and fast. To watch progress, build the backend directly: `cd src-tauri && cargo build`. Avoid running other RAM-heavy apps; the 8 GB ceiling is a hard constraint and the linker step is memory-hungry. |

---

## 2. Runtime problems

These occur after the app launches.

| Symptom | Cause | Fix |
| --- | --- | --- |
| First **ingest** of an image, or the symbol-extraction step of **analyze**, hangs for tens of seconds, then either succeeds or errors with `internal: fastembed init: ...` | The BGE-small embedding model (`fastembed`, `BGESmallENV15`) is a lazy global singleton (`OnceLock<Mutex<TextEmbedding>>` in `ingest/embeddings.rs`). On first use it **downloads the model from the network** and runs CPU-only behind a mutex. The first call blocks while it downloads. | This is expected on first run. Ensure outbound network access is allowed. If the download fails (`internal: fastembed init`/`fastembed embed`), retry — the model is fetched again on the next call. The model is CPU-only and serialized by the mutex, so concurrent ingests do not parallelize. |
| Saving or reading the API key fails with `keyring error: ...` (e.g. access denied / no backend) | Anthropic key is stored in the OS keyring (`keyring` crate, service `cognitrade`, user `anthropic_api_key`) — on Windows this is **Credential Manager**. Access can be denied by Group Policy, a locked credential store, or a non-interactive session. | Open **Control Panel → Credential Manager → Windows Credentials** and confirm you can read/write. Re-enter the key via the Settings tab (`set_api_key`). If a stale/corrupt entry exists, clear it (`clear_api_key`) and re-add. The key is **never** stored in the DB — clearing the SQLite files will not help. |
| Log line on startup: `global shortcut registration (may already be registered): ...`, and **Ctrl+Shift+Space** does not toggle the panel | The global hotkey `Ctrl+Shift+Space` (`lib.rs`, `Modifiers::CONTROL | SHIFT`, `Code::Space`) is already held by another process — commonly a **prior CogniTrade instance that crashed without releasing it**, or another app that grabbed the same chord. | Registration failure is logged as a `warn` and the app keeps running (it is tolerant by design), but the hotkey will not work until the chord is free. Fully exit any lingering CogniTrade process (check the **system tray** and Task Manager), and quit any other app bound to Ctrl+Shift+Space, then restart. Use the **tray icon** to toggle/show the panel in the meantime. |
| An analyze call fails with `api key not set` | No key in the keyring. `analyze` ultimately calls Anthropic via `analyze_with_cap`, which calls `settings::require_api_key()` (returning `AppError::NoApiKey`) — but only **after** first checking the daily cost cap (`client.rs`: cap check precedes the key check), so a user at the cap sees `CostCapReached` instead. When there is **no** `symbol_hint` but a screenshot is supplied, the Haiku symbol-extraction step (`ipc.rs`) also calls `require_api_key()` earlier in the flow; with a non-empty `symbol_hint`, resolution does not require the key. | Set the Anthropic API key in the **Settings** tab. |

> The configured hotkey string in Settings (`settings.hotkey`, default `"Ctrl+Shift+Space"`)
> is stored but the actual registered chord is **hardcoded** in `lib.rs`. Changing the
> setting does **not** re-bind the hotkey. Rebindable hotkeys are **Planned — not yet
> implemented**.

---

## 3. Functional problems (analysis & RAG behavior)

These are not crashes — they are surprising-but-correct behaviors of the analysis pipeline,
or recoverable errors surfaced to the chat UI.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Analysis silently analyzes **BTCUSDT** when you expected another coin | Symbol resolution in `ipc::analyze` is: (1) use `symbol_hint` if non-empty; (2) else ask Haiku (`model_extract`) to read the ticker from the screenshot via `extract_symbol_from_image`; (3) on **any** extraction error it falls back to `"BTCUSDT"` (`.unwrap_or_else(\|_\| "BTCUSDT".into())`). Haiku returns `UNKNOWN` / an empty token → `AppError::Invalid("could not extract symbol")` → swallowed by the fallback. | Pass an explicit `symbol_hint` (the Chat input's symbol field) when the chart is ambiguous or unlabeled. Provide a clearer screenshot showing the ticker. The fallback is intentional so analysis never hard-fails on symbol detection. |
| `invalid input: unknown symbol for CoinGecko: XYZ` shows up in logs / no price recorded for the prediction | `coingecko.rs` has a **hardcoded ~12-coin table** (BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC; USDT/USD/USDC suffixes stripped). Any other symbol → `AppError::Invalid`. Price fetch happens in a fire-and-forget `tokio::spawn`, so the **analysis still completes**; only `record_initial_price` (and later prediction resolution) is skipped. | Use one of the supported tickers if you want price tracking and prediction self-calibration. For other coins the textual analysis still works; only the price/outcome tracking is unavailable. Expanding the symbol table requires a code change in `coingecko::symbol_to_id`. |
| Analysis refuses with `daily cost cap reached` (`CostCapReached`) | `analyze_with_cap` checks the day's accumulated spend in `cost_daily` against `settings.daily_cost_cap_usd` (**default $2**) **before** each main model call. If today's cost ≥ cap, it returns `AppError::CostCapReached` without calling the model. | Raise `daily_cost_cap_usd` in the **Settings** tab, or wait for the next UTC day (the cap is keyed on `%Y-%m-%d`). Switching `model_main` to a cheaper model (e.g. Sonnet vs Opus) lowers per-call spend; rates are matched by substring (`opus`/`haiku`/else→Sonnet) in `cost::rates_for`. |
| The rationale never cites your books; analysis seems to ignore the library | RAG returned **no chunks**. `ipc::analyze` embeds the query `"<SYMBOL> chart pattern analysis"` and runs a brute-force cosine top-k (`retrieval::search`, k=6) over the `doc_chunks` table in `data/vec.db`. If nothing is ingested, or the embedding came back empty (`q.is_empty()` → returns `vec![]`), the retrieved-knowledge block is empty. | Add PDFs/images to `%USERPROFILE%\CogniTrade\library\` (the watcher auto-ingests them) or call `ingest_path` via the Library tab. **PDFs need no API key; images require the Anthropic key** (Haiku describes the image first, then it is chunked/embedded). Confirm `data/vec.db` has rows. Note: the store is plain SQLite with in-Rust cosine — see note below. |
| The chat shows the prose answer but **no structured recommendation** (no action/probability/SL/TP card), and no prediction is recorded | The recommendation is the **trailing fenced ` ```json ` block** parsed by `extract_json_tail` (`llm/parse.rs`) — it is *not* tool-use. It returns `None` if: the model omitted the fence, the JSON is malformed, or it fails `AnalysisJson::validate()` (e.g. `probability_up` outside 0–1). When `analysis` is `None`, `predictions::insert_from_analysis` is skipped, so no prediction row and no price are recorded. | Re-run the analysis. If it recurs, the model isn't emitting a valid JSON tail — the system prompt asks for one, so a transient model formatting miss is the usual cause. The free-text answer is still saved and shown; only the structured card/prediction is missing. |

> **Why "Lance" appears but there is no LanceDB.** `lance.rs`, `LanceStore`, and
> `paths::lancedb()` are **misnomers**. The vector store is plain **SQLite** (`data/vec.db`,
> table `doc_chunks`) storing embeddings as little-endian `f32` BLOBs; `search()` loads
> **every row** and computes cosine similarity in Rust per query. Embedding dimension is
> fixed at **384** (`EMBED_DIM`) and validated on insert. A real ANN index (LanceDB) is
> **Planned — not yet implemented**.

---

## 4. Where to look: logs & tracing

| What | Detail |
| --- | --- |
| Log/data directory | `%USERPROFILE%\CogniTrade\` with subdirs `library\`, `screenshots\`, `data\`, `logs\` (the directories are created on startup by `Paths::ensure_all`). |
| Databases | `data\chat.db` (`messages`, `predictions`, `cost_daily`, `settings`; schema `migrations/001_initial.sql`) and `data\vec.db` (`doc_chunks`). The DB files are created **lazily** by `db::open` / `lance::open`, not by `ensure_all`. Both run in **WAL** mode, so you will also see `-wal`/`-shm` sidecar files. |
| What's emitted | The code logs via the `tracing` facade: e.g. `tracing::info!("ingested {}: {} chunks", ...)` and `tracing::warn!("ingest failed ...")` in the watcher, `tracing::warn!("global shortcut registration ...")` in `lib.rs`, `tracing::warn!("prune failed: ...")` in `prune.rs`. |

> **Important — logging is currently facade-only.** The crate depends on
> `tracing-subscriber` (with the `env-filter` feature) in `Cargo.toml`, but **no subscriber
> is initialized** anywhere in `main.rs` / `lib.rs`. Without an installed subscriber,
> `tracing` events are **dropped** — they are **not** written to a file under `logs\` and
> generally won't appear unless you run a build that wires up a subscriber. Treat the
> `logs\` directory as reserved: **file logging is Planned — not yet implemented.**

To see log output today, install a subscriber at the top of `run()` in `src-tauri/src/lib.rs`,
for example:

```rust
// add at the very start of run(), before tauri::Builder
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,cognitrade_lib=debug".into()),
    )
    .init();
```

Then control verbosity with the `RUST_LOG` env-filter and run from a console build so the
output is visible:

```powershell
# PowerShell — verbose backend logs, quieter dependencies
$env:RUST_LOG = "info,cognitrade_lib=debug,reqwest=warn"
npm run tauri dev
```

```cmd
:: cmd.exe
set RUST_LOG=info,cognitrade_lib=debug
npm run tauri dev
```

The `env-filter` syntax is `target=level` pairs (comma-separated); `cognitrade_lib` is the
library crate name (see `Cargo.toml` `[lib] name`). Note that the release binary sets
`#![windows_subsystem = "windows"]` (no console window), so prefer `npm run tauri dev` or a
debug build when you need to watch logs interactively.

---

## 5. Quick reference

| Error variant (`error.rs`) | Typical trigger | First thing to check |
| --- | --- | --- |
| `NoApiKey` (`api key not set`) | analyze / image ingest with no key in keyring | Set the key in Settings (Credential Manager). |
| `Keyring(..)` | OS credential store inaccessible | Windows Credential Manager permissions. |
| `CostCapReached` | day's spend ≥ `daily_cost_cap_usd` | Raise the cap or wait for next UTC day. |
| `Anthropic(..)` | non-2xx from the Messages API | API key validity, model name, network. |
| `Invalid("could not extract symbol")` | Haiku couldn't read the ticker | Falls back to BTCUSDT; pass a `symbol_hint`. |
| `Invalid("unknown symbol for CoinGecko: …")` | symbol outside the ~12-coin table | Use a supported ticker (price tracking only). |
| `Internal("fastembed …")` | embedding model download/init failed | Network access; retry first ingest. |
| `Http(..)` | transport failure (CoinGecko/Anthropic) | Connectivity, proxy, TLS (rustls). |
