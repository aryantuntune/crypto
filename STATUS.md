# Project Status & Build Handoff

**Last updated:** 2026-06-09 · **Branch:** `main`

This is the "pick up where we left off" file. It records the exact current state of the
build, what was just changed, and the **single remaining step** to turn the code into a
shippable Windows installer. For deeper detail see [`docs/README.md`](docs/README.md).

---

## TL;DR — what to do next

The app is **code-complete for Track A** and the frontend is type-checked clean. The only
thing left is to **compile and package on a Windows machine** (this could not be done in the
Linux session where the code was written — see [Why Windows](#why-this-must-finish-on-windows)).

```powershell
# From the repo root on Windows, with prerequisites installed (see below)
npm install                         # restore JS deps (node_modules is gitignored)

cd src-tauri
cargo test                          # backend unit tests — THE real compile gate
cargo test -- --ignored             # integration tests: network + first-run BGE-small model
                                    #   download; the LLM test needs a live Anthropic API key
cd ..

npm run tauri build                 # produces the NSIS + MSI installers
# -> output under src-tauri/target/release/bundle/
```

If `cargo test` reports a compiler error, it will be a type/trait issue that source
inspection could not catch (the Rust was never compiled). Paste the error back and it can be
fixed quickly.

### Prerequisites (one-time, on Windows)

- **Rust** via `rustup` with the **MSVC** toolchain (`stable-x86_64-pc-windows-msvc`)
- **Microsoft Visual C++ Build Tools** (the "Desktop development with C++" workload)
- **WebView2 Runtime** — preinstalled on Windows 11; on Windows 10 install the Evergreen
  Bootstrapper (the installer is also configured to fetch it at runtime)
- **Node.js LTS** + npm

Full step-by-step: [`docs/development/windows-setup.md`](docs/development/windows-setup.md)
and [`docs/development/build-and-release.md`](docs/development/build-and-release.md).

---

## Where the project stands

| Area | State |
|------|-------|
| Documentation suite (`docs/`, 22 files) | ✅ Complete |
| Track A — polish to a shippable Windows build | ✅ Code-complete, frontend type-checked |
| Frontend `npm run check` (svelte-check) | ✅ 0 errors (1 cosmetic `@types/node` warning) |
| Backend `cargo test` / `cargo build` | ⏳ **Not yet run** — needs Windows (no Rust toolchain in the dev session) |
| Windows installer (`npm run tauri build`) | ⏳ Pending the build above |
| Track B — paper-trading, real indicators, guarded execution | ⛔ Not started (see plan) |

---

## What changed in Track A (not yet reflected in every doc)

The `docs/` suite was generated **before** Track A landed, so a few documents describe these
items as future work. The actual code now includes:

**Backend**
- New IPC commands: `list_documents`, `delete_document`, `embeddings_ready`
  (in `src-tauri/src/ipc.rs`, registered in `src-tauri/src/lib.rs`).
- Embedding-model **warm-up** at startup on a background thread
  (`src-tauri/src/ingest/embeddings.rs`: `is_ready()`, `warmup()`), so the first analysis
  isn't blocked by model init/download.
- Vector store (`src-tauri/src/lance.rs`) now stores **unit-normalized** embeddings and ranks
  by dot product (same cosine order, faster); added `DocInfo` + `list_docs()`.
- `coingecko.rs` symbol map expanded from 12 to ~70 coins, with a request timeout + one retry.
- Friendlier user-facing `AppError` messages; daily-cost-cap input validation.

**Frontend (`src/`)**
- Dismissible **error banner** (backend errors no longer leak into the chat stream).
- **API-key gating** (Analyze disabled until a key is set, with a jump-to-Settings prompt).
- First-run **"preparing local search model…"** hint (polls `embeddings_ready`).
- Empty states; a working **Library tab** (list / add via file picker / delete documents);
  richer **Settings** (extract model, library path, hotkey, cap validation).

**Packaging**
- `tauri.conf.json`: product name `CogniTrade`, Windows `nsis`+`msi` targets, WebView2
  download bootstrapper. `Cargo.toml`: release-size profile. `capabilities/default.json`:
  least-privilege, scoped to the library dir.

> If you want the architecture/module docs reconciled with these deltas, that's a small
> follow-up pass — ask and it can be regenerated.

---

## Why this must finish on Windows

The code was authored from a Linux session that has **no runnable Rust toolchain** (only a
Windows toolchain on a mounted drive, which can't execute on Linux). Frontend type-checking
(`npm run check`) ran there and passed; the Rust was verified by careful inspection only. The
Tauri bundler is platform-native, so the Windows `.msi`/NSIS installer must be built on
Windows regardless.

---

## After the build — what's next

Track B (the path toward the original autonomous-trading vision) is laid out in
[`docs/guides/gap-analysis-and-completion-plan.md`](docs/guides/gap-analysis-and-completion-plan.md).
Recommended sequence: **paper-trading + real RSI/MACD/BB computation → backtesting →
guarded exchange execution with pre-trade approval** — never wire real-money execution before
the paper-trading and backtesting layers exist.
