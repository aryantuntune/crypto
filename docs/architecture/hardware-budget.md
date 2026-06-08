# Hardware & Resource Budget Analysis

> **Scope.** This document analyzes the **as-built** CogniTrade desktop app against its hard
> hardware target: an **AMD Ryzen 3400G, 8 GB RAM, 1 TB storage, 450 W PSU** machine running
> the app **24/7** on Windows. It is a budget and capacity-planning document, not a design
> proposal. Where a capability is described in the original spec but is **not** in the code,
> it is labelled **Planned — not yet implemented**.
>
> Cross-links: [`../../CLAUDE.md`](../../CLAUDE.md) (authoritative codebase summary),
> the original brief [`../../crypto Dhriti.txt`](../../crypto%20Dhriti.txt), and the
> aspirational design under [`../superpowers/`](../superpowers/).

## 0. What is actually running (the thing we are budgeting)

CogniTrade is a **Tauri 2** desktop app: a Rust backend (`src-tauri/`) plus a SvelteKit /
TypeScript frontend (`src/`) compiled to a static SPA and rendered inside the OS WebView. It
is an **advisory** crypto-chart analyst — the user pastes/drops a chart screenshot, the
backend reads it with Claude (vision) and retrieved PDF/image passages (RAG), and streams back
a plain-English analysis ending in a structured JSON recommendation.

There is **no exchange integration, no order execution, no Minimax engine, and no automated
SL/TP enforcement** — those are all **Planned — not yet implemented**. The system prompt is
explicit: *"You are an advisor. The user places all trades themselves."* This dramatically
shrinks the resource footprint we have to budget for: the heavy, always-on components a real
trading bot would need (continuous WebSocket order books, a backtester, a live OMS) do **not**
exist in the codebase today.

**Build output / Windows runtime.** The bundle is configured with `"targets": "all"` in
[`../../src-tauri/tauri.conf.json`](../../src-tauri/tauri.conf.json) — it is **not** pinned to
a Windows-specific MSI/NSIS target; `npm run tauri build` produces whatever installer formats
Tauri selects for the host platform (on Windows, WiX MSI / NSIS). The relevant runtime
dependency on Windows is the **WebView2 runtime** — the 24/7 Windows budget below **assumes
WebView2 is installed** (it ships with current Windows 10/11). Build-time it also needs the
Rust MSVC toolchain and the Visual Studio C++ Build Tools, but those do not affect the runtime
RAM budget.

The processes/threads that consume the 8 GB budget are:

| Component | Where | Always resident? |
|---|---|---|
| OS WebView (WebView2 on Windows) | hosts the SvelteKit SPA | Yes, while the panel is open |
| Rust backend process (`cognitrade-app`) | `src-tauri/` | Yes (tray + background loops) |
| fastembed BGE-small model | `ingest::embeddings`, lazy global singleton | Only after first embed call |
| `data/chat.db` SQLite connection | `db::open` | Yes |
| `data/vec.db` SQLite connection | `lance::open`, lazy | After first ingest/analyze |
| Brute-force cosine working set | `lance::LanceStore::search` | Transient, per query |

The Rust process is what stays up 24/7. From `lib.rs::run`, on startup it:

- creates `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` (`Paths::ensure_all`);
- opens `chat.db` (`db::open`);
- spawns the **prune/resolve background loops** (`prune::spawn_background`);
- spawns the **library file watcher** (`ingest::watcher::run_watcher`);
- registers a system tray and the `Ctrl+Shift+Space` global hotkey to toggle the panel.

Everything else (the vector store, the embedding model, the LLM HTTP calls) is created
**lazily** on first use, which is the single most important fact for the idle memory budget.

---

## 1. The hard 8 GB RAM constraint, subsystem by subsystem

8 GB is the **non-negotiable ceiling**, and on Windows a meaningful slice is gone before our
app starts. Plan for the OS + desktop to hold **~2.0–3.0 GB** resident on an 8 GB box. That
leaves CogniTrade a realistic working envelope of roughly **5 GB**, and we must never assume
the whole 8 GB is ours.

### 1.1 WebView2 (the frontend host)

The frontend is a static SvelteKit SPA (`adapter-static`, `frontendDist: "../build"` in
[`../../src-tauri/tauri.conf.json`](../../src-tauri/tauri.conf.json)) rendered by the OS
WebView2 runtime — a separate browser process tree (`msedgewebview2.exe`), **not** counted
inside the Rust process. WebView2 is the largest single consumer we do not control:

- Expect **~150–350 MB** resident for a small, mostly-static three-tab UI (Chat / Library /
  Settings, `src/routes/+page.svelte`), spread across WebView2's multi-process model
  (browser + renderer + GPU).
- The window is tiny and single-purpose (`width: 420, height: 800`, `decorations: false`,
  `skipTaskbar: true`), and the SPA holds only chat history in memory via the Svelte stores
  (`src/lib/stores/chat.ts`). There is no chart-rendering canvas, no live data grid, no
  heavy client library — the UI is deliberately thin.
- **Lever:** the window is created `"visible": false` and toggled from the tray/hotkey. When
  the user hides the panel, WebView2 is backgrounded and the OS trims its working set; idle
  RAM drops accordingly. 24/7 operation should be **tray-resident with the panel hidden**,
  not panel-open.

### 1.2 The Rust backend process

The native process itself is modest. It is `tokio` (`features = ["full"]`), `rusqlite`
(bundled SQLite), `reqwest` (rustls-tls), and the Tauri runtime. With no model loaded and no
query in flight, expect the Rust process to sit around **~80–200 MB** resident. Its long-lived
state (`AppState` in `ipc.rs`) is two SQLite connections, a `Paths` struct, an `LlmClient`
(an idle `reqwest::Client`), and an `Arc<Mutex<Option<LanceStore>>>` that is `None` until the
first analyze/ingest. The background loops (`prune::spawn_background`) are two sleeping `tokio`
tasks that wake every **6 h** (chat prune) and **60 s** (prediction resolution) — negligible.

### 1.3 fastembed BGE-small — the resident model

This is the one ML model in the process. `ingest::embeddings` wraps **fastembed**'s
`BGESmallENV15` (384-dim, `EMBED_DIM = 384`) as a **lazy global singleton**:

```rust
static MODEL: OnceLock<Mutex<TextEmbedding>> = OnceLock::new();
```

Key budget facts, all confirmed in `src-tauri/src/ingest/embeddings.rs`:

- **CPU-only and synchronous behind a `Mutex`.** No GPU path, no async. This is a deliberate
  choice for the 8 GB / iGPU target — see §2 and §4.
- **Lazy:** the model is downloaded and loaded on **first** `embed()` call, never at startup.
  An idle install that has never ingested a document or run an analysis **does not** pay this
  cost at all. (But note: the first *analyze* always triggers it too — see §1.6.)
- **Resident size:** BGE-small-en-v1.5 is ~33 M parameters. The code constructs it with
  `TextEmbedding::try_new(InitOptions::new(EmbeddingModel::BGESmallENV15))` and **defaults** —
  it does **not** pin a precision/quantization, so the exact weights are whatever the default
  ONNX artifact fastembed downloads for that model (precision per fastembed's default, not
  fixed in our code). Loaded under fastembed + the ONNX Runtime it pulls in, **estimate
  ~150–400 MB** resident once loaded, including the runtime's arenas. It is small precisely
  *because* of the 8 GB ceiling — a larger embedding model (e.g. BGE-large at 1024-dim) was
  correctly avoided.
- **Single-flight:** the `Mutex` serializes all embedding. Concurrent ingests/queries do not
  multiply model memory; they queue. This caps peak RAM at the cost of throughput.

> **Important:** there is **no local LLM**. All generation (chart analysis, symbol extraction,
> image description) goes to the **Anthropic API** over HTTPS (`llm::client`, `ipc.rs`). The
> only thing resident on the 8 GB box is the *embedding* model. Running a local 7B/70B LLM
> would blow the budget instantly and is **not** done.

### 1.4 SQLite (two databases)

Two separate SQLite files, both **WAL mode** (`db::open`, `lance::open`):

- `data/chat.db` — `messages`, `predictions`, `cost_daily`, `settings`
  ([`../../src-tauri/migrations/001_initial.sql`](../../src-tauri/migrations/001_initial.sql)).
- `data/vec.db` — single `doc_chunks` table (the misnamed "Lance" store, see §1.5).

SQLite's resident cost is dominated by its **page cache** (default ~2 MB per connection) plus
WAL. For `chat.db` this is trivial — chat/prediction/cost rows are tiny and the chat table is
pruned to 7 days (§3). The memory risk lives entirely in `vec.db`, below.

> **Secrets are not in either DB.** The Anthropic API key is stored in the **OS keyring**
> (`settings.rs`, `keyring` crate; service `"cognitrade"`, user `"anthropic_api_key"`), which
> on Windows is the **Credential Manager**. It is therefore outside both SQLite files and
> outside the disk-growth budget (§3). Only **non-secret** settings live in the `settings`
> table. See §3 for the full data-location picture.

### 1.5 Brute-force cosine — the memory cost that grows with the library

`lance.rs` / `LanceStore` / `paths::lancedb()` are **misnomers**: there is **no LanceDB
dependency**. `vec.db` is plain SQLite, embeddings stored as little-endian `f32` **BLOBs**
(`embedding_to_bytes`). The critical line is `LanceStore::search`:

```rust
let mut stmt = conn.prepare(
    "SELECT id, doc_path, doc_type, chunk_index, text, page, embedding FROM doc_chunks",
)?;
// ... loads EVERY row, computes cosine() in Rust, sorts, truncates to k
```

**Every query loads every row** into Rust, decodes each BLOB to a `Vec<f32>`, computes cosine
similarity, sorts, and truncates to `k`. There is **no ANN index** (`idx_doc_chunks_path` is
only for delete-by-path replace). Memory implications:

- **Per-chunk fixed cost:** 384 dims × 4 bytes = **1,536 bytes** of embedding, plus the chunk
  **text**, plus row overhead. Chunks are produced by `chunker::chunk_text(text, 500, 50)`
  (called from `ingest/mod.rs`): a **500-token target with 50-token overlap**, where the
  chunker uses a **1-token ≈ 4-chars heuristic**, so `target_chars = 2000` (≈2 KB of text,
  ~330–350 English words — *not* 500 words). Sentence-boundary back-off can land a chunk a bit
  under or over that. Call it **~4–6 KB resident per chunk while a query is in flight**
  (≈1.5 KB embedding + ~2 KB text + row overhead), because `search` materializes the *entire*
  table (embeddings **and** text) into a `Vec` before truncating.
- This is a **transient** peak per query, not permanently resident — but it scales **O(N)**
  with the library and competes with WebView2 + the embedding model for the same 5 GB.

Rough envelope for the brute-force working set (embeddings + text materialized per query):

| Chunks in `doc_chunks` | ≈ PDF pages ingested | Transient peak per query |
|---|---|---|
| 1,000 | ~150–250 pages | ~5 MB |
| 10,000 | ~1,500–2,500 pages | ~50 MB |
| 50,000 | ~7,500–12,500 pages | ~250 MB |
| 200,000 | ~30,000–50,000 pages | ~1 GB ⚠️ |

At a few hundred PDF pages the cost is noise. At tens of thousands of chunks the **per-query
allocation** starts to matter against the 8 GB ceiling, and at ~200k chunks a single query's
working set approaches a gigabyte. That is the point where the brute-force scan must be
replaced with an index (§4). For the intended use — a personal library of trading books and
reports — we expect to live comfortably in the top two rows.

### 1.6 Idle vs. active RAM summary

| State | WebView2 | Rust proc | Embed model | Vector scan | Approx total (app only) |
|---|---|---|---|---|---|
| Idle, panel **hidden**, never embedded | trimmed (~80–150 MB) | ~80–150 MB | not loaded | none | **~0.2–0.3 GB** |
| Idle, panel **open** | ~150–350 MB | ~80–200 MB | maybe loaded | none | **~0.3–0.6 GB** |
| Active analyze (model loaded, query running, small library) | ~200–350 MB | ~150–300 MB | ~150–400 MB | ~5–50 MB | **~0.6–1.1 GB** |
| Active analyze, **large** library (50k chunks) | ~250–350 MB | ~200–350 MB | ~150–400 MB | ~250 MB | **~0.9–1.4 GB** |

> **The embedder always loads on the first analyze, not only on first ingest.** Every
> `ipc::analyze` calls `retrieval::search`, which unconditionally calls
> `embeddings::embed(vec![query])` to embed the query *before* the cosine scan (`retrieval.rs`,
> reached from `ipc.rs`). So even with an **empty** library the very first analysis loads the
> ~150–400 MB model. The "never embedded → not loaded" idle rows above are accurate only while
> the user has neither ingested a document nor run a single analysis; every "active analyze"
> row therefore includes the embedder's resident cost.

**Verdict:** the as-built app fits the 8 GB target with large headroom (see §5). The 8 GB
ceiling is respected because (a) there is no local LLM, (b) the embedding model is small,
lazy, and single-flight, and (c) the only O(N) growth term is the per-query vector scan, which
stays small for a personal-scale library.

---

## 2. CPU and thermal considerations for 24/7 operation

### 2.1 No GPU / no discrete card needed

The target is a **Ryzen 3400G** — a 4-core/8-thread APU with integrated Vega graphics, on a
**450 W** PSU. CogniTrade requires **no discrete GPU and no GPU compute**:

- The embedding model runs **CPU-only** (fastembed default execution provider; no CUDA/DML
  path is configured). See `ingest::embeddings`.
- There is **no local LLM inference** — all generation is remote (Anthropic API).
- "Visual chart reading" is done by **Claude vision over the API**, not a local vision model.
  A local on-device vision/OCR pipeline is **Planned — not yet implemented**.
- The only GPU work is the iGPU compositing the small WebView2 window — trivial.

So the 450 W PSU and iGPU-only configuration are comfortably sufficient; there is no
power-hungry, always-on compute element in the as-built design.

### 2.2 What actually uses CPU, and when

The app is overwhelmingly **idle and I/O-bound**, with short, bursty CPU spikes:

| Activity | CPU profile | Frequency |
|---|---|---|
| Tray-resident idle | ~0% (two sleeping `tokio` timers) | continuous |
| Prediction-resolution loop (`prune.rs`) | wakes every **60 s**, an HTTP poll to CoinGecko per *due* prediction only | continuous, negligible |
| Chat prune loop (`prune.rs`) | one `DELETE` every **6 h** | continuous, negligible |
| Library watcher (`ingest::watcher`) | a `recv_timeout` poll every **200 ms**; debounces events **800 ms** before queuing | continuous, negligible |
| `analyze` LLM stream | mostly **network-bound** (SSE from Anthropic); CPU is light SSE parsing | per user request |
| **Embedding** (ingest or per-analyze query embed) | **CPU-bound burst**, all cores via ONNX Runtime, behind the `Mutex` | per ingest / per analyze |
| Brute-force cosine scan | CPU-bound burst, O(N) over the library | per analyze/search |
| PDF text extraction (`pdf-extract`) + image decode/resize (`image`, Lanczos3) | CPU-bound burst | per ingested file |

The two genuinely CPU-heavy operations are **embedding** and the **brute-force cosine scan**,
and both are **transient, user-triggered bursts**, not sustained load. Crucially, embedding is
**single-flight** (one global `Mutex`), so it cannot saturate all cores indefinitely from
concurrent requests — it serializes. The watcher (`ingest::watcher`) `tokio::spawn`s an ingest
per *ready* (debounced) file, but each ultimately queues on the same embedding mutex.

### 2.3 Thermals for an always-on desktop

Because steady-state CPU is near-idle and the heavy work is short and bursty, **thermals are
not a concern at the app level** for this APU. The relevant 24/7 considerations are
operational, not computational:

- **Burst clustering:** dropping a large folder of PDFs into the library dir queues many
  ingests. The watcher (`ingest::watcher`, `Duration::from_millis(800)`) **debounces each
  file's events by 800 ms** before it becomes "ready" — so a file that is still being written
  or rapidly re-modified keeps re-debouncing and is not queued until it settles. Once ready,
  each file is `tokio::spawn`'d, but all ingests still **serialize on the single embedding
  `Mutex`**, so you get a sustained (but single-pipeline) CPU burn until the queue drains.
  This is the one scenario that warms the APU for minutes. **Lever:** ingest large libraries in
  batches, or off-hours (§4).
- **Disk wakeups:** the 60 s resolution loop, the 6 h prune loop, and the watcher's 200 ms
  poll keep WAL active enough that the OS won't aggressively park the disk; on a desktop this
  is fine.
- **No GPU thermal load**, no fan ramp from compute — only the iGPU compositing a tiny window.

There is no need for elevated cooling beyond a standard desktop heatsink for this workload.

---

## 3. Disk / storage growth

The 1 TB drive is enormous relative to anything the as-built app produces. All user data lives
under `%USERPROFILE%\CogniTrade\` (created by `Paths::ensure_all`; **never** committed):

| Path | Producer | Growth driver | Bounded? |
|---|---|---|---|
| `data\chat.db` | `db::open` | chat messages, predictions, cost rows | **Yes** — chat pruned to **7 days** |
| `data\vec.db` | `lance::open` | one row per ingested chunk (`doc_chunks`) | No — grows with library |
| `screenshots\` | `ipc::save_screenshot` | one PNG per analyzed chart | **No automatic prune** ⚠️ |
| `library\` | user-dropped files | user-controlled | user-controlled |
| `logs\` | `tracing` | log output | depends on log config |
| fastembed model cache | fastembed (first use) | one-time model download | one-time |

> **Where the API key lives (and where it does *not*).** The four directories above are the
> *entire* on-disk footprint under `%USERPROFILE%\CogniTrade\`. The Anthropic API key is **not**
> among them: it is stored in the Windows **Credential Manager** via the `keyring` crate
> (service `"cognitrade"`, user `"anthropic_api_key"`, `settings.rs`). So secrets are outside
> the SQLite DBs and outside this disk-growth budget entirely — relevant for backup/exclude
> rules on a 24/7 Windows box.

### 3.1 `vec.db` (the vector store)

Each chunk row is `id, doc_path, doc_type, chunk_index, text, page, embedding`. The embedding
BLOB is fixed at **1,536 bytes** (384 × 4). The `text` field targets ~2 KB per chunk
(`chunk_text(.., 500, 50)` → 500-token target × the 1-token≈4-char heuristic = ~2000 chars,
§1.5). With overhead, budget **~4–6 KB on disk per chunk**. So:

- 10,000 chunks ≈ **~40–60 MB**.
- 50,000 chunks ≈ **~200–300 MB**.

Even a very large personal library is a few hundred MB — negligible on 1 TB. The disk growth
of `vec.db` is a **RAM/CPU concern (the O(N) scan, §1.5), not a storage concern.**

### 3.2 `screenshots\` — the only unbounded grower to watch

`ipc::save_screenshot` writes one PNG per analyzed chart (`{stem}-{uuid}.png`) and **never
deletes them**. Charts are resized in-place to a `MAX_LONG_EDGE = 1568` px / `MAX_BYTES = 5 MB`
ceiling (`image_util.rs`) before analysis, so each is bounded to **≤ ~5 MB** (typically far
less). At, say, 50 analyses/day this is on the order of tens of MB/month — irrelevant against
1 TB for years — but it grows **monotonically**. A screenshot-retention prune is
**Planned — not yet implemented**; today it is a manual cleanup of `%USERPROFILE%\CogniTrade\screenshots\`.

### 3.3 `chat.db` — effectively self-bounding

`prune::spawn_background` runs `chat_store::prune_older_than_secs(.., 7 * 86400)` every 6 h, so
`messages` is capped to a 7-day rolling window. `predictions`, `cost_daily`, and `settings` are
tiny and not pruned, but they accrete at most a handful of rows per analysis/day. `chat.db`
stays in the low MB range indefinitely.

### 3.4 fastembed model cache

On first embed, fastembed downloads BGE-small (and ONNX Runtime artifacts) to its cache
directory. This is a **one-time, low-hundreds-of-MB** download; it does not grow. (It also
means the very first ingest/analyze on a fresh install has extra latency and requires network —
the same download the integration test `cargo test --test ingest_integration -- --ignored`
triggers.)

---

## 4. What must be offloaded if scope expands — and what stays local

The as-built app stays **entirely local** by virtue of being advisory. The spec's ambitions are
what would break the 8 GB / 450 W / single-APU budget. Per the spec's hardware analysis, the
following must be **offloaded** (VPS/cloud) if/when those features are built. All are currently
**Planned — not yet implemented**:

| Capability (spec) | Status | Local or offload | Why |
|---|---|---|---|
| Chart analysis generation (LLM) | **Implemented** | **Already offloaded** to Anthropic API | No local LLM fits 8 GB; correct as-is |
| Document embedding (BGE-small) | **Implemented** | **Local** | Small, CPU-only, lazy, single-flight — fits |
| RAG retrieval | **Implemented (brute-force)** | **Local** | Fine at personal scale; needs an index at large scale (below) |
| Price lookups (CoinGecko) | **Implemented** | **Local** (on-demand HTTP) | Lightweight, polled only for due predictions |
| Heavy **backtesting** over historical data | **Planned** | **Offload** | CPU/RAM/IO-heavy, sustained — would starve the 8 GB box and warm the APU continuously |
| **Large** models (bigger embedder, local LLM, local vision) | **Planned** | **Offload** | Violates the 8 GB ceiling; no GPU on the APU for serious inference |
| **Continuous** market-data feeds (live WebSocket order books, tick streams) | **Planned** | **Offload** | Always-on, unbounded memory/IO; the as-built app deliberately uses on-demand pulls, not streams |
| Exchange integration / order execution / Minimax engine / auto SL/TP | **Planned** | n/a (out of scope as-built) | Not present in code at all |
| Vector index at large scale (ANN) | **Planned** | **Local (memory-mapped/on-disk preferred)** or offload | An *in-process* ANN index has its own resident RAM cost — see §6.1 |

**Note on the ANN row.** Adding an index is **not** pure upside on an 8 GB box. A real
in-process ANN backend (HNSW/IVF) keeps its index either **fully resident** or memory-mapped,
and that footprint **grows with N** — trading the *transient* per-query scan for a *persistent*
resident structure. On the 8 GB ceiling, the cheap "don't materialize text in the scan"
optimization (§6.1) plus a **memory-mapped / on-disk** ANN is preferable to a fully-resident
in-process index for very large libraries.

**Stays local (today and intended):** the Tauri shell, WebView2 UI, both SQLite DBs, the
BGE-small embedder, brute-force-or-indexed retrieval, screenshot capture/resize, on-demand
CoinGecko price lookups, and the prune/resolve loops. **Goes remote (today):** all Claude
generation. **Must go remote if built (per spec):** backtesting, large models, and continuous
market data.

The guiding rule, straight from the hard constraints: **anything that cannot fit the 8 GB /
single-APU / 450 W budget must be explicitly flagged for offload — never silently run locally.**

---

## 5. Concrete resource budget table & headroom

Assumptions: Windows 8 GB box with **WebView2 installed**; OS + desktop reserve ~2.5 GB; a
**personal-scale** library (~10,000 chunks); a single analyze in flight (model loaded — and
recall from §1.6 that *any* analyze loads the embedder). This is the realistic steady worst
case for the as-built app.

| Subsystem | Idle (panel hidden) | Active analyze (10k-chunk lib) |
|---|---:|---:|
| Windows OS + desktop (reserved) | ~2,500 MB | ~2,500 MB |
| WebView2 (UI) | ~120 MB (trimmed) | ~300 MB |
| Rust backend process | ~120 MB | ~250 MB |
| fastembed BGE-small (resident) | 0 MB (not loaded) | ~300 MB |
| SQLite page caches (both DBs) | ~10 MB | ~20 MB |
| Brute-force cosine working set | 0 MB | ~50 MB |
| Transient buffers (image b64, SSE, chunk vecs) | ~0 MB | ~80 MB |
| **App subtotal (excl. OS)** | **~250 MB** | **~1,000 MB** |
| **Total (incl. OS)** | **~2.75 GB** | **~3.5 GB** |
| **Headroom vs. 8 GB** | **~5.25 GB** | **~4.5 GB** |

**Headroom estimate.** The app uses **~1 GB at active peak** for a personal-scale library,
leaving roughly **4–4.5 GB free** after the OS — a comfortable margin on an 8 GB machine. The
budget only tightens in two scenarios, both addressed in §6:

1. **A very large library** (≥ ~100k chunks): the O(N) brute-force scan's per-query working set
   climbs toward the high-hundreds-of-MB / GB range (§1.5). This is the **first thing to break**
   and the clearest trigger for adding an index — but note an in-process ANN index would itself
   become *resident* RAM (§4, §6.1).
2. **A bloated WebView2** if the panel is left open with a long, un-pruned chat — mitigated by
   the 7-day chat prune and by keeping the panel **hidden** when idle.

The 8 GB ceiling is **not** at risk for the intended workload; it becomes a real constraint only
if the library grows by 1–2 orders of magnitude beyond personal scale, or if any **Planned**
feature (local LLM, backtester, live feeds) is built locally instead of offloaded.

---

## 6. Optimization levers

Concrete, code-anchored levers to keep CogniTrade inside the budget as the library and usage
grow. Ordered roughly by impact-per-effort.

### 6.1 Replace brute-force scan with an index (the big one)

`LanceStore::search` is the only O(N) RAM/CPU term that grows without bound. Today it
`SELECT *`s the whole `doc_chunks` table and cosines in Rust every query. Options, cheapest
first:

- **Don't materialize text in the scan.** `search` currently pulls each row's `text` (the
  biggest per-row field) only to discard it for all but the top-`k`. Selecting `id, embedding`
  in the scan and fetching `text` for the surviving `k` rows in a second query would cut the
  transient working set by **~60–80%** at large N — a small, low-risk change to `lance.rs`, and
  the **preferred first step** on the 8 GB box because it adds **no** persistent RAM.
- **Add an ANN index** (e.g. an HNSW/IVF index, or migrate to a real on-disk vector store) so
  search is sub-linear. **Caveat for the 8 GB box:** a real in-process ANN index trades the
  transient per-query scan for a **persistent resident index whose RAM grows with N**. So on
  this hardware, prefer a **memory-mapped / on-disk** ANN over a fully-resident in-process
  index, and combine it with the "don't materialize text" change above. Keep it **local** where
  possible — an on-disk index preserves the "fits on the box" property; a hosted vector DB is
  the offload fallback (§4). This is the principled fix once a library exceeds tens of thousands
  of chunks. (The store is already *named* "Lance"/`vec.db` — adopting a true ANN backend would
  make the name honest.)

### 6.2 Cap / tune top-`k`

`ipc::analyze` retrieves a fixed `k = 6` (`retrieval::search(&store, &q, 6)`). `k` bounds the
retrieved-knowledge block size, which feeds the Anthropic request (and thus token **cost** as
well as RAM). Keep `k` small and consider making it a setting; do **not** raise it
speculatively. The cosine scan cost is independent of `k` until §6.1 lands, but the prompt size
(and cost cap pressure, §6.5) scales with it.

### 6.3 Batch embeds

`embeddings::embed(Vec<String>)` already accepts a batch and `ingest_pdf` passes **all** chunks
of a document in one call — good. Preserve this: embedding is single-flight behind the global
`Mutex`, so batching amortizes per-call ONNX overhead and shortens the CPU burst. Avoid
embedding chunk-by-chunk in any new code path. For very large folder drops, the watcher
serializes on the mutex anyway; if needed, throttle or off-hours-schedule bulk ingest to avoid
a long sustained APU burn (§2.3).

### 6.4 Prune cadence and retention

- **Chat:** the 7-day prune (`prune.rs`, every 6 h) keeps `chat.db` small and caps the chat
  history WebView2 holds. Keep it.
- **Screenshots:** add a retention/prune job (currently **Planned — not yet implemented**) so
  `screenshots\` stops growing monotonically (§3.2). Even a simple "delete PNGs older than N
  days" mirrors the chat prune.
- **Predictions/cost rows:** tiny, but a periodic archival of resolved predictions would keep
  `chat.db` query-fast over years of use.

### 6.5 Keep generation remote and capped

The biggest single lever against the **8 GB ceiling** is the one already taken: **no local
LLM**. Keep all generation on the Anthropic API. The `analyze_with_cap` path enforces a daily
USD cap (`daily_cost_cap_usd`, default **$2**) **before** each main call (`llm::client`), which
also indirectly bounds how much work the box does per day.

Model choice affects **cost** (and how fast the $2 cap is hit), but **not** the local RAM/CPU
footprint, since generation is remote either way:

- `model_extract` (`claude-haiku-4-5-20251001`) does the cheap work — vision symbol extraction
  and image description (Haiku 4.5: $1 / $0.10 / $5 per Mtok input/cached/output, `cost::rates_for`).
- `model_main` is **user-configurable** in settings (`settings.rs`): **`claude-sonnet-4-6`**
  (the default, Sonnet 4.6: $3 / $0.30 / $15) **or** the optional **`claude-opus-4-7`**
  (Opus 4.7: $15 / $1.50 / $75, the `opus` tier in `cost::rates_for`). Selecting Opus raises
  per-call cost ~5×, so the **$2 daily cap is reached far sooner** — but the local footprint is
  unchanged (the only local effect is marginally more SSE/token-handling work for longer
  outputs).

### 6.6 Stay tray-resident with the panel hidden

For 24/7 operation, keep the window **hidden** (it starts `"visible": false` and toggles via
tray / `Ctrl+Shift+Space`). A backgrounded WebView2 has its working set trimmed by the OS,
which is the difference between the ~250 MB idle and ~1 GB active rows in §5. This is a
zero-code operational lever with real RAM payoff.

### 6.7 Lazy initialization (already in place — preserve it)

The embedding model (`OnceLock`), the vector store (`Arc<Mutex<Option<LanceStore>>>`, opened by
`lance_get_or_open`), and the WebView panel are all **lazy**. A fresh install that only sits in
the tray pays almost nothing. Any future feature should follow the same pattern — **load on
first use, never at startup** — to protect the idle footprint on the 8 GB box. (Caveat from
§1.6: "first use" of the embedder is the first *analyze* **or** first ingest, whichever comes
sooner — analyze always embeds the query.)

---

## 7. Bottom line

For its **as-built, advisory** scope, CogniTrade fits the Ryzen 3400G / 8 GB / 450 W target
with substantial headroom (**~1 GB active peak, ~4.5 GB free** at personal-library scale, §5).
The budget is protected by three deliberate choices: **all LLM generation is remote**, the
**only resident model is a small, lazy, CPU-only embedder**, and the **single O(N) growth term**
(the brute-force cosine scan) stays cheap for a personal-scale library. The pressure points are
(1) the brute-force scan at very large libraries — fix with §6.1, preferring an on-disk index so
the fix does not itself become resident RAM, and (2) unbounded `screenshots\` growth — fix with
a prune (§6.4). Every spec feature that would break the budget — **heavy backtesting,
large/local models, continuous market-data streams, exchange execution** — is
**Planned — not yet implemented** and must be **offloaded**, never run locally on this hardware.
