# CogniTrade — Personal Crypto Analysis Assistant

**Status:** Design approved 2026-05-05
**Owner:** aryantuntune42@gmail.com
**Hardware target:** Ryzen 3400G, 8GB RAM, 1TB storage, Windows 11

## 1. Purpose

A personal desktop assistant that helps the user trade crypto manually. The user shares chart screenshots; the bot reads them, retrieves relevant passages from a private library of trading research (PDFs + reference chart images), and returns a structured analysis with a calibrated probability estimate, suggested action, and citations.

The user executes every trade themselves on their exchange of choice. **The bot never connects to any exchange and never places orders.**

## 2. Scope

### In v1

- System tray app on Windows: click icon → slide-out chat panel anchored to right edge of screen, always-on-top, ~420px wide.
- Drag-and-drop document library (PDFs + chart images) with local vector indexing.
- Chat with screenshot paste/upload. Each analysis returns prose + a structured JSON card (action, probability, horizon, stop-loss %, take-profit %, citations).
- Persistent chat history (SQLite) with a **rolling 7-day window** — older messages auto-pruned nightly.
- Outcome logging: every prediction is timestamped; a background job fetches CoinGecko price after the prediction's horizon and records actual movement for later calibration analysis.
- Anthropic API key stored encrypted in Windows Credential Manager (OS keychain).
- Soft daily API cost cap (default $2/day) with hard block at limit.
- English only.

### Out of v1

- Multi-language support
- Push/background price alerts (pull-only — bot only responds when user sends a message)
- Calibration scoring dashboard (data is logged in v1; dashboard is a v2 feature)
- Auto-tagging of documents
- Multi-account or multi-user support
- Any exchange integration or order placement
- Auto-trading, even with approval gates

## 3. Hard constraints

| Constraint | Rule |
|---|---|
| RAM ceiling | 8GB system total. App must idle < 250MB and stay < 1GB during active query. |
| No exchange access | Bot has no API keys for Binance/Kraken/etc. and no code paths that could place orders. |
| API key handling | Anthropic key lives in Windows Credential Manager only. Never written to disk in plaintext, never logged, never sent anywhere except the Anthropic API. |
| Cost cap | Hard cap is enforced at the LLM client layer — no request leaves the app once the daily total exceeds the configured limit. |
| Chat retention | Messages older than 7 days are deleted. No archival, no cloud sync. |

## 4. Architecture

### 4.1 Stack

- **Shell:** Tauri 2.x (Rust)
- **Frontend:** Svelte + Tailwind CSS (chosen over React for lower memory footprint)
- **Vector store:** LanceDB embedded (Rust-native)
- **Embeddings:** `fastembed-rs` running BGE-small ONNX locally
- **PDF parsing:** `pdf-extract` crate
- **Chat & metadata DB:** SQLite via `rusqlite`
- **HTTP client:** `reqwest` (async, rustls)
- **Keychain access:** `keyring` crate (uses Windows Credential Manager on Windows)
- **Tray + global hotkey:** Tauri 2.x built-in tray APIs + `tauri-plugin-global-shortcut`

### 4.2 Component diagram

```
┌─────────────────────────────────────────────────────┐
│                  Tauri App (Rust)                    │
│  ┌────────────────┐    ┌───────────────────────┐   │
│  │  Webview UI    │◄──►│   Rust Backend (IPC)   │   │
│  │  Svelte+Tailwind│    │                       │   │
│  │  - Chat panel  │    │  - Document ingester  │   │
│  │  - Library mgmt│    │  - Vector search      │   │
│  │  - Settings    │    │  - LLM client         │   │
│  └────────────────┘    │  - Chat store         │   │
│                         │  - Prediction logger  │   │
│                         │  - Cost tracker       │   │
│                         │  - Keychain access    │   │
│                         └──────────┬────────────┘   │
└─────────────────────────────────────┼───────────────┘
                                      │
              ┌───────────────────────┼───────────────┐
              ▼                       ▼               ▼
         LanceDB              SQLite (chat)    Claude API
         (vectors,            + 7-day          (Sonnet 4.6
         doc chunks)          prune job        default;
                                               Opus 4.7
                                               opt-in)
                                      ▲
                                      │
                              CoinGecko free API
                              (outcome scraping)
```

### 4.3 Components

| Component | Module | Responsibilities |
|---|---|---|
| Tray controller | `src-tauri/src/tray.rs` | Icon, panel show/hide, global hotkey (`Ctrl+Shift+Space` default), positioning to right edge of primary monitor |
| Document ingester | `src-tauri/src/ingest/` | Watches `~/CogniTrade/library/`, dispatches PDF → text chunks (≈500 tokens, 50-token overlap) and images → Claude vision description → text chunks; embeds with BGE-small; writes to LanceDB |
| Vector search | `src-tauri/src/retrieval.rs` | Top-k=6 cosine similarity over LanceDB; metadata filter by doc type when needed |
| LLM client | `src-tauri/src/llm/` | Single `analyze_chart()` entry point. Builds prompt: system + retrieved chunks (with prompt caching) + last-10 chat turns within 7d + screenshot + user message. Calls Anthropic Messages API. Streams tokens to UI. Parses trailing JSON block. |
| Chat store | `src-tauri/src/chat_store.rs` | SQLite. Schema in §5.1. Nightly prune of rows where `ts < now - 7d`. |
| Prediction logger | `src-tauri/src/predictions.rs` | On JSON parse, persist `{symbol, ts, horizon, predicted_direction, predicted_prob, sl, tp}`; scheduled job after `horizon` fetches CoinGecko price and writes outcome row |
| Cost tracker | `src-tauri/src/cost.rs` | Reads `usage` from each Anthropic response, sums per-day input + cached + output × per-token rates, persists to SQLite, exposes `is_over_cap()` |
| Settings | `src-tauri/src/settings.rs` | API key (keychain), daily cap, model selection, library path, hotkey |

## 5. Data design

### 5.1 SQLite schema

```sql
CREATE TABLE messages (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,           -- unix epoch seconds
  role TEXT NOT NULL,            -- 'user' | 'assistant' | 'system'
  content TEXT NOT NULL,
  image_path TEXT,               -- absolute path under ~/CogniTrade/screenshots/
  prediction_id INTEGER,         -- FK to predictions.id when applicable
  FOREIGN KEY (prediction_id) REFERENCES predictions(id)
);
CREATE INDEX idx_messages_ts ON messages(ts);

CREATE TABLE predictions (
  id INTEGER PRIMARY KEY,
  ts INTEGER NOT NULL,
  symbol TEXT NOT NULL,          -- 'BTCUSDT' etc.
  horizon_seconds INTEGER NOT NULL,
  predicted_prob_up REAL NOT NULL,  -- 0.0 to 1.0
  recommended_action TEXT NOT NULL, -- 'buy' | 'sell' | 'hold'
  stop_loss_pct REAL,
  take_profit_pct REAL,
  citations_json TEXT,           -- raw citation list
  outcome_status TEXT NOT NULL DEFAULT 'pending', -- 'pending' | 'resolved' | 'failed'
  outcome_price_at_ts REAL,
  outcome_price_at_horizon REAL,
  outcome_resolved_at INTEGER
);

CREATE TABLE cost_daily (
  date TEXT PRIMARY KEY,         -- 'YYYY-MM-DD'
  input_tokens INTEGER NOT NULL DEFAULT 0,
  cached_input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL NOT NULL DEFAULT 0
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

### 5.2 LanceDB schema

Single table `documents`:

| Column | Type | Notes |
|---|---|---|
| `id` | string | uuid |
| `doc_path` | string | original file under `~/CogniTrade/library/` |
| `doc_type` | string | `pdf` \| `image` |
| `chunk_index` | int | order within source doc |
| `text` | string | the chunk content (or vision-derived description for images) |
| `page` | int? | for PDFs only |
| `embedding` | vector(384) | BGE-small dimension |

### 5.3 Filesystem layout

```
~/CogniTrade/
├── library/                    # user drops PDFs and chart images here
├── screenshots/                # paste/upload destination, uuid-named
├── data/
│   ├── chat.db                 # SQLite
│   └── lancedb/                # vector store
└── logs/
    └── cognitrade.log          # rotating, 7-day max
```

## 6. Critical flows

### 6.1 Document ingestion

1. User drops file into `~/CogniTrade/library/` (drag-drop in UI or filesystem watcher).
2. Ingester computes SHA-256; if already indexed, skip.
3. **PDF path:** `pdf-extract` → text → chunk (≈500 tokens, 50 overlap) → embed batch.
4. **Image path:** load image → call Claude with prompt "Describe this chart for retrieval purposes: pattern, indicators, timeframe, conclusion. Be specific and factual." → chunk the description → embed.
5. Write rows to LanceDB.
6. UI shows progress bar; failure surfaces a toast with file name.

Image-ingestion calls Claude once per image and counts toward the daily cap.

### 6.2 Chart analysis ("I sent a screenshot")

1. User pastes/uploads screenshot → saved to `~/CogniTrade/screenshots/<uuid>.png`.
2. **If user didn't type a symbol/timeframe:** issue a tiny extraction call to Claude (Haiku 4.5, ~$0.0002 per call) returning `{symbol, timeframe}`.
3. Build retrieval query: `"{symbol} {timeframe} chart pattern analysis"`.
4. Vector search → top-6 chunks.
5. Build Anthropic Messages request:
   - System prompt: analyst role, output contract (§7), citation rules, refusal-to-hold on poor input.
   - Cached blocks: system prompt + retrieved chunks (`cache_control: ephemeral`).
   - Conversation: last 10 messages within 7-day window.
   - User turn: image + user's typed question.
6. Stream response to UI. After `[DONE]`, parse trailing fenced JSON block.
7. Persist message + prediction rows, update cost tracker.
8. Schedule outcome-fetch task at `now + horizon`.

### 6.3 Outcome resolution

1. Background tokio task wakes when a prediction's horizon elapses.
2. Fetch `simple/price` from CoinGecko free API for the symbol.
3. Compare to price at prediction time (also fetched at `t=0`); compute % move.
4. Update `predictions` row: `outcome_status='resolved'`, prices, resolved-at timestamp.
5. On API error: retry up to 3× with exponential backoff, then mark `outcome_status='failed'`.

### 6.4 7-day prune

Tokio interval task, runs every 6h:
```sql
DELETE FROM messages WHERE ts < strftime('%s','now') - 604800;
```
Predictions are kept indefinitely (small footprint, useful for calibration).

## 7. Output contract

Every assistant turn that analyzes a chart ends with a fenced JSON block:

````
```json
{
  "action": "buy" | "sell" | "hold",
  "probability_up": 0.65,
  "horizon": "4h",
  "stop_loss_pct": 2.5,
  "take_profit_pct": 5.0,
  "key_signals": ["bullish RSI divergence", "ascending triangle"],
  "citations": [
    {"doc": "wyckoff_book.pdf", "page": 42},
    {"doc": "btc_4h_double_bottom.png"}
  ]
}
```
````

The UI renders this as a card; the prose explanation appears above it. Failed parse → error toast, prose still shown, no prediction row written.

`probability_up` is constrained to `[0, 1]`. `horizon` is one of `1h`, `4h`, `1d`, `3d`, `1w`. Out-of-range values are rejected.

## 8. Error handling

| Condition | Behavior |
|---|---|
| No API key set | Chat input disabled. Banner: "Set your Anthropic API key in Settings." |
| Daily cap reached | Chat input disabled. Banner: "Daily $X cap reached. Resets at midnight local time." |
| Anthropic API 5xx | One retry with 2s backoff. Then surface error in chat as system message. |
| Anthropic API 4xx | No retry. Surface the error message verbatim. |
| Image > 5MB | Auto-resize to 1568px on long edge before sending. |
| JSON tail missing | Show prose response, no prediction row, log warning. |
| Empty document library | Analysis still runs; system prompt notes "no retrieved context — answering from general knowledge only." |
| Blurry/unreadable chart | Model is instructed to return `action: "hold"` and ask for a clearer image. |
| CoinGecko API down (outcome fetch) | Retry 3× with exponential backoff. Then mark `outcome_status='failed'`. |
| LanceDB corruption | App refuses to start, suggests removing `~/CogniTrade/data/lancedb/` to rebuild. Document files in `library/` are untouched. |

## 9. Performance budget

| Resource | Idle | Active query |
|---|---|---|
| RAM | < 250MB | < 900MB |
| CPU | < 1% | spike during embedding only |
| Disk | < 100MB code + library size | — |
| Network | none | 1 LLM request + outcome fetches |

The embedding model loads on first ingestion or query and stays resident during a session; unloaded after 10 minutes idle to free RAM.

## 10. Testing strategy

- **Unit tests** (Rust `cargo test`):
  - Chunker boundary behavior (no chunk > 500 tokens, overlap correct)
  - Trailing-JSON parser (valid, missing, malformed, embedded backticks)
  - Cost arithmetic (input/cached/output per the published Anthropic rates)
  - 7-day prune SQL on a fixture DB
  - Image resize threshold logic
- **Integration tests:** end-to-end flow with a mocked Claude client returning canned responses; asserts a prediction row is written and the UI receives a stream event.
- **Manual smoke test:** real Claude API on a fixed test screenshot + 1 dummy PDF, run before each release. Asserts the JSON block parses and `probability_up ∈ [0,1]`. Not in CI (would cost money).

## 11. Security

- API key in Windows Credential Manager via `keyring` crate. App reads on demand, never copies to disk.
- Keychain entry name: `cognitrade.anthropic_api_key`.
- All HTTP calls use rustls (no system OpenSSL dependency, no MITM via system cert tricks).
- Logs scrub the API key value if it ever appears (regex on log writer).
- App has no remote-update mechanism in v1 (user updates by reinstalling).

## 12. Open questions deferred to implementation

- v1 commits to **BGE-small-en-v1.5** (384d) for embeddings. The LanceDB schema in §5.2 is sized to match. Upgrading to BGE-base would require a schema migration and is deferred.
- v1 commits to **Claude Sonnet 4.6** for chart analysis and **Claude Haiku 4.5** for the symbol-extraction tiny call. User can opt into Opus 4.7 in Settings for high-stakes analysis.
- Tray icon iconography (will design during implementation).

## 13. Non-goals (explicit)

- Will NOT call any exchange API.
- Will NOT place orders, even with user approval.
- Will NOT push notifications, alerts, or sounds.
- Will NOT cloud-sync chats, documents, or predictions.
- Will NOT support more than one user profile in v1.

---

**Document version:** 1.0 (2026-05-05)
