# Contributing Guide

Welcome. This guide onboards a new contributor to **CogniTrade** end-to-end: how the repo is laid out, how to pick up and ship work, the local dev loop, the definition of done, the pull-request checklist, and the safety rules you must never break.

Read this once, top to bottom, before your first change.

---

## 1. What CogniTrade actually is (read this first)

CogniTrade is a **Tauri 2 desktop app**: a Rust backend (`src-tauri/`) and a SvelteKit/TypeScript frontend (`src/`, Svelte 5 + Tailwind 3 + Vite 6, static SPA via `adapter-static`).

It is an **advisory crypto-chart analyst, not an autonomous trader.** The user pastes or drops a chart screenshot; the backend reads it with Claude (vision), retrieves relevant passages from ingested PDFs/images (RAG), and streams back a plain-English analysis ending in a structured JSON recommendation. The system prompt (`src-tauri/src/llm/prompt.rs`, `SYSTEM_PROMPT`) opens with:

> "You are CogniTrade, a personal crypto trading analyst. The user will share a chart screenshot."

and ends, as its final rule, by making the advisory boundary explicit:

> "You are an advisor. The user places all trades themselves."

There is **no** exchange integration (Binance/Kraken), **no** order execution, **no** Minimax decision engine, and **no** automated stop-loss/take-profit enforcement in the implemented code. Those appear in the original brief (`crypto Dhriti.txt`) and the design docs under `docs/superpowers/`, but they are **aspirational / not implemented**. Do not present them as existing features, and do not "wire them up" without explicit sign-off (see [§9, Safety rules](#9-safety-rules-non-negotiable)).

### Orientation links

| Resource | Path | What it is |
| --- | --- | --- |
| Authoritative codebase guide | [`../../CLAUDE.md`](../../CLAUDE.md) | The single source of truth for how the code is structured. Start here. |
| Tech stack | [`../architecture/tech-stack.md`](../architecture/tech-stack.md) | Frameworks, crates, and frontend deps. |
| Build & release | [`build-and-release.md`](./build-and-release.md) | How to produce installers. |
| Windows setup | [`windows-setup.md`](./windows-setup.md) | Toolchain install (Windows is the primary platform). |
| Vision & scope | [`../project/vision-and-scope.md`](../project/vision-and-scope.md) | Product intent and what is in/out of scope. |
| Roadmap / build plan | [`../superpowers/plans/2026-05-05-cognitrade-v1.md`](../superpowers/plans/2026-05-05-cognitrade-v1.md) | Phase 0–8 milestones. |
| Design intent (aspirational) | [`../superpowers/specs/2026-05-05-cognitrade-design.md`](../superpowers/specs/2026-05-05-cognitrade-design.md) | Diverges from code; background only. |

> There is no `docs/README.md` index yet. If you add one, link it here.

---

## 2. Repository layout

```
crypto/
├─ CLAUDE.md                  # authoritative codebase guide — read first
├─ crypto Dhriti.txt          # original (aspirational) product brief
├─ package.json               # frontend + Tauri CLI scripts
├─ svelte.config.js           # adapter-static SPA config
├─ vite.config.js
├─ tsconfig.json
├─ src/                       # SvelteKit frontend
│  ├─ routes/+page.svelte     # 3-tab shell: Chat / Library / Settings
│  └─ lib/
│     ├─ ipc.ts               # typed wrappers over every Tauri `invoke`
│     ├─ types.ts             # shared TS types (mirror Rust serde shapes)
│     ├─ stores/chat.ts       # listens for analysis_chunk / analysis_done
│     └─ components/          # ChatPanel, LibraryView, MessageBubble,
│                             #   PredictionCard, ScreenshotPaste, SettingsView
├─ src-tauri/                 # Rust backend
│  ├─ src/
│  │  ├─ lib.rs               # app bootstrap, tray, hotkey, bg tasks, IPC registration
│  │  ├─ ipc.rs              # all #[tauri::command]s
│  │  ├─ llm/{mod,client,prompt,streaming,parse,types}.rs
│  │  ├─ ingest/{mod,chunker,pdf,image,embeddings,watcher}.rs
│  │  ├─ lance.rs            # vector store (SQLite + brute-force cosine — NOT LanceDB)
│  │  ├─ retrieval.rs predictions.rs coingecko.rs cost.rs settings.rs
│  │  ├─ paths.rs db.rs chat_store.rs prune.rs tray.rs image_util.rs error.rs
│  ├─ migrations/001_initial.sql   # chat.db schema
│  ├─ tests/                  # ingest_integration.rs (#[ignore]d), llm_integration.rs (mocked, runs by default)
│  ├─ capabilities/default.json
│  ├─ Cargo.toml
│  └─ tauri.conf.json
└─ docs/
   ├─ architecture/  development/  project/  superpowers/
```

### Things to internalize early

- **The whole app is essentially one IPC command**, `ipc::analyze`. Understand that flow (next section) before touching anything.
- **`lance.rs` / `LanceStore` / `paths::lancedb()` are misnomers.** There is no LanceDB. It is plain SQLite storing embeddings as little-endian `f32` BLOBs; `search()` loads *every* row and computes cosine similarity in Rust per query. Embedding dim is fixed at `384` (`EMBED_DIM`), validated on insert. Note `paths::lancedb()` resolves to `data/vec.db` despite its name (`paths.rs`).
- **Two separate SQLite DBs:** `data/chat.db` (`db::open`; tables `messages`, `predictions`, `cost_daily`, `settings`; schema in `migrations/001_initial.sql`) and `data/vec.db` (`lance::open`; table `doc_chunks`). Both run in WAL mode.
- **The Anthropic API key lives in the OS keyring**, never in any DB or file. Service `cognitrade`, user `anthropic_api_key` (`settings.rs`).

### The `analyze` request flow (canonical mental model)

1. Persist the user message (`chat_store::insert`).
2. Resolve the symbol: user hint → else Haiku vision extraction from the screenshot (`extract_symbol_from_image`) → else fallback `BTCUSDT`.
3. RAG: embed `"<SYMBOL> chart pattern analysis"`, brute-force cosine top-k over `doc_chunks` (`retrieval::search` → `lance::LanceStore::search`).
4. Build the Anthropic request (`llm::prompt::build_messages`): chat history, then a user turn = cached retrieved-knowledge block + image + user text. The system prompt is also `cache_control: ephemeral`.
5. Stream SSE; forward text deltas to the frontend over the Tauri event `analysis_chunk`.
6. Parse the trailing fenced ` ```json ` block (`llm::parse::extract_json_tail`) into `AnalysisJson`. **The recommendation is a JSON tail of free text, not tool-use.**
7. **If the JSON tail parsed** (`out.analysis.is_some()`), store a `prediction` row and record the current CoinGecko price (fire-and-forget); otherwise skip — the assistant message gets `prediction_id = None`. Either way, always store the assistant message (`chat_store::insert`) and emit `analysis_done`.

`predictions` are a **self-calibration loop**, not trades. `prune::spawn_background` runs two loops: chat prune (rows older than 7d, every 6h) and prediction resolution (every 60s) — `predictions::resolve_one` resolves predictions whose `ts + horizon_seconds` has passed by polling CoinGecko. It records price-at-horizon vs price-at-prediction; **it does not simulate intra-horizon SL/TP hits.** Horizons: `1h` / `4h` / `1d` / `3d` / `1w`.

### RAG ingestion at a glance

`ingest_pdf` extracts text (`pdf-extract`), then `chunker::chunk_text(text, target_tokens, overlap_tokens)` splits it. `ingest/mod.rs` calls `chunk_text(&full_text, 500, 50)` — i.e. **≈500-token chunks with 50-token overlap**, using a `~4 chars/token` heuristic (`target_chars = target_tokens * 4`, so ~2000 chars per chunk), split on sentence boundaries. These are **token** budgets, not word counts. Each chunk is embedded (fastembed BGE-small, CPU-only, behind a global mutex), then `delete_by_doc_path` + insert into `doc_chunks`. `ingest_image` first has Haiku describe the image, then chunks/embeds that description the same way.

---

## 3. Prerequisites & environment

Windows is the **primary** platform. You need:

| Tool | Notes |
| --- | --- |
| **Rust (MSVC toolchain)** | `rustup default stable-x86_64-pc-windows-msvc`. Tauri on Windows uses MSVC, not GNU. |
| **Visual Studio C++ Build Tools** | "Desktop development with C++" workload — required to link the Rust backend. |
| **WebView2 Runtime** | Required at runtime; preinstalled on current Windows 10/11. |
| **Node.js + npm** | For the frontend and the Tauri CLI. |

Full step-by-step install lives in [`windows-setup.md`](./windows-setup.md). Read it before your first `npm run tauri dev`.

After cloning:

```powershell
npm install
```

This installs JS deps and the Tauri CLI. Rust deps are fetched by Cargo on the first build.

### API key (for anything that calls Claude)

The app reads/writes the Anthropic key via the OS keyring (Windows Credential Manager). Set it from the **Settings** tab in the running app — never hardcode it, never commit it, never put it in the DB. PDF ingestion works **without** a key; image ingestion and chart analysis require one.

### User data directories (never commit)

On first run `lib.rs` creates `%USERPROFILE%\CogniTrade\` with subfolders:

```
%USERPROFILE%\CogniTrade\
├─ library\       # drop PDFs/images here → auto-ingested by the watcher
├─ screenshots\   # saved chart screenshots
├─ data\          # chat.db + vec.db (SQLite, WAL)
└─ logs\
```

None of these belong in the repo.

---

## 4. Picking up work

Work is organized by the **Phase 0–8 milestones** in the build plan: [`../superpowers/plans/2026-05-05-cognitrade-v1.md`](../superpowers/plans/2026-05-05-cognitrade-v1.md). The plan's phases map roughly to module build order (paths/db → ingest/embeddings → LLM/streaming → analyze flow → predictions/cost → tray/hotkey/UI → frontend → polish → release packaging).

Before starting:

1. Pick a milestone or scoped task from the roadmap (or an agreed issue).
2. Re-read the relevant module(s) and their inline `#[cfg(test)]` tests — they document expected behavior.
3. Confirm your change does not contradict [`../../CLAUDE.md`](../../CLAUDE.md) or the [safety rules](#9-safety-rules-non-negotiable). If it seems to, stop and raise it before coding.

---

## 5. Branch & commit workflow

> **This repo is not a git repository yet.** Initialize it before doing any version-control work:
>
> ```powershell
> git init
> git add -A
> git commit -m "chore: initial import"
> ```
>
> If you are the first to set this up, also add a `.gitignore` that excludes `node_modules/`, `src-tauri/target/`, build artifacts, and **all** `%USERPROFILE%\CogniTrade\` user data (it lives outside the repo, but never let a stray copy in).

Once git exists:

- **Never commit straight to the default branch.** Branch first.
- Use short, descriptive branch names: `feat/ingest-csv`, `fix/cost-cap-haiku-rate`, `docs/contributing`.
- Keep commits focused and conventional: `feat:`, `fix:`, `docs:`, `chore:`, `test:`, `refactor:`.
- One logical change per commit; do not mix a refactor with a feature.

**Commit and push only when the user/maintainer asks.** Don't auto-push.

---

## 6. Local dev loop

| Command | What it does |
| --- | --- |
| `npm install` | Install JS deps + Tauri CLI (run once after clone). |
| `npm run tauri dev` | Run the app in dev with frontend hot reload. |
| `npm run tauri build` | Produce a release installer (MSI via WiX / NSIS on Windows; see `tauri.conf.json`). |
| `npm run check` | `svelte-check` — Svelte/TypeScript typecheck. |
| `cd src-tauri && cargo test` | All backend Rust tests (see note below on what runs by default). |

Typical loop:

1. `npm run tauri dev` and leave it running. Frontend edits hot-reload; Rust edits trigger a backend rebuild + relaunch.
2. Make your change.
3. Run the relevant tests (next section).
4. `npm run check` for frontend type safety.

### Running tests

```powershell
# all backend tests that run by default (unit tests + the mocked llm_integration test)
cd src-tauri
cargo test

# a single module's tests
cargo test cost::tests
cargo test predictions::tests
```

`src-tauri/tests/` holds two integration files, and they behave **differently**:

- **`llm_integration.rs`** (`analyze_streams_text_and_parses_json`) is **not** `#[ignore]`d — it runs as part of the default `cargo test` suite. It is fully **mocked** via a `wiremock` `MockServer` serving a fake SSE body, and uses the literal key `"sk-test"`. It never touches the real Anthropic API: **no live key, no network, no cost.**
- **`ingest_integration.rs`** (`ingest_pdf_into_lancedb`) **is** `#[ignore]`d because it downloads the BGE-small embedding model on first run. Run it explicitly when you want the full ingest smoke test:

```powershell
# downloads the BGE-small embedding model on first run (CPU-only)
cargo test --test ingest_integration -- --ignored --nocapture
```

### Test conventions

- Each backend module keeps its tests inline in a `#[cfg(test)] mod tests` block (see `cost.rs`, `predictions.rs`, `paths.rs`, `settings.rs`, `llm/prompt.rs`).
- Use `tempfile` for DB-backed tests (open a temp `chat.db` / `vec.db`).
- Use `wiremock` to stub Anthropic / CoinGecko HTTP — never call the real APIs from a unit test (the `llm_integration` test is the reference pattern).
- Assertions: `pretty_assertions` is available.

---

## 7. Definition of done

A change is **done** only when all of the following hold:

- [ ] The behavior matches the milestone/task and contradicts nothing in `CLAUDE.md`.
- [ ] New or changed backend behavior has inline unit tests; `cargo test` is green.
- [ ] `npm run check` (svelte-check) is clean — no type errors.
- [ ] Frontend and backend serde shapes stay in sync (`src/lib/types.ts` mirrors the Rust structs; `Settings`, `Message`, `AnalysisJson`, `DailyCost`).
- [ ] No secrets in code, tests, fixtures, logs, or the DB. The API key stays in the keyring only.
- [ ] User-data paths are untouched as artifacts (nothing under `%USERPROFILE%\CogniTrade\` committed).
- [ ] Docs updated if behavior, commands, schema, or IPC surface changed.
- [ ] Errors flow through the single `AppError` enum (`error.rs`) — no stringly-typed `Result<_, String>` in new code.
- [ ] If you changed model IDs, `cost::rates_for` substring matching (`opus` / `haiku` / else→sonnet) still prices them correctly.

---

## 8. Pull-request checklist

Before opening a PR, confirm:

- [ ] **Tests pass** — `cd src-tauri && cargo test` is green (this already includes the mocked `llm_integration` test). If your change touches ingest, note whether you ran the `#[ignore]`d `ingest_integration` smoke test.
- [ ] **svelte-check clean** — `npm run check` reports no errors.
- [ ] **Docs updated** — any change to commands, schema (`migrations/`), the IPC surface, settings, or model defaults is reflected in `docs/` and, if structural, in `CLAUDE.md`.
- [ ] **No secrets** — grep your diff for keys/tokens; confirm nothing under `%USERPROFILE%\CogniTrade\` or any `.db` file is staged.
- [ ] **Windows build sanity** — for non-trivial changes, `npm run tauri build` completes and produces an installer; the app launches and the Chat / Library / Settings tabs render. At minimum, `npm run tauri dev` runs without panics on startup.
- [ ] **Safety** — your change adds **no** trade-execution, exchange-connection, or auto-execution code path (see §9). If it touches the recommendation/prediction surface, call that out explicitly in the PR description.

PR description should state: what milestone/task, what changed, how it was tested (commands run), and any divergence from the spec that a reviewer should be aware of.

---

## 9. Safety rules (non-negotiable)

CogniTrade is **advisory by design**. These rules protect that invariant:

1. **No auto-execution.** Do not add code that places, cancels, or modifies real orders, connects to an exchange (Binance/Kraken or any other), or otherwise acts on a recommendation without a human. Any such code requires **explicit, written maintainer sign-off in the PR** before it is merged — never sneak it in alongside other work.
2. **The recommendation is advice, not an instruction the program acts on.** The `AnalysisJson` tail carries `action`, `probability_up`, `horizon`, the optional `stop_loss_pct` / `take_profit_pct`, and the **required** `key_signals` (`string[]`) and `citations` (`Citation[]`) fields. The authoritative shape is `src/lib/types.ts` (`AnalysisJson`) mirrored from the system prompt in `llm/prompt.rs`. It is informational only — the app stores it as a `prediction` for self-calibration. Do not add a path that turns `action: "buy"` into an actual trade.
3. **`stop_loss_pct` / `take_profit_pct` are advisory numbers, not enforced.** The implemented prediction-resolution loop records price-at-horizon vs price-at-prediction; it does **not** simulate or enforce SL/TP. Do not claim or imply SL/TP enforcement exists.
4. **Keep secrets in the keyring.** Never log, serialize, or persist the API key outside Windows Credential Manager.
5. **Respect the hardware ceiling.** The target is a Ryzen 3400G / 8GB RAM / 450W PSU machine. The 8GB RAM ceiling is a hard constraint and drives model-size, concurrency, and embedding choices (e.g. CPU-only fastembed BGE-small behind a global mutex). Don't introduce a change that obviously blows the budget (e.g. a large local model) without flagging the trade-off.
6. **Don't present aspirational spec features as real.** When you mention something from `crypto Dhriti.txt` or `docs/superpowers/`, label it **"Planned — not yet implemented."**

If a task seems to require breaking one of these, **stop and escalate** rather than implementing it.

---

## 10. Adding an IPC command (three places, in order)

IPC commands are `#[tauri::command]` async functions in `src-tauri/src/ipc.rs`, returning `Result<T>` (the `AppError`-based alias). To add one, edit **all three** of these or the frontend can't call it / won't typecheck:

### 1. Define & register the command (Rust)

Write the command in `ipc.rs`:

```rust
#[tauri::command]
pub async fn my_command(state: State<'_, AppState>, foo: String) -> Result<MyResult> {
    // ... use state.db / state.lance / state.paths / state.llm
}
```

Then register it in the `tauri::generate_handler!` macro in `src-tauri/src/lib.rs` (the existing list ends with `ipc::analyze, ipc::ingest_path`):

```rust
.invoke_handler(tauri::generate_handler![
    ipc::list_recent_messages,
    // ... existing commands ...
    ipc::ingest_path,
    ipc::my_command,        // <-- add here
])
```

> A command that is defined but not added to `generate_handler!` is invisible to the frontend — `invoke('my_command')` fails at runtime.

### 2. Add a typed wrapper (frontend)

Add it to the `api` object in `src/lib/ipc.ts`. Tauri converts snake_case Rust arg names to the JS object keys you pass to `invoke`:

```ts
export const api = {
  // ... existing wrappers ...
  myCommand: (foo: string) => invoke<MyResult>('my_command', { foo }),
};
```

### 3. Add/extend shared types

Add `MyResult` (and any input shapes) to `src/lib/types.ts`, mirroring the Rust serde shape exactly. Keep field names identical to the Rust `#[derive(Serialize)]`/`Deserialize` struct (snake_case). Re-export from `ipc.ts` if other modules import it, following the existing `export type { ... }` pattern.

### Events vs. commands

If your feature streams data (like analysis does via `analysis_chunk` / `analysis_done`), emit a Tauri event from the backend (`app.emit("my_event", payload)`) and `listen` for it in a store under `src/lib/stores/` — see `src/lib/stores/chat.ts` for the pattern. Events are one-way push; commands are request/response.

---

## 11. Quick reference

| You changed… | Also update… |
| --- | --- |
| An IPC command | `ipc.rs` + `lib.rs` handler + `src/lib/ipc.ts` + `src/lib/types.ts` |
| A Rust struct sent to the frontend | the matching type in `src/lib/types.ts` |
| The DB schema | `src-tauri/migrations/001_initial.sql` (and any reader in `chat_store.rs` / `predictions.rs` / `cost.rs`) |
| Model IDs / defaults | `settings.rs` defaults **and** `cost::rates_for` |
| A new coin/ticker | the hardcoded `symbol_to_id` table in `coingecko.rs` (~12 coins; unlisted symbols error) |
| Chunk sizing | `chunker.rs` (`target_tokens` / `overlap_tokens`) and the `chunk_text(.., 500, 50)` calls in `ingest/mod.rs` |
| Commands or workflow | this guide + `CLAUDE.md` |

When in doubt, the inline module tests and [`../../CLAUDE.md`](../../CLAUDE.md) are the most reliable description of current behavior.
