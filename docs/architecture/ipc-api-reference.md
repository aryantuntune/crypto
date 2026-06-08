# IPC & Events API Reference

This is the authoritative reference for the **Rust &harr; Svelte boundary** in CogniTrade. It documents every Tauri command exposed by the Rust backend, the request/response payloads, the events the backend emits, the typed TypeScript wrappers the frontend uses, and how errors cross the boundary.

> **Scope check.** CogniTrade is an **advisory** crypto-chart analyst, not an autonomous trader. There is **no exchange integration, no order execution, and no automated stop-loss/take-profit enforcement**. The system prompt states verbatim: *"You are an advisor. The user places all trades themselves."* The `stop_loss_pct` / `take_profit_pct` fields described below are advisory numbers the model suggests and that get stored for prediction self-calibration — they are never sent to any exchange. Spec features such as Binance/Kraken integration and a Minimax engine are **Planned — not yet implemented**; they do not exist in the code and are not part of this API.

Related docs:

- Project overview & constraints: [`../../CLAUDE.md`](../../CLAUDE.md)
- Original spec (aspirational, diverges from code): [`../../crypto Dhriti.txt`](../../crypto%20Dhriti.txt)
- Design plans: [`../superpowers/`](../superpowers/)

---

## 1. Architecture at a glance

| Layer | Tech | Location |
| --- | --- | --- |
| Backend | Rust, Tauri 2 (tray-icon) | `src-tauri/src/` |
| Frontend | SvelteKit / Svelte 5, TypeScript, Tailwind 3, Vite 6 (static SPA adapter) | `src/` |
| Transport | Tauri `invoke` (command calls) + Tauri events (`emit`/`listen`) | — |

Commands are registered in [`src-tauri/src/lib.rs`](../../src-tauri/src/lib.rs) via `tauri::generate_handler!` and implemented in [`src-tauri/src/ipc.rs`](../../src-tauri/src/ipc.rs). The frontend never calls `invoke` directly outside of the typed wrappers in [`src/lib/ipc.ts`](../../src/lib/ipc.ts).

All command handlers return `crate::error::Result<T>` (alias for `std::result::Result<T, AppError>`). On the wire, success returns the serialized `T`; failure rejects the JS Promise with a **string** (see [§7 Error serialization](#7-error-serialization)).

---

## 2. Command registration

Registered in `run()` in [`src-tauri/src/lib.rs`](../../src-tauri/src/lib.rs):

```rust
.invoke_handler(tauri::generate_handler![
    ipc::list_recent_messages,
    ipc::get_settings,
    ipc::save_settings,
    ipc::get_api_key_set,
    ipc::set_api_key,
    ipc::clear_api_key,
    ipc::cost_today,
    ipc::save_screenshot,
    ipc::analyze,
    ipc::ingest_path,
])
```

Shared state is `AppState` (managed via `app.manage(state)`), injected into commands that need it as `State<'_, AppState>`:

```rust
pub struct AppState {
    pub db: Db,                                  // SQLite handle for data/chat.db
    pub lance: Arc<Mutex<Option<LanceStore>>>,   // vector store for data/vec.db, opened lazily
    pub paths: Paths,                            // %USERPROFILE%\CogniTrade\{library,screenshots,data,logs}
    pub llm: LlmClient,                          // Anthropic streaming client
}
```

> **Naming caveat.** `LanceStore` / `paths::lancedb()` are **misnomers**: the vector store is plain SQLite (`data/vec.db`) storing embeddings as little-endian `f32` BLOBs, with brute-force cosine search in Rust. It is **not** LanceDB. See [`src-tauri/src/lance.rs`](../../src-tauri/src/lance.rs).

---

## 3. Command reference

The argument **names** below are exactly what `invoke()` must send. Tauri deserializes the JS argument object directly into the command's Rust parameters, so the JS keys must match the Rust parameter identifiers (see [§6 camelCase mapping](#6-camelcase-argument-mapping) for the one nuance that applies).

| Command | Params (Rust) | Returns | Notes / Errors |
| --- | --- | --- | --- |
| `list_recent_messages` | `limit: i64` | `Vec<Message>` | Returns most-recent `limit` messages **in chronological (oldest→newest) order** (queried `ORDER BY ts DESC` then reversed). The frontend wrapper `api.listRecentMessages` defaults `limit` to **50**, and the chat store's `refreshMessages()` always requests `50` ([`src/lib/stores/chat.ts`](../../src/lib/stores/chat.ts)). `Sqlite`. |
| `get_settings` | — (`State`) | `Settings` | Loads non-secret settings from the `settings` table, filling defaults for any missing key. `Sqlite`. |
| `save_settings` | `value: Settings` | `()` | Upserts each setting key. `Sqlite`. |
| `get_api_key_set` | — | `bool` | `true` if an Anthropic key exists in the OS keyring. Does **not** return the key. `Keyring`. |
| `set_api_key` | `value: String` | `()` | Stores the key in the OS keyring (service `cognitrade`, user `anthropic_api_key`). `Invalid("empty api key")` on blank input; `Keyring`. |
| `clear_api_key` | — | `()` | Deletes the keyring entry; missing entry is treated as success. `Keyring`. |
| `cost_today` | — (`State`) | `DailyCost` | Today's accumulated token usage and USD cost (UTC date). Empty row (zeros) if none yet. `Sqlite`. |
| `save_screenshot` | `base64_png: String`, `filename_hint: Option<String>` | `String` (absolute path) | Decodes base64 PNG, writes to `…\CogniTrade\screenshots\{hint}-{uuid}.png`, returns the path. `Invalid("base64: …")`, `Io`. |
| `analyze` | `req: AnalyzeReq` | `AnalyzeAck` | The core flow. Streams via events; see [§4](#4-the-analyze-flow). `NoApiKey`, `CostCapReached`, `Anthropic`, `Sqlite`, `Io`, `Http`, `Serde`. |
| `ingest_path` | `path: String` | `usize` (chunk count) | Ingests a single PDF or image into the vector store; returns number of chunks written. `Invalid("unsupported extension: …")`, `NoApiKey` (images only), `Anthropic`, `Io`. |

### 3.1 Per-command details

#### `list_recent_messages(limit) -> Vec<Message>`
Thin wrapper over `chat_store::recent`. `Message` shape — see [§5.1](#51-message).

#### `get_settings() / save_settings(value)`
Operate on the `settings` key/value table only. **API keys are never stored here** — they live in the OS keyring. `Settings` shape — see [§5.2](#52-settings).

#### `get_api_key_set / set_api_key / clear_api_key`
Backed by [`src-tauri/src/settings.rs`](../../src-tauri/src/settings.rs) using the `keyring` crate:

- Service: `cognitrade`
- User/account: `anthropic_api_key`
- On Windows this maps to **Windows Credential Manager**.

`set_api_key` rejects empty/whitespace input with `AppError::Invalid`.

#### `cost_today() -> DailyCost`
Returns the row for the current UTC date from `cost_daily`, or a zeroed `DailyCost { date, .. }` if the day has no usage yet. `DailyCost` shape — see [§5.3](#53-dailycost).

#### `save_screenshot(base64_png, filename_hint?) -> String`
Used by the chat UI before calling `analyze`: it persists the captured chart PNG and returns the on-disk path, which is then passed as `screenshot_path` in the analyze request.

```rust
// filename pattern, with a UUID v4 to guarantee uniqueness
let stem = filename_hint.unwrap_or_else(|| id.clone());
let path = screenshots.join(format!("{}-{}.png", stem, id));
```

#### `ingest_path(path) -> usize`
Dispatches on file extension (case-insensitive):

| Extension | Path | API key required? |
| --- | --- | --- |
| `pdf` | `ingest_pdf` — `pdf-extract` → `chunk_text` (~500 words / 50 overlap) → embed → replace doc rows | No |
| `png` / `jpg` / `jpeg` / `webp` | `ingest_image` — Haiku (`model_extract`) describes the image → chunk/embed the description | **Yes** (`NoApiKey` if unset) |
| anything else | — | `Invalid("unsupported extension: …")` |

Returns the number of chunks written to the vector store. (The library file **watcher** auto-ingests dropped files independently; for watcher behaviour see [`src-tauri/src/ingest/watcher.rs`](../../src-tauri/src/ingest/watcher.rs) — it is not part of the IPC surface.)

---

## 4. The `analyze` flow

`analyze` is the single command that drives the product. It is implemented in [`src-tauri/src/ipc.rs`](../../src-tauri/src/ipc.rs) and orchestrates RAG retrieval, the Anthropic streaming call, prediction logging, and event emission.

### 4.1 Request / response shapes

```rust
#[derive(Deserialize)]
pub struct AnalyzeReq {
    pub user_text: String,
    pub screenshot_path: Option<String>,   // absolute path returned by save_screenshot
    pub symbol_hint: Option<String>,       // e.g. "BTCUSDT"; optional
}

#[derive(Serialize)]
pub struct AnalyzeAck {
    pub message_id: i64,                    // id of the persisted ASSISTANT message
}
```

> Unlike the other commands, `AnalyzeReq` field names use **snake_case** and are passed as a single `req` object. The wrapper in `ipc.ts` constructs that object with snake_case keys (see [§6](#6-camelcase-argument-mapping)).

`AnalyzeAck.message_id` is the row id of the **assistant** message saved at the end of the flow. The streamed text and parsed analysis arrive separately via events (below), so the Promise resolving with `AnalyzeAck` is **not** the primary delivery channel for content.

### 4.2 Step-by-step

1. **Persist the user message** (`chat_store::insert`, role `user`, with `image_path = screenshot_path`).
2. **Resize the screenshot in place** if it exceeds the size threshold (`image_util::needs_resize` / `resize_in_place`).
3. **Build chat history**: the 10 most recent messages (excluding the just-inserted user row) become `ChatMessage` history.
4. **Open the vector store** lazily (`lance_get_or_open`).
5. **Base64-encode the image** if a screenshot was provided.
6. **Resolve the symbol** (priority order):
   1. `symbol_hint` if non-empty → uppercased.
   2. Otherwise, if an image is present → a tiny non-streaming **Haiku** vision call (`extract_symbol_from_image`, using `settings.model_extract`) returns the ticker; on any failure it falls back to `"BTCUSDT"`.
   3. Otherwise → `"BTCUSDT"`.
7. **RAG retrieval**: embed the query `"{symbol} chart pattern analysis"` and brute-force cosine top-6 over `doc_chunks` (`retrieval::search(&store, &q, 6)`).
8. **Build the Anthropic request** (`PromptInputs` → `build_messages`): chat history, then a single new `user` turn containing, in order — a cached *"Retrieved knowledge"* text block (`cache_control: ephemeral`, only if chunks exist), the image block (if any), and the user's text. The **system prompt** is also sent with `cache_control: ephemeral`.
9. **Stream**: `analyze_with_cap` enforces the cost cap, sends the streaming request, and forwards text deltas through an `mpsc` channel. A spawned task re-emits each delta as the Tauri event **`analysis_chunk`**.
10. **Parse the JSON tail**: after the stream ends, `extract_json_tail` parses the **last** fenced ```` ```json ```` block in the full text into `AnalysisJson` (validated; `None` if absent/invalid).
11. **Log a prediction** (if analysis parsed): `predictions::insert_from_analysis` writes a `predictions` row (`outcome_status = 'pending'`), and a fire-and-forget task records the current CoinGecko price as the prediction's initial price.
12. **Persist the assistant message** (`chat_store::insert`, role `assistant`, `content = full_text`, linked `prediction_id`).
13. **Emit `analysis_done`** with `{ message_id, analysis }` and return `AnalyzeAck { message_id }`.

### 4.3 Cost cap enforcement

Before the main streaming call, `analyze_with_cap` ([`src-tauri/src/llm/client.rs`](../../src-tauri/src/llm/client.rs)) checks `cost::is_over_cap(today.cost_usd, settings.daily_cost_cap_usd)`. If today's spend is at or above the cap (default **$2/day**), it returns **`AppError::CostCapReached`** *before* any tokens are spent. The frontend surfaces this as the error string `"daily cost cap reached"`.

> The Haiku symbol-extraction call in step 6 runs **before** the cap check and is **not** cost-capped; its token usage is **never** recorded to `cost_daily` — `extract_symbol_from_image` makes its own `reqwest` call and never invokes `cost::add_usage`. Only the main streaming call's tokens are accounted. Treat this as current behaviour, not a guarantee.

---

## 5. Shared payload shapes

These are the structs that cross the boundary, with their TypeScript mirrors in [`src/lib/types.ts`](../../src/lib/types.ts).

### 5.1 `Message`

Rust ([`chat_store.rs`](../../src-tauri/src/chat_store.rs)) / TS ([`types.ts`](../../src/lib/types.ts)):

| Field | Rust type | TS type | Notes |
| --- | --- | --- | --- |
| `id` | `i64` | `number` | Row id. |
| `ts` | `i64` | `number` | Unix seconds (UTC). |
| `role` | `String` | `'user' \| 'assistant' \| 'system'` | DB CHECK constrains to these three. |
| `content` | `String` | `string` | Plain text (assistant content includes the JSON tail). |
| `image_path` | `Option<String>` | `string \| null` | Absolute path for user screenshots. |
| `prediction_id` | `Option<i64>` | `number \| null` | Set on assistant messages that produced an analysis. |

### 5.2 `Settings`

Rust ([`settings.rs`](../../src-tauri/src/settings.rs)) / TS ([`types.ts`](../../src/lib/types.ts)):

| Field | Rust type | TS type | Default |
| --- | --- | --- | --- |
| `daily_cost_cap_usd` | `f64` | `number` | `2.0` |
| `model_main` | `String` | `string` | `"claude-sonnet-4-6"` (analysis model). `"claude-opus-4-7"` is also supported — `cost::rates_for` matches the `opus`/`haiku` substrings, else applies Sonnet rates. |
| `model_extract` | `String` | `string` | `"claude-haiku-4-5-20251001"` (vision symbol extraction + image description) |
| `library_path` | `Option<String>` | `string \| null` | `null` (defaults to `…\CogniTrade\library`) |
| `hotkey` | `String` | `string` | `"Ctrl+Shift+Space"` |

> The API key is **not** a settings field. It lives only in the OS keyring and is toggled via `set_api_key` / `clear_api_key` / `get_api_key_set`.

### 5.3 `DailyCost`

Rust ([`cost.rs`](../../src-tauri/src/cost.rs)) / TS ([`types.ts`](../../src/lib/types.ts)):

| Field | Rust type | TS type |
| --- | --- | --- |
| `date` | `String` | `string` (UTC `YYYY-MM-DD`) |
| `input_tokens` | `u64` | `number` |
| `cached_input_tokens` | `u64` | `number` |
| `output_tokens` | `u64` | `number` |
| `cost_usd` | `f64` | `number` |

Rate table (`cost::rates_for`, per Mtok input / cached / output): Sonnet 4.6 `$3 / $0.30 / $15`; Opus 4.7 `$15 / $1.50 / $75`; Haiku 4.5 `$1 / $0.10 / $5`. Model selection matches substrings `opus` / `haiku`, else Sonnet rates.

### 5.4 `AnalysisJson` schema

The model is instructed (system prompt, [`src-tauri/src/llm/prompt.rs`](../../src-tauri/src/llm/prompt.rs)) to end its reply with a fenced ```` ```json ```` block matching this schema. It is parsed by `extract_json_tail` into `AnalysisJson` ([`src-tauri/src/llm/types.rs`](../../src-tauri/src/llm/types.rs)). This is a **JSON tail of free text — not tool-use.**

| Field | Rust type | Wire value | Required? | Validation |
| --- | --- | --- | --- | --- |
| `action` | `Action` (enum) | `"buy" \| "sell" \| "hold"` (lowercase) | Yes | enum |
| `probability_up` | `f32` | number | Yes | must be in `[0.0, 1.0]`; out-of-range fails the whole parse |
| `horizon` | `Horizon` (enum) | `"1h" \| "4h" \| "1d" \| "3d" \| "1w"` | Yes | enum |
| `stop_loss_pct` | `Option<f32>` | number | No (omitted if `None`) | advisory only |
| `take_profit_pct` | `Option<f32>` | number | No (omitted if `None`) | advisory only |
| `key_signals` | `Vec<String>` | `string[]` | No (defaults to `[]`) | — |
| `citations` | `Vec<Citation>` | `[{ "doc": string, "page"?: number }]` | No (defaults to `[]`) | `page` omitted when `None` |

Example tail the model emits:

```json
{
  "action": "buy",
  "probability_up": 0.65,
  "horizon": "4h",
  "stop_loss_pct": 2.5,
  "take_profit_pct": 5.0,
  "key_signals": ["bullish RSI divergence"],
  "citations": [{ "doc": "wyckoff_book.pdf", "page": 42 }]
}
```

Parsing rules (`extract_json_tail`):

- Uses the **last** ```` ```json ```` fence in the response.
- If the block is missing, not valid JSON, or `probability_up` is out of `[0,1]`, the analysis is `None` (no prediction row is written, and `analysis_done` carries `null`).

`stop_loss_pct` / `take_profit_pct` and `horizon` are persisted on the `predictions` row and used by the self-calibration loop, which resolves predictions at `ts + horizon_seconds` by comparing CoinGecko price-at-horizon vs. price-at-prediction. **It does not simulate intra-horizon SL/TP hits — there is no SL/TP enforcement.**

If the CoinGecko price fetch fails at resolution time (e.g. an unmapped symbol — only ~12 coins are supported in `coingecko::symbol_to_id`), `predictions::resolve_one` sets `outcome_status = 'failed'` instead of `'resolved'`. The `BTCUSDT` fallback is always resolvable, but an arbitrary user-hinted or vision-extracted ticker may be unlisted.

---

## 6. camelCase argument mapping

Tauri's `invoke(cmd, args)` serializes the `args` object and deserializes it directly into the command's Rust parameters. **The JS keys must equal the Rust parameter identifiers.** Today every Rust command parameter is already `snake_case` or single-word, so the wrappers pass matching keys verbatim — there is no automatic camelCase→snake_case conversion in this codebase.

The frontend wrappers in [`src/lib/ipc.ts`](../../src/lib/ipc.ts):

```ts
import { invoke } from '@tauri-apps/api/core';
import type { Message, Settings, DailyCost, AnalysisJson } from './types';

export const api = {
  listRecentMessages: (limit = 50) => invoke<Message[]>('list_recent_messages', { limit }),
  getSettings:        () => invoke<Settings>('get_settings'),
  saveSettings:       (value: Settings) => invoke<void>('save_settings', { value }),
  getApiKeySet:       () => invoke<boolean>('get_api_key_set'),
  setApiKey:          (value: string) => invoke<void>('set_api_key', { value }),
  clearApiKey:        () => invoke<void>('clear_api_key'),
  costToday:          () => invoke<DailyCost>('cost_today'),
  saveScreenshot:     (base64Png: string, filenameHint?: string) =>
                        invoke<string>('save_screenshot', { base64Png, filenameHint }),
  analyze:            (req: { user_text: string; screenshot_path?: string; symbol_hint?: string }) =>
                        invoke<{ message_id: number }>('analyze', { req }),
  ingestPath:         (path: string) => invoke<number>('ingest_path', { path }),
};
```

Key points:

- **`save_screenshot` is the one place camelCase matters.** Its Rust params are `base64_png` and `filename_hint`, but the wrapper sends `{ base64Png, filenameHint }`. This works because **Tauri converts incoming `camelCase` argument keys to `snake_case` when binding to command parameters** — so `base64Png → base64_png` and `filenameHint → filename_hint`. The TS-level wrapper takes camelCase ergonomic params, but the on-the-wire keys it sends are camelCase that Tauri normalizes.
- **`analyze` passes a single nested object** under the key `req` (matching the Rust param `req: AnalyzeReq`). The *inner* fields use **snake_case** (`user_text`, `screenshot_path`, `symbol_hint`) because `AnalyzeReq` derives `Deserialize` with default (snake_case) field names — they are not transformed. The chat store builds this object:

```ts
// src/lib/stores/chat.ts
await api.analyze({
  user_text: opts.text,
  screenshot_path: opts.screenshotPath,
  symbol_hint: opts.symbolHint,
});
```

- All other wrappers use keys identical to their Rust parameter names (`limit`, `value`, `path`), so no transformation is involved.

> Rule of thumb: prefer wrapper params in camelCase for ergonomics, but be explicit about the **on-the-wire keys** — Tauri normalizes top-level camelCase argument keys to snake_case, but it does **not** rewrite keys *inside* a nested object like `req`.

---

## 7. Emitted Tauri events

The backend pushes streaming output and completion through Tauri events (`AppHandle::emit`). The frontend subscribes in [`src/lib/stores/chat.ts`](../../src/lib/stores/chat.ts) via `listen(...)`.

| Event | Payload (wire) | Emitted by | Frequency |
| --- | --- | --- | --- |
| `analysis_chunk` | `string` — a raw text delta | `analyze` (per streamed text delta) | many per request |
| `analysis_done` | `{ message_id: number, analysis: AnalysisJson \| null }` | `analyze` (once, at the end) | once per request |

### 7.1 `analysis_chunk`

Each payload is a **string delta** (the text fragment from one Anthropic `content_block_delta` / `text_delta`). The frontend appends them into a `streaming` store:

```ts
unlistenChunk = await listen<string>('analysis_chunk', (e) => {
  streaming.update((s) => s + e.payload);
});
```

Deltas are emitted in order. The concatenation of all `analysis_chunk` payloads equals the assistant message `content` that is later persisted — **including** the trailing ```` ```json ```` block.

### 7.2 `analysis_done`

Emitted exactly once after the stream completes, the prediction (if any) is logged, and the assistant message is saved:

```rust
let _ = app.emit("analysis_done", serde_json::json!({
    "message_id": asst_id,
    "analysis": out.analysis,   // AnalysisJson or null
}));
```

Frontend handler resets streaming state and reloads the message list:

```ts
unlistenDone = await listen<{ message_id: number; analysis: AnalysisJson | null }>(
  'analysis_done',
  async (e) => {
    analysisResult.set(e.payload.analysis);  // may be null if no valid JSON tail
    streaming.set('');
    isAnalyzing.set(false);
    await refreshMessages();
  }
);
```

`analysis` is `null` when the model produced no parseable/valid JSON tail. Consumers must handle the `null` case.

> Listeners are registered once in `initEvents()` (guarded by a `unlistenChunk` null check). There are currently no per-request correlation ids on `analysis_chunk`; the UI assumes a single in-flight analysis at a time (`isAnalyzing`).

---

## 8. Error serialization (`AppError` → frontend)

All commands return `Result<T, AppError>`. `AppError` ([`src-tauri/src/error.rs`](../../src-tauri/src/error.rs)) has a **custom `Serialize` impl that serializes to its `Display` string**:

```rust
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_string())
    }
}
```

So on the JS side, a failed command **rejects the Promise with a plain string** (not an object, not a typed code). The frontend treats it as such:

```ts
try {
  await api.analyze({ /* ... */ });
} catch (e) {
  isAnalyzing.set(false);
  streaming.set(`\n\n[error: ${String(e)}]`);
}
```

### 8.1 Variants and their wire strings

| Variant | `Display` string (what JS receives) | Typical cause |
| --- | --- | --- |
| `Io(std::io::Error)` | `io error: <msg>` | file read/write (screenshot, image) |
| `Sqlite(rusqlite::Error)` | `sqlite error: <msg>` | DB access |
| `Http(reqwest::Error)` | `http error: <msg>` | network failure to Anthropic / CoinGecko |
| `Serde(serde_json::Error)` | `serde error: <msg>` | JSON (de)serialization |
| `Keyring(keyring::Error)` | `keyring error: <msg>` | OS credential store access |
| `Anthropic(String)` | `anthropic api error: <msg>` | non-2xx from Anthropic (includes HTTP status + body) |
| `CostCapReached` | `daily cost cap reached` | today's spend ≥ `daily_cost_cap_usd` |
| `NoApiKey` | `api key not set` | no key in keyring (analyze / image ingest) |
| `NotFound(String)` | `not found: <msg>` | — |
| `Invalid(String)` | `invalid input: <msg>` | empty API key, bad base64, unsupported file extension, symbol extraction failure |
| `Internal(String)` | `internal: <msg>` | catch-all |

> Because errors arrive as opaque strings, the frontend cannot reliably branch on error *type* without string matching. If you need typed/structured error handling (e.g. distinguishing `CostCapReached` to prompt the user to raise the cap), that is **not yet implemented** — it would require changing `AppError`'s `Serialize` impl to emit a structured object (e.g. `{ kind, message }`) and updating `types.ts` accordingly.

---

## 9. Capabilities & permissions

The Tauri capability set is minimal ([`src-tauri/capabilities/default.json`](../../src-tauri/capabilities/default.json)):

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

The `global-shortcut`, `dialog`, and `fs` plugins are `init()`'d in `lib.rs`, but they are **not** granted any capability permissions in this set — only `core:default` and `opener:default` are. Consequently the **frontend cannot invoke any of them directly**: all custom backend interaction goes through the IPC commands documented above. In particular, the `Ctrl+Shift+Space` panel-toggle hotkey is registered and handled **entirely in Rust** (`run()` in [`src-tauri/src/lib.rs`](../../src-tauri/src/lib.rs)), driven by the `global-shortcut` plugin from the backend — not via any frontend permission.

---

## 10. Quick reference: typical call sequences

**Send a chart for analysis** (chat tab):

```text
1. capture PNG (frontend)            → base64
2. api.saveScreenshot(b64, hint)     → invoke 'save_screenshot' → absolute path
3. api.analyze({ user_text,          → invoke 'analyze' → resolves with { message_id }
      screenshot_path, symbol_hint })
   ── meanwhile ──
   listen 'analysis_chunk' (string)  → append to `streaming` store
   listen 'analysis_done' ({id,      → set `analysisResult`, clear streaming,
      analysis})                        refresh messages
```

**Ingest a document** (library tab):

```text
api.ingestPath(path) → invoke 'ingest_path' → resolves with chunk count (number)
```

**Settings / key management** (settings tab):

```text
api.getSettings()  / api.saveSettings(value)
api.getApiKeySet() / api.setApiKey(value) / api.clearApiKey()
api.costToday()    → DailyCost (drives the cost meter)
```
