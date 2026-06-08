# Windows Developer Setup

This guide takes you from a **fresh Windows machine** to a running `npm run tauri dev`
for **CogniTrade** — the Tauri 2 desktop app (Rust backend + SvelteKit/TypeScript
frontend) that acts as an **advisory** crypto-chart analyst.

> **What CogniTrade actually is.** You paste/drop a chart screenshot; the backend reads
> it with Claude (vision), retrieves relevant passages from ingested PDFs/images (RAG),
> and streams back a plain-English analysis ending in a structured JSON recommendation.
> The system prompt is explicit: *"You are an advisor. The user places all trades
> themselves."* There is **no exchange integration, no order execution, and no automated
> SL/TP enforcement** in the code. (The original brief `crypto Dhriti.txt` and
> `docs/superpowers/` describe an autonomous trading bot — treat that as aspirational
> background, not a description of the codebase.)

Windows is the **primary platform**: `npm run tauri build` produces a Windows installer,
the API key lives in **Windows Credential Manager**, and user data lives under
`%USERPROFILE%\CogniTrade`.

---

## 1. Prerequisites

| Component | Why it's needed | Notes |
|---|---|---|
| **Rust (MSVC toolchain, via `rustup`)** | Compiles the `src-tauri` backend | Use the **`stable-x86_64-pc-windows-msvc`** toolchain — *not* the GNU one |
| **Visual Studio C++ Build Tools** | Provides `link.exe` + the Windows SDK that the MSVC Rust toolchain links against | Required even if you never open Visual Studio |
| **Node.js LTS** | Runs Vite, SvelteKit, and the `@tauri-apps/cli` | LTS (20.x or 22.x). Bundles `npm` |
| **WebView2 runtime** | Tauri renders the SvelteKit UI inside Microsoft Edge WebView2 | Pre-installed on Windows 11 and current Windows 10; install the Evergreen runtime if missing |
| **Git** | Optional version control (`git init`) | The project is **not** a git repository and cannot be cloned — see [§2](#2-get-the-source-the-repo-is-not-git-initialized) |

There is **no native LanceDB / ONNX / CUDA dependency to install**. Embeddings run on the
CPU via the bundled `fastembed` crate, and SQLite is statically compiled
(`rusqlite` with the `bundled` feature). The hard hardware target is a Ryzen 3400G with
**8 GB RAM** — every model-size and concurrency choice in the code is constrained by that
ceiling.

---

### 1.1 Install Rust (MSVC toolchain)

Download and run `rustup-init.exe` from <https://rustup.rs>, or via PowerShell:

```powershell
Invoke-WebRequest -Uri https://win.rustup.rs/x86_64 -OutFile rustup-init.exe
.\rustup-init.exe
```

When prompted, accept the **default host triple `x86_64-pc-windows-msvc`**. After install,
open a **new** terminal and verify, then make the MSVC toolchain the default:

```powershell
rustup default stable-x86_64-pc-windows-msvc
rustc --version
cargo --version
```

> The MSVC toolchain needs the C++ Build Tools from the next step to link successfully. If
> you skip that step you'll hit a `link.exe not found` / `linker `link.exe` not found`
> error on the first `cargo build` — see [Pitfalls](#7-common-first-run-pitfalls).

### 1.2 Install Visual Studio C++ Build Tools (exact workloads)

Download **Build Tools for Visual Studio 2022** from
<https://visualstudio.microsoft.com/visual-studio-build-tools/> (the standalone "Build
Tools" installer — you do **not** need the full IDE).

In the installer, select the workload:

- **Desktop development with C++**

That workload pulls in the components Rust's MSVC linker needs. Confirm these individual
components are checked (they are included by default with the workload):

- **MSVC v143 — VS 2022 C++ x64/x86 build tools** (the `link.exe` toolset)
- **Windows 11 SDK** (or Windows 10 SDK)

Unattended PowerShell install (adjust the bootstrapper filename to the one you downloaded):

```powershell
.\vs_BuildTools.exe `
  --add Microsoft.VisualStudio.Workload.VCTools `
  --includeRecommended `
  --passive --norestart
```

Reboot if the installer asks you to.

### 1.3 Install Node.js LTS

Download the **LTS** installer (.msi) from <https://nodejs.org>, or via winget:

```powershell
winget install OpenJS.NodeJS.LTS
```

Verify in a new terminal:

```powershell
node --version   # expect v20.x or v22.x
npm --version
```

### 1.4 Install the WebView2 runtime

WebView2 ships with Windows 11 and recent Windows 10. To install/repair the Evergreen
runtime explicitly:

```powershell
winget install Microsoft.EdgeWebView2Runtime
```

Or download the **Evergreen Standalone Installer** from
<https://developer.microsoft.com/microsoft-edge/webview2/>. A missing WebView2 runtime
produces a blank/empty app window at launch — see [Pitfalls](#7-common-first-run-pitfalls).

### 1.5 Install Git

```powershell
winget install Git.Git
```

---

## 2. Get the source (the repo is **not** git-initialized)

This project is **not** a git repository yet — there is no `.git` directory, so you cannot
`git clone` it. Obtain the project folder however it was shared with you (zip, network
share, etc.) and place it somewhere without spaces in the path, e.g. `C:\dev\cognitrade`.

If you want version control, initialize it yourself:

```powershell
cd C:\dev\cognitrade
git init
git add .
git commit -m "Initial import"
```

The repo root contains `package.json` (frontend) and `src-tauri/` (Rust backend +
`Cargo.toml`, `tauri.conf.json`).

---

## 3. Install dependencies and do the first build

### 3.1 JavaScript dependencies

From the **repo root**:

```powershell
npm install
```

This installs the frontend toolchain declared in `package.json`: `@tauri-apps/cli`,
SvelteKit 2, Svelte 5, Vite 6, Tailwind 3, `svelte-check`, and the `@tauri-apps/plugin-*`
JS bindings.

### 3.2 First Rust build (long — downloads + compiles crates)

The first compile is the slow one. It downloads and builds the full backend dependency
tree from `src-tauri/Cargo.toml` — Tauri 2, `tokio` (full), `reqwest` (rustls-tls),
`rusqlite` (bundled SQLite, compiled from C), `fastembed`, `pdf-extract`, `image`,
`keyring`, and more. Expect **several minutes to tens of minutes** on first run depending
on CPU and network.

> **Build the frontend first on a fresh machine.** `lib.rs::run` calls
> `tauri::generate_context!()`, which reads `frontendDist: "../build"` from
> `tauri.conf.json` and **requires that `build/` directory to exist at compile time**. On a
> fresh checkout `..\build` does not exist yet, so a bare `cargo build` fails with a
> missing-`frontendDist` error. Produce it once with `npm run build` (the SvelteKit static
> build) before running `cargo build` standalone:

```powershell
npm run build        # repo root — produces ..\build that generate_context!() needs
cd src-tauri
cargo build
cd ..
```

> You don't strictly need the standalone `cargo build` step at all — `npm run tauri dev`
> (and `npm run tauri build`) run the SvelteKit build for you first via
> `beforeDevCommand` / `beforeBuildCommand` in `tauri.conf.json`, so the `build/` directory
> is always present before the Rust compile starts. Running `cargo build` yourself only
> makes sense to get the long compile out of the way and surface toolchain problems
> (e.g. missing `link.exe`) early — and only after `npm run build` has created `..\build`.

### 3.3 First app run — the BGE-small model downloads on first use

The embedding model is **not** downloaded at build time. `src-tauri/src/ingest/embeddings.rs`
lazily initializes `fastembed`'s **BGE-small-en-v1.5** (`BGESmallENV15`, 384-dim) as a
global singleton (`OnceLock<Mutex<TextEmbedding>>`) the **first time embedding is needed** —
i.e. the first time you ingest a document or run an analysis that triggers RAG retrieval.
That first call downloads the model weights from the network and caches them locally, so it
will pause for a while the first time and be fast afterward.

If model download is blocked (corporate proxy/firewall), embedding-dependent features fail
at runtime, not at build — see [Pitfalls](#7-common-first-run-pitfalls).

---

## 4. Run the dev loop

From the **repo root**:

```powershell
npm run tauri dev
```

What happens (wired in `tauri.conf.json`):

1. Tauri runs `beforeDevCommand: npm run dev`, starting **Vite** on
   `http://localhost:1420` (`devUrl`).
2. Tauri compiles the Rust backend (`src-tauri`) and launches the desktop window pointed
   at the Vite dev server, so frontend edits hot-reload.

On startup `lib.rs::run` (via `setup`):

- creates `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` (`paths::ensure_all`),
- opens the two SQLite DBs (`data\chat.db`, `data\vec.db`),
- spawns the library file-watcher and the background prune/prediction-resolution loops,
- registers a **system tray** icon and a **`Ctrl+Shift+Space`** global hotkey to toggle the
  panel.

> **The main window starts hidden.** `tauri.conf.json` sets the window
> `"visible": false`, `"decorations": false`, `"skipTaskbar": true`. If you don't see a
> window after launch, click the **tray icon** or press **`Ctrl+Shift+Space`** to toggle
> the panel. The window is a fixed 420×800 always-on-top panel.

The UI has three tabs (`src/routes/+page.svelte`): **Chat**, **Library**, **Settings**.

---

## 5. Set the Anthropic API key and create the data directory

### 5.1 The `%USERPROFILE%\CogniTrade` directory

You don't have to create this by hand — the app creates it on first launch via
`paths::ensure_all`. The layout (`paths.rs`, rooted at `dirs::home_dir()\CogniTrade`):

| Path | Contents |
|---|---|
| `%USERPROFILE%\CogniTrade\library\` | Drop PDFs / images here to auto-ingest (RAG corpus) |
| `%USERPROFILE%\CogniTrade\screenshots\` | Chart screenshots saved via `save_screenshot` |
| `%USERPROFILE%\CogniTrade\data\chat.db` | Messages, predictions, daily cost, settings |
| `%USERPROFILE%\CogniTrade\data\vec.db` | `doc_chunks` vector store (SQLite, **not** LanceDB) |
| `%USERPROFILE%\CogniTrade\logs\` | Reserved for logs (see [§6](#6-where-logs-go)) |

To create it ahead of time anyway:

```powershell
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\CogniTrade"
```

> **Never commit this directory.** It holds user documents and chat history.

### 5.2 Set the API key — in-app → Windows Credential Manager

The Anthropic API key is **never stored in the database or in any file**. It is written to
the **OS keyring** via the `keyring` crate, which on Windows is the **Windows Credential
Manager**. From `settings.rs`:

- **Service:** `cognitrade`
- **User/account:** `anthropic_api_key`

**Set it from the running app:** open **Settings** in the app and paste your key. The
frontend calls `set_api_key` (`src/lib/ipc.ts` → `ipc::set_api_key` →
`settings::set_api_key`), which validates the key is non-empty and stores it in Credential
Manager. There is no `.env` file and no environment variable involved.

To confirm/inspect the stored credential in Windows:

```powershell
# GUI: Credential Manager > Windows Credentials > "Generic Credentials"
#      look for an entry whose target contains "cognitrade".
# Or list via cmdkey:
cmdkey /list | Select-String "cognitrade"
```

To clear it, use the **Clear key** action in the app's Settings tab (`clear_api_key` →
`settings::clear_api_key`), or remove the credential in Credential Manager.

**Models & cost guardrail** (defaults in `settings.rs`, stored in the `settings` table —
non-secret):

| Setting | Default | Used for |
|---|---|---|
| `model_main` | `claude-sonnet-4-6` | The main streaming analysis call |
| `model_extract` | `claude-haiku-4-5-20251001` | Vision **symbol extraction** on the analyze path. See note below — auto-ingest image description does **not** read this setting |
| `daily_cost_cap_usd` | `2.0` | `analyze_with_cap` rejects the main call with `CostCapReached` once the day's spend **reaches or exceeds** the cap. The gate `cost::is_over_cap` uses `>=` (`today_cost >= cap`), so hitting the cap exactly blocks the call |

> **API features need a valid key.** Without a key set, `analyze` and image ingestion fail
> (`AppError::NoApiKey`); the library watcher silently **skips images** but still ingests
> **PDFs** (which don't require the API). When changing model IDs, also update
> `cost::rates_for` so cost tracking stays correct.

> **`model_extract` only applies on the analyze path.** It is honored for the vision
> **symbol extraction** done during `ipc::analyze`. The library watcher's **image-ingest**
> path (`ingest/watcher.rs`, line 50) **hardcodes** `claude-haiku-4-5-20251001` for image
> descriptions and does **not** read `settings.model_extract`. Today the two happen to
> match because that is also the default, but changing `model_extract` in Settings will not
> change which model auto-ingest uses to describe dropped images.

---

## 6. Where logs go

The app **creates** `%USERPROFILE%\CogniTrade\logs\`, but the current code does **not**
install a `tracing` subscriber or a file appender. `tracing-subscriber` is a declared
dependency in `Cargo.toml`, but **no `tracing_subscriber::fmt().init()` /
`set_global_default` call exists anywhere in `src-tauri/src`** (verified by grep). The
practical consequence matters:

- **Backend `tracing` events are currently dropped.** Because no subscriber is registered,
  every `tracing::info!` / `tracing::warn!` / `tracing::error!` event — including the
  hotkey-registration `tracing::warn!` in `lib.rs` (lines 60/63) and the ingest warnings in
  `ingest/watcher.rs` — is silently discarded and produces **no terminal output**.
- **What you _do_ see in `npm run tauri dev`:** only framework logs from Tauri / Cargo /
  Vite, plus anything written directly to `stderr` (notably **panics**). `tracing`
  diagnostics from the app's own code are not among them.
- **To actually see backend diagnostics today**, add a subscriber — e.g. call
  `tracing_subscriber::fmt().init();` near the top of `lib.rs::run` — then rerun. Until that
  is done, treat backend `tracing` macros as no-ops for observability.
- **Frontend / WebView console:** right-click the app window → **Inspect** (or the WebView2
  devtools) for the SvelteKit side. This is independent of the Rust `tracing` situation.
- **Release builds** set `windows_subsystem = "windows"` (`src-tauri/src/main.rs`), so there
  is **no console window** and stderr is not shown — wiring file logging into
  `paths.logs()` is the natural next step if you need persistent release logs.

> **Planned — not yet implemented:** (1) registering a `tracing` subscriber so backend
> diagnostics are emitted at all, and (2) persistent on-disk logging under
> `%USERPROFILE%\CogniTrade\logs\`. The directory exists but nothing writes to it today.

---

## 7. Common first-run pitfalls

| Symptom | Cause | Fix |
|---|---|---|
| `error: linker `link.exe` not found` during `cargo build` | MSVC C++ Build Tools / Windows SDK not installed, or you're on the GNU Rust toolchain | Install **Desktop development with C++** ([§1.2](#12-install-visual-studio-c-build-tools-exact-workloads)); run `rustup default stable-x86_64-pc-windows-msvc`; open a **new** terminal so `PATH` picks up the toolset |
| App window is **blank / empty** or fails to create a webview | **WebView2 runtime missing** | Install the Evergreen WebView2 runtime ([§1.4](#14-install-the-webview2-runtime)) |
| App launches but **no window appears** | Window starts hidden (`"visible": false` in `tauri.conf.json`) | Click the **tray icon** or press **`Ctrl+Shift+Space`** to toggle the panel — this is expected behavior, not a bug |
| First analysis / ingest hangs, then errors with a `fastembed` / download message | **BGE-small model download blocked by proxy/firewall** | Allow outbound HTTPS to the model host, or configure the proxy via `HTTPS_PROXY`/`HTTP_PROXY` env vars before launching, then retry — the download is cached after the first success ([§3.3](#33-first-app-run--the-bge-small-model-downloads-on-first-use)) |
| Analysis fails immediately with `NoApiKey` | No Anthropic key in Credential Manager | Set the key in the app's **Settings** tab ([§5.2](#52-set-the-api-key--in-app--windows-credential-manager)) |
| Analysis blocked with `CostCapReached` | Daily spend exceeded `daily_cost_cap_usd` ($2 default) | Raise the cap in **Settings** (resets per UTC day in `cost_daily`) |
| `port 1420 already in use` on `npm run tauri dev` | A previous Vite dev server is still running | Kill the stray Node process; the dev URL is hardcoded to `http://localhost:1420` in `tauri.conf.json` |
| `Ctrl+Shift+Space` does nothing after a previously crashed run | The global hotkey may already be registered by a stale process, so re-registration fails | Re-registration failure is tolerated in `lib.rs` (wrapped in `tracing::warn!`), but with **no `tracing` subscriber installed the warning is never printed** — so a failed registration is currently **silent** ([§6](#6-where-logs-go)). If the hotkey is unresponsive, use the **tray icon** to toggle the panel, or kill the stale process and relaunch |
| `git clone` fails | The project is **not a git repo** | Obtain the folder directly and optionally `git init` ([§2](#2-get-the-source-the-repo-is-not-git-initialized)) |

---

## 8. Quick verification checklist

```powershell
rustc --version            # ...-msvc
node --version             # LTS
npm install                # repo root, succeeds
npm run build              # produces ..\build that generate_context!() needs
cd src-tauri; cargo build  # compiles with MSVC linker (needs ..\build to exist)
cd ..
npm run tauri dev          # runs npm run build itself, then Vite on :1420, tray icon appears
```

> The standalone `cargo build` line is optional; it is shown here only to validate the MSVC
> linker early. `npm run tauri dev` runs `npm run build` for you via `beforeDevCommand`. If
> you skip `npm run build` and run a bare `cargo build` on a fresh checkout, it fails
> because `..\build` (the `frontendDist`) does not yet exist ([§3.2](#32-first-rust-build-long--downloads--compiles-crates)).

Then in the app:

1. Open via the tray icon / `Ctrl+Shift+Space`.
2. **Settings** → paste your Anthropic API key (lands in Windows Credential Manager).
3. **Library** → drop a PDF into `%USERPROFILE%\CogniTrade\library\` (triggers the
   BGE-small download on first ingest).
4. **Chat** → paste a chart screenshot and run an analysis; watch text stream in via the
   `analysis_chunk` event and finish with `analysis_done`.

Backend tests:

```powershell
cd src-tauri
cargo test                                                   # unit tests
cargo test --test ingest_integration -- --ignored --nocapture # downloads BGE-small
cargo test --test llm_integration -- --ignored                # needs a live API key
```

Type-check the frontend:

```powershell
npm run check
```

---

## Related docs

- Project overview & architecture: [`../../CLAUDE.md`](../../CLAUDE.md)
- Original product brief (aspirational): [`../../crypto Dhriti.txt`](../../crypto%20Dhriti.txt)
- Design specs & build plan (Phase 0–5): [`../superpowers/`](../superpowers/)
