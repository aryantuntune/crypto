# CogniTrade

CogniTrade is an **advisory crypto-chart analyst** — a local-first desktop app built on
**Tauri 2** with a **SvelteKit/TypeScript** frontend (Svelte 5 + Tailwind 3 + Vite 6, static
SPA) and a **Rust** backend. Windows is the primary platform, and the whole thing is designed
to run 24/7 on modest hardware (the target is a Ryzen 3400G with **8 GB RAM**). You paste or
drop a chart screenshot; CogniTrade reads it with Claude vision, retrieves relevant passages
from your own ingested PDFs/images (RAG), and streams back a plain-English analysis that ends
in a structured recommendation card.

> **CogniTrade advises — it never trades.** There is **no exchange integration
> (Binance/Kraken), no order execution, and no automated stop-loss/take-profit enforcement.**
> The system prompt is explicit: *"You are an advisor. The user places all trades
> themselves."* Capabilities from the original brief that are not built — exchange execution,
> a Minimax decision engine, automated SL/TP enforcement, and numerical indicator computation
> — are **Planned — not yet implemented**.

## Features

- **Chart reading via Claude vision** — a chart screenshot is sent to the Anthropic Messages
  API; the analysis streams back into a chat panel.
- **RAG over your own PDFs and images** — documents dropped into the library are chunked,
  embedded locally (fastembed BGE-small, 384-dim, CPU-only), and retrieved at analysis time;
  citations surface in the recommendation.
- **Probability + horizon recommendations with SL/TP** — each analysis ends in a structured
  JSON tail: `action` (buy/sell/hold), `probability_up`, `horizon` (1h/4h/1d/3d/1w), plus
  optional advisory `stop_loss_pct` / `take_profit_pct`. *SL/TP are advice only — nothing
  enforces them.*
- **Prediction self-calibration** — every recommendation is logged as a prediction and later
  resolved against the real CoinGecko price at its horizon, so you can judge the advisor's
  accuracy over time. *(Records endpoint price only; intra-horizon SL/TP simulation is
  Planned — not yet implemented.)*
- **Local-first & private** — all user data lives on disk; the Anthropic API key is stored in
  the OS keyring (Windows Credential Manager), never in the database; no telemetry. The only
  outbound calls are to Anthropic and CoinGecko.
- **Daily cost cap** — a hard per-day USD spend cap (default **$2**) is enforced before each
  main analysis call.
- **Ambient desktop shell** — a system tray app togglable with a global `Ctrl+Shift+Space`
  hotkey, with a three-tab UI (Chat / Library / Settings).

## Quickstart

```powershell
npm install            # install JS deps (once after clone)
npm run tauri dev      # run the app (dev) with hot reload
npm run tauri build    # produce a release MSI (WiX) / NSIS installer
```

**Windows prerequisites** (primary platform):

- **Rust** — the `stable-x86_64-pc-windows-msvc` toolchain (via `rustup`); *not* the GNU one.
- **Visual Studio C++ Build Tools** — provides `link.exe` and the Windows SDK that the MSVC
  toolchain and the bundled SQLite (`rusqlite`) link against.
- **Node.js LTS** — runs Vite, SvelteKit, and the `@tauri-apps/cli`.
- **WebView2 runtime** — renders the UI; pre-installed on Windows 11 and current Windows 10.

> The BGE-small embedding model downloads on first use, so the first ingest/analysis needs
> network access. The repository is **not** a git repository — run `git init` before any
> version-control operations.

To run an analysis you also need an **Anthropic API key**, set from the Settings tab (stored
in Windows Credential Manager).

### User data directory

All user data lives under `%USERPROFILE%\CogniTrade\`:

```
%USERPROFILE%\CogniTrade\
├─ library\       # drop PDFs/images here -> auto-ingested
├─ screenshots\   # saved chart screenshots
├─ data\          # chat.db + vec.db (SQLite, WAL)
└─ logs\
```

**Never commit anything under `%USERPROFILE%\CogniTrade\`** — it is user data.

## Documentation

See [`docs/README.md`](docs/README.md) for the full documentation index — project vision and
scope, architecture, the IPC/events API, the data model, Windows setup and build/release,
testing, security, the user guide, and the gap analysis. The authoritative codebase guide is
[`CLAUDE.md`](CLAUDE.md).
