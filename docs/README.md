# CogniTrade Documentation

This is the documentation index for **CogniTrade**, a Tauri 2 desktop app that acts as an
**advisory crypto-chart analyst** — *not* an autonomous trader. You paste or drop a chart
screenshot; the backend reads it with Claude vision, retrieves passages from your ingested
PDFs/images (RAG), and streams back a plain-English analysis ending in a structured JSON
recommendation. There is **no exchange integration, no order execution, and no automated
stop-loss/take-profit enforcement**. The system prompt is explicit: *"You are an advisor.
The user places all trades themselves."*

For the full picture of which spec capabilities exist versus which are **Planned — not yet
implemented**, see the
[Gap Analysis & Completion Plan](./guides/gap-analysis-and-completion-plan.md).

Authoritative codebase guide: [`../CLAUDE.md`](../CLAUDE.md). Original product brief
(aspirational, diverges from the code): [`../crypto Dhriti.txt`](../crypto%20Dhriti.txt).

---

## Start here (recommended reading order — new developer on Windows)

1. [Project · Vision & Scope](./project/vision-and-scope.md) — understand the advisory-only
   boundary and what is in/out of scope before touching anything.
2. [Architecture · Overview](./architecture/overview.md) — the single `ipc::analyze` flow,
   the component diagram, and the concurrency model.
3. [Development · Windows Setup](./development/windows-setup.md) — install the Rust MSVC
   toolchain, Node, WebView2, and C++ Build Tools.
4. [Development · Build & Release](./development/build-and-release.md) — run
   `npm run tauri dev`, then produce an MSI/NSIS installer.
5. [Architecture · Tech Stack](./architecture/tech-stack.md) and
   [Module Reference](./architecture/module-reference.md) — the crates and the per-module map.
6. [Development · Coding Conventions](./development/coding-conventions.md) and
   [Contributing](./development/contributing.md) — match the codebase idioms and the
   safety rules.
7. [Guides · Gap Analysis & Completion Plan](./guides/gap-analysis-and-completion-plan.md) —
   what is missing, what it takes to finish, and how long.

---

## Project

| Document | Description |
| --- | --- |
| [Vision & Scope](./project/vision-and-scope.md) | Reconciles the aspirational brief against the implemented advisory app; the authoritative scope boundary for v1. |
| [Product Requirements (PRD)](./project/prd.md) | Implemented functional requirements (FR-1…FR-13), planned ones, acceptance criteria, and a requirements traceability table to source modules. |
| [Roadmap & Milestones](./project/roadmap.md) | Path from the current advisory app to a signed Windows release (M1–M3) and the gated, optional execution work (M4). |
| [Risk Register](./project/risk-register.md) | Technical, financial/safety, and operational risks with likelihood, impact, mitigations, and owners. |

## Architecture & Resources

| Document | Description |
| --- | --- |
| [System Overview](./architecture/overview.md) | High-level component diagram, the `ipc::analyze` sequence, the ingestion path, the prediction loop, and the concurrency model. |
| [Tech Stack & Dependencies](./architecture/tech-stack.md) | Every backend crate and frontend package, with the rationale behind each choice (Tauri vs Electron, SQLite, fastembed, rustls). |
| [Backend Module Reference](./architecture/module-reference.md) | Per-module reference for the Rust backend under `src-tauri/src/`. |
| [Frontend Architecture](./architecture/frontend-architecture.md) | The SvelteKit static SPA: three-tab shell, stores, typed IPC wrappers, and the screenshot → analyze flow. |
| [IPC & Events API Reference](./architecture/ipc-api-reference.md) | Every Tauri command and event across the Rust ↔ Svelte boundary, payloads, and error serialization. |
| [Data Model & Storage](./architecture/data-model.md) | Every persistent store: the two SQLite DBs (`chat.db`, `vec.db`), their tables, the keyring secret, and the on-disk file tree. |
| [Hardware & Resource Budget](./architecture/hardware-budget.md) | The 8 GB RAM target broken down by consumer, and the design choices it forces. |

## Development

| Document | Description |
| --- | --- |
| [Windows Developer Setup](./development/windows-setup.md) | From a fresh Windows machine to a running `npm run tauri dev`: MSVC Rust toolchain, Node, WebView2, C++ Build Tools. |
| [Build & Release (Windows)](./development/build-and-release.md) | Producing and distributing the Windows MSI (WiX) / NSIS installer, plus the WebView2 runtime requirement. |
| [Coding Conventions & Patterns](./development/coding-conventions.md) | The idioms to follow: the single `AppError` type, IPC patterns, store usage, and test placement. |
| [Contributing Guide](./development/contributing.md) | Repo layout, the dev loop, the definition of done, the PR checklist, and the non-negotiable safety rules. |
| [Testing Strategy](./development/testing.md) | How CogniTrade is tested today, how to run the suites (including the `#[ignore]`d integration tests), and where coverage is weak. |
| [Troubleshooting](./development/troubleshooting.md) | Fixes for common build, run, and runtime problems on Windows. |

## Guides

| Document | Description |
| --- | --- |
| [User Guide (Windows)](./guides/user-guide.md) | Installing, configuring, and using CogniTrade end-to-end: API key, cost cap, ingesting knowledge, running an analysis, and reading the recommendation. |
| [Security, Secrets & Privacy](./guides/security.md) | The security posture as implemented: the single keyring-stored API key, local-only data, and the outbound calls (Anthropic, CoinGecko). |
| [Glossary](./guides/glossary.md) | Domain, AI/RAG, and codebase terms, with each spec-only term flagged Planned — not yet implemented. |
| [Gap Analysis & Completion Plan](./guides/gap-analysis-and-completion-plan.md) | The capability matrix (spec vs. code), what each gap takes to finish, and effort estimates. |

---

> **Note on scope.** Throughout these docs, any capability from the original brief that is
> not in the code — exchange integration (Binance/Kraken), order execution, a Minimax
> decision engine, automated SL/TP enforcement, numerical indicator computation — is
> labelled **Planned — not yet implemented**. Never treat those as existing features.
