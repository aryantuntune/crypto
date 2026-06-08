# Security, Secrets & Privacy

This document describes CogniTrade's security posture, secret handling, and data-handling behavior **as the code actually implements it today**. Where a control is described in the spec or design docs but is not yet in the code, it is labeled **Planned — not yet implemented**.

CogniTrade is a single-user **advisory** desktop app (Tauri 2: Rust backend + SvelteKit/TypeScript frontend). It reads a crypto chart screenshot, retrieves passages from locally-ingested documents (RAG), and streams back an analysis. It does **not** connect to any exchange, hold custody of funds, place orders, or move money. The system prompt is explicit: *"You are an advisor. The user places all trades themselves."* This single fact eliminates the largest class of risk for a "trading bot" — there are no exchange API keys, no withdrawal permissions, and no order-execution code path to compromise.

Related reading:

- [IPC API reference](../architecture/ipc-api-reference.md) — the full surface of commands the frontend can invoke.
- [Data model](../architecture/data-model.md) — the SQLite schemas that store local data.
- [Tech stack](../architecture/tech-stack.md) — dependencies and versions.
- [Windows setup](../development/windows-setup.md) — toolchain and runtime prerequisites.
- [Build & release](../development/build-and-release.md) — installer/bundle targets.

---

## 1. Secret handling

### 1.1 The only secret: the Anthropic API key

CogniTrade has exactly one credential: the **Anthropic API key** used to call `https://api.anthropic.com/v1/messages`. There are no exchange keys, no OAuth tokens, no user passwords.

The key is stored in the **OS keyring** via the `keyring` crate (`keyring = "2"`), never in the SQLite database, never in a config file. On Windows, the `keyring` crate is backed by the **Windows Credential Manager** (Generic Credentials). All access goes through `src-tauri/src/settings.rs`:

| Function | Behavior |
| --- | --- |
| `set_api_key(value)` | `Entry::new(KEYRING_SERVICE, KEYRING_USER)?.set_password(value)` |
| `get_api_key()` | Returns `Ok(Some(_))`, or `Ok(None)` if the keyring has no entry (`keyring::Error::NoEntry`) |
| `clear_api_key()` | Deletes the entry; treats "no entry" as success |
| `require_api_key()` | `get_api_key()` or fails with `AppError::NoApiKey` |

The keyring coordinates are constants in `settings.rs`:

```rust
const KEYRING_SERVICE: &str = "cognitrade";
const KEYRING_USER: &str = "anthropic_api_key";
```

On Windows this surfaces as a Credential Manager entry under the service name `cognitrade`. To inspect or remove it manually:

```powershell
# List the credential (Windows)
cmdkey /list | findstr /i cognitrade

# Remove it entirely (forces re-entry on next analysis)
cmdkey /delete:cognitrade
```

### 1.2 The key never lands in the database, logs, or screenshots

- **Database**: The `chat.db` `settings` table (see `migrations/001_initial.sql`) holds only **non-secret** values — `daily_cost_cap_usd`, `model_main`, `model_extract`, `library_path`, `hotkey` (see `Settings` in `settings.rs`). The API key is intentionally **not** one of these keys; `settings::save` writes a fixed 5-element list that excludes it.
- **Logs**: The key is read into a local `String` only at call time (`require_api_key()` inside `analyze_with_cap` and `extract_symbol_from_image`) and passed straight into the HTTP `x-api-key` header. It is never `tracing::info!`/`warn!`-ed. The logging that exists (`ingest::watcher`, `prune`) logs file paths, chunk counts, and error messages — not credentials. **Caveat:** the watcher *does* `info!`-log the **path of each ingested document** (`ingest::watcher` lines 42 and 53: `"ingested {doc_path}: {n} chunks"`). Those paths can reveal the **filenames/titles of a user's private trading research** in `logs\`. Credentials are never logged, but `logs\` should be treated as sensitive — see §6.1.
- **Screenshots**: `ipc::save_screenshot` writes only the decoded PNG bytes the frontend sent; it never touches the keyring.

### 1.3 IPC surface for the key

The frontend can never read the key back. The only key-related commands registered in `lib.rs` are:

| Command | Returns | Notes |
| --- | --- | --- |
| `get_api_key_set` | `bool` | Existence check only — **never returns the key value** |
| `set_api_key(value)` | `()` | Rejects empty/whitespace input (`AppError::Invalid`) |
| `clear_api_key` | `()` | Removes the credential |

This is a deliberate write-only/existence-only design: the UI can tell the user "a key is set" and let them replace or clear it, but there is no IPC path that exfiltrates the secret to the renderer.

---

## 2. Data locality — what stays local vs. what leaves the machine

### 2.1 What is stored locally (and only locally)

All user data lives under `%USERPROFILE%\CogniTrade\` (`~/CogniTrade/` on other platforms), created at startup by `paths::Paths::ensure_all()`:

```
%USERPROFILE%\CogniTrade\
├── library\        # user-dropped PDFs/images to ingest (watched)
├── screenshots\    # chart PNGs saved by save_screenshot
├── data\
│   ├── chat.db     # messages, predictions, cost_daily, settings
│   └── vec.db      # doc_chunks: embeddings + chunk text
└── logs\
```

Two **SQLite** databases, both opened in WAL mode, hold structured data:

| Store | File | Tables | Contents |
| --- | --- | --- | --- |
| Chat DB (`db::open`) | `data\chat.db` | `messages`, `predictions`, `cost_daily`, `settings` | Full chat transcript, prediction/calibration rows, daily token spend, non-secret settings |
| Vector DB (`lance::open`) | `data\vec.db` | `doc_chunks` | Embeddings (384-dim little-endian `f32` BLOBs) + the source chunk text |

> **Naming caveat:** `lance.rs`, `LanceStore`, and `paths::lancedb()` are **misnomers**. There is no LanceDB dependency. `vec.db` is plain SQLite; `search()` loads every row and computes cosine similarity in Rust. This matters for security only in that there is no external/embedded vector service and no extra network listener.

**Embeddings are computed locally.** `ingest::embeddings` runs `fastembed` BGE-small (`BGESmallENV15`) on the CPU, behind a global `OnceLock<Mutex<TextEmbedding>>`. On first use it **downloads the model weights** from the fastembed/Hugging Face source — that is a one-time model fetch, not user data leaving the machine. After that, all embedding is offline.

### 2.2 What leaves the machine

CogniTrade makes outbound calls to exactly **two** third-party hosts. There are no analytics, no telemetry, no crash reporting, no auto-update beacons in the code.

| Destination | Triggered by | What is sent | What is *not* sent |
| --- | --- | --- | --- |
| **Anthropic** `api.anthropic.com/v1/messages` | `ipc::analyze` (main analysis), `extract_symbol_from_image` (symbol from chart), `ingest_image` / `ingest::image::describe_image` (image description) | See the three distinct payloads below. The `x-api-key` header carries the key. | The API key is never stored remotely beyond Anthropic's own auth |
| **CoinGecko** `api.coingecko.com/api/v3/simple/price` | `coingecko::get_usd_price` (price-at-prediction, and prediction resolution in `prune`) | **Only a coin id** derived from the resolved symbol (e.g. `bitcoin`) via the hardcoded `symbol_to_id` table. The endpoint is the free `simple/price` GET. | No chat content, no images, no documents, no key (the free endpoint is unauthenticated) |

The Anthropic destination actually covers **three different payloads**, sent to three models/paths:

| Path | Function | Model | Payload sent |
| --- | --- | --- | --- |
| Main analysis | `ipc::analyze` → `analyze_with_cap` | `model_main` (default `claude-sonnet-4-6`) | A **base64 PNG chart screenshot** (the analyze path hardcodes `image/png`), the **user's text**, recent **chat history** (last 10 messages), and the **top-k retrieved document chunks** (`retrieval::search`, k=6) in the prompt. |
| Symbol extraction | `extract_symbol_from_image` (`ipc.rs`) | `model_extract` (default `claude-haiku-4-5-20251001`) | The **chart screenshot** as base64 (`image/png`), to extract the trading symbol when the user gives no hint. |
| Library image indexing | `ingest_image` / `ingest::image::describe_image` | `model_extract` (Haiku) | The **full user library image** dropped into `library\` — `png`/`jpg`/`jpeg`/`webp` (rejected if over **5 MB**) — sent with `DESCRIBE_PROMPT` so Haiku writes an indexing description. This is **any image the user drops in**, not necessarily a chart screenshot, and it uses its own media-type and size guard distinct from the analyze path. |

So the `library\` directory is a transmission surface: every image dropped there is auto-described by Haiku (see §2.2 watcher note and §4.4).

Two further notes on what leaves:

- **Outbound TLS** uses `reqwest` with `rustls-tls` (see `Cargo.toml`: `default-features = false, features = ["json", "rustls-tls", "stream"]`) — no system OpenSSL dependency.
- **Prompt caching:** the system prompt and the retrieved-knowledge block are marked `cache_control: ephemeral` (`llm/prompt.rs`, `llm/client.rs`). This is an Anthropic-side ephemeral cache for cost/latency; it does not change *who* receives the data.

**Privacy implication to communicate to users:** whatever they put in the chat, the screenshots they analyze, and the contents of any document chunks that get retrieved are transmitted to Anthropic as part of normal operation. Users should treat the library directory and chat history as "will be sent to a third-party LLM API."

---

## 3. Tauri capability / allowlist model (least privilege)

Tauri 2 gates what the frontend (WebView) is allowed to ask the backend to do through a **capabilities** file. CogniTrade's is `src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default"
  ]
}
```

### 3.1 Why this is least-privilege

- **Scoped to one window.** The capability applies only to the `"main"` window (the single window declared in `tauri.conf.json`).
- **Minimal permission sets.** Only `core:default` (the baseline Tauri core commands) and `opener:default` (the `tauri-plugin-opener` defaults, for opening external links) are granted. Notably, **no broad filesystem or shell capability is exposed to the WebView** here, even though `tauri-plugin-fs`, `tauri-plugin-dialog`, and `tauri-plugin-global-shortcut` are initialized as plugins in `lib.rs`. Because **none of those plugins' permission sets are listed in `capabilities/default.json`**, their JS bindings are **unreachable from the WebView** — the renderer cannot invoke a dialog, an `fs` read/write, or a global-shortcut command at all. File selection for ingest therefore flows through the Rust commands (e.g. `ingest_path` receives a path **string**), not a renderer-side dialog/fs call.
- **All real work goes through typed `#[tauri::command]`s.** File reads/writes the app needs (reading a screenshot, writing to the library, opening SQLite) happen **inside Rust**, invoked through the explicit command allowlist registered in `lib.rs`:
  `list_recent_messages`, `get_settings`, `save_settings`, `get_api_key_set`, `set_api_key`, `clear_api_key`, `cost_today`, `save_screenshot`, `analyze`, `ingest_path`.
  The renderer cannot read an arbitrary file or run a shell command directly; it can only call these ten commands, each of which validates and constrains what it does. See the [IPC API reference](../architecture/ipc-api-reference.md).

### 3.2 The CSP gap (call it out)

`tauri.conf.json` sets `app.security.csp` to `null`:

```json
"security": { "csp": null }
```

A `null` CSP means **no Content-Security-Policy is injected** into the WebView. For a local SPA bundled with `adapter-static` and no remote content, the practical exposure is limited, but a defined CSP is defense-in-depth against accidentally loading remote scripts or against an XSS foothold in rendered LLM output. **Recommendation:** set a restrictive CSP (e.g. `default-src 'self'`, with the explicit `connect-src` entries the app needs) before release. Until then, treat any rendering of model output in the WebView as untrusted HTML and ensure it is escaped, not injected as raw markup.

---

## 4. Threat model

CogniTrade is a **local, single-user desktop application**. There is no server, no multi-tenancy, no authentication of remote users, and — critically — **no fund custody and no trade execution**. That removes the highest-severity outcomes (stolen exchange keys, unauthorized withdrawals, rogue orders). What remains:

### 4.1 Assets

1. The **Anthropic API key** (financial value: API spend). The in-app daily cost cap (§4.4) bounds spend through CogniTrade's *own* analyze path, but it does **not** protect against a process that has already extracted the key and calls Anthropic directly — for that, rely on OS account isolation around Credential Manager and on setting a low spend limit on the key itself.
2. **Local user data**: chat transcript, ingested documents, screenshots, prediction history (`chat.db`, `vec.db`, the `library`/`screenshots` directories).

### 4.2 Trust boundaries & attack surface

| Surface | Description | Exposure |
| --- | --- | --- |
| **IPC boundary** (WebView → Rust) | The 10 registered commands. The WebView is the SPA we ship; in a Tauri app the renderer is the lower-trust side. | Inputs are validated per-command (`set_api_key` rejects empty; `ingest_path` rejects unsupported extensions; `save_screenshot` validates base64). |
| **Library file watcher** | `ingest::watcher` runs `notify` recursively on the library dir and **auto-ingests** any PDF (no key needed) or image (needs key) dropped in, with an 800ms debounce. | Anything that can write to `%USERPROFILE%\CogniTrade\library\` gets its content embedded into `vec.db` and, for images, **sent to Anthropic (Haiku) for description** with **no per-call cost gating and no usage accounting** (see §4.4). A malicious or accidental file dropped there is processed automatically. PDF parsing (`pdf-extract`) and image decoding (`image`) run on untrusted file bytes. |
| **Network responses** | Streaming SSE from Anthropic; JSON from CoinGecko. | The SSE parser (`llm/streaming`) and JSON-tail parser (`llm/parse::extract_json_tail`) consume model output. When the trailing fenced `json` block parses into a valid `AnalysisJson` (`out.analysis.is_some()`, `ipc.rs:195`), `predictions::insert_from_analysis` inserts a `prediction` row; **malformed or absent JSON is tolerated** — no prediction row is stored, but the assistant message is still saved. Model output is also rendered in the WebView (see CSP note, §3.2). |
| **Global hotkey** | `Ctrl+Shift+Space` toggles the panel (`lib.rs`, `tray::toggle_panel`). | Local-only; toggles visibility, not a data path. |
| **On-disk data at rest** | `chat.db`, `vec.db`, screenshots, library files. | Plain files under the user profile. **No application-level encryption** — they inherit only the OS account's file permissions. Anyone with access to the Windows user account (or a backup of it) can read the chat history and ingested documents. The API key is the exception: it lives in Credential Manager, not in these files. |

### 4.3 Out of scope (by design, because the code does not do it)

- **Exchange/withdrawal compromise** — no exchange integration exists.
- **Order execution / SL-TP enforcement** — *Planned in the spec, not implemented.* The app stores `stop_loss_pct`/`take_profit_pct` on `prediction` rows for **self-calibration only**; `predictions::resolve_one` compares price-at-horizon to price-at-prediction and does **not** simulate intra-horizon SL/TP hits and does not act on them.
- **Minimax decision engine** — *Planned in the spec, not implemented.*

### 4.4 Built-in abuse limiter: the daily cost cap

`analyze_with_cap` (`llm/client.rs`) checks `settings.daily_cost_cap_usd` (default **$2.00**) **before** each main analysis call and returns `AppError::CostCapReached` if the day's recorded spend (`cost_daily`) is over the cap. The cap bounds spend through **CogniTrade's own analyze path only**.

Two important limitations to be precise about:

- **It is not a key-leak mitigation.** The cap is enforced inside `analyze_with_cap` — the app's own code path. A process that has already stolen the key from Credential Manager calls `api.anthropic.com` directly and is **wholly unaffected** by the in-app cap. For that risk, rely on the OS account isolation around Credential Manager and on setting a low spend limit on the Anthropic key itself.
- **The auxiliary Haiku calls are not just ungated — they are unmetered.** `extract_symbol_from_image` (`ipc.rs`) and `ingest::image::describe_image` both issue raw `reqwest::Client::new()` requests and **never call `cost::add_usage`**. Their token spend is therefore **never recorded in `cost_daily` at all**, so it is invisible to both the cap check *and* the `cost_today` display. Because `ingest::watcher` **auto-describes every image dropped into `library\`**, this is an **unbounded, unmetered spend path** the cap cannot see. **Recommendation:** route all Anthropic calls through usage accounting (`cost::add_usage`) and/or cap watcher image throughput.

---

## 5. Recommendations

These are hardening steps for moving toward release. Items marked *Planned* are not in the codebase today.

### 5.1 Code signing (release builds)

`npm run tauri build` produces an installer per `tauri.conf.json` (`bundle.targets: "all"` → on Windows, MSI/WiX and NSIS). **Sign the installer and the executable** with an Authenticode certificate so Windows SmartScreen and users can verify provenance. Tauri supports configuring Windows signing in the bundle config (certificate thumbprint + `signtool`). Unsigned builds will trigger SmartScreen warnings and offer no tamper-evidence. See [Build & release](../development/build-and-release.md).

### 5.2 Dependency auditing (`cargo audit`)

The backend pulls in network- and parser-facing crates (`reqwest`, `pdf-extract`, `image`, `fastembed`, `rusqlite` bundled). Run vulnerability auditing in CI and locally:

```powershell
cargo install cargo-audit
cd src-tauri
cargo audit
```

Also consider `cargo deny` for license + advisory + duplicate-version policy, and `npm audit` for the frontend dependency tree. *None of these are wired into the repo yet (the repo is not even a git repository) — adding them is recommended.*

### 5.3 Log redaction & log hygiene

Logging is via `tracing`/`tracing-subscriber`. Today no secret is logged, but to keep it that way as the code grows:

- **Never log the value returned by `require_api_key()`/`get_api_key()`** — pass it directly into the request and drop it.
- **Be careful logging error bodies.** `AppError::Anthropic` includes the response body (`HTTP {status}: {body}`) from `llm/client.rs`. Anthropic error bodies do not echo the request key, but audit any future "log the full request" debugging to ensure headers (which carry `x-api-key`) are never serialized.
- **Treat the user's chat text and document chunks as sensitive** in any future verbose/debug logging — they are personal/financial in nature.
- **Redact or path-strip ingested file names.** The watcher already `info!`-logs the **full path** of each ingested document (`ingest::watcher`), which can reveal the titles of private research. Consider logging only a basename hash or chunk count, and treat `logs\` as sensitive material protected by the OS account.

### 5.4 Do not commit `%USERPROFILE%\CogniTrade\`

The user-data directory contains chat history, ingested documents, screenshots, and prediction history. It must **never** be committed. Per `CLAUDE.md`, `~/CogniTrade/{library,screenshots,data,logs}` is explicitly "NEVER commit."

When this project is put under version control (`git init`), add an ignore rule. Note the data dir lives under the user profile, not the repo, so the main risk is a developer copying it in for debugging:

```gitignore
# Never commit user data
CogniTrade/
**/CogniTrade/library/
**/CogniTrade/screenshots/
**/CogniTrade/data/
**/CogniTrade/logs/
*.db
*.db-wal
*.db-shm
```

(SQLite WAL/SHM sidecar files should be ignored too, since the DBs run in WAL mode.)

### 5.5 Additional hardening (Planned / recommended)

- **Define a CSP** (§3.2) instead of `null`.
- **Encryption at rest** for `chat.db`/`vec.db` (e.g. SQLCipher) is *not implemented*; consider it if the threat model expands to shared/backed-up machines.
- **Validate/limit watcher inputs** — cap file size and ignore unexpected types beyond the current PDF/image extension check, since the watcher auto-processes anything dropped in the library dir.

---

## 6. Financial-data sensitivity & the not-financial-advice stance

### 6.1 Sensitivity

CogniTrade handles inherently sensitive material: the user's **trading ideas, chart screenshots, private research documents, and a history of model predictions**. Even though no money moves through the app, this data reveals a user's positions, strategy, and risk appetite. Two concrete consequences:

1. Anything analyzed or ingested is **sent to Anthropic** (§2.2). Users must be comfortable with that third-party processing.
2. The local DBs and library are **unencrypted at rest** (§4.2) — protected only by the OS user account.
3. The watcher **`info!`-logs the file paths of ingested documents** (`ingest::watcher` lines 42/53). Those paths can leak the **filenames/titles of private research material** into `logs\`. Treat `logs\` as sensitive (it sits under the same OS-account protection as the DBs), and consider redacting file names in verbose logging — see §5.3.

### 6.2 Not financial advice

CogniTrade is **advisory and informational only**. By design it:

- **Never executes trades.** The system prompt (`llm/prompt.rs`) states: *"You are an advisor. The user places all trades themselves."* There is no exchange/order code.
- **Produces probabilities, not guarantees.** The prompt instructs the model to be *calibrated* (`probability_up` in `[0, 1]`, "reflect realistic uncertainty, not advocacy") and to `hold` when the chart is ambiguous. The `predictions` self-calibration loop exists precisely to measure how well-calibrated those probabilities are over time — it is a quality signal, not a promise.
- **Stores SL/TP as metadata, not enforcement.** `stop_loss_pct`/`take_profit_pct` on a `prediction` are recorded values, never executed or guaranteed (§4.3).

**Recommended user-facing stance:** present a clear disclaimer in the UI and docs — *CogniTrade is an experimental analysis assistant, not a financial advisor; its outputs are AI-generated, may be wrong, and must not be the sole basis for any trade. The user is solely responsible for all trading decisions.* This matches what the code actually does and avoids implying capabilities (autonomous trading, enforced risk controls) that the spec describes but the implementation does not provide.
