# Frontend Architecture

This document describes the **SvelteKit/TypeScript frontend** of CogniTrade — the desktop UI shell that runs inside the Tauri 2 WebView. It covers the static-SPA build setup, the three-tab application shell, the Svelte components, the reactive stores, the typed IPC wrapper, and the screenshot-paste → analyze flow.

CogniTrade is an **advisory crypto-chart analyst**, not an autonomous trader. The frontend never places orders, never enforces stop-loss/take-profit, and never talks to an exchange. Its entire job is: collect a question + optional chart screenshot from the user, hand it to the Rust backend over one IPC command, stream the model's answer back into a chat transcript, and render a structured recommendation card. Every trade is placed by the user, by hand, outside this app.

> **Scope note.** This document describes only what the code in `src/` actually does as of the current tree. Features named in `crypto Dhriti.txt` and the design docs under [`../superpowers/`](../superpowers/) (autonomous trading, exchange order execution, Minimax engine, automated SL/TP enforcement, live indicator widgets) are **Planned — not yet implemented** and are explicitly called out as such where relevant. They have no corresponding frontend code.

Related docs:
- Design spec: [`../superpowers/specs/2026-05-05-cognitrade-design.md`](../superpowers/specs/2026-05-05-cognitrade-design.md)
- Implementation plan (Phase 1–5): [`../superpowers/plans/2026-05-05-cognitrade-v1.md`](../superpowers/plans/2026-05-05-cognitrade-v1.md)
- Backend IPC commands live in `src-tauri/src/ipc.rs` (the other half of every `invoke` call below).

---

## 1. Technology stack and build setup

| Concern | Choice | Where configured |
| --- | --- | --- |
| Framework | SvelteKit 2 (`@sveltejs/kit ^2.9.0`) | `package.json` |
| Component model | Svelte 5 (`svelte ^5.0.0`) | `package.json` |
| Adapter | `@sveltejs/adapter-static ^3.0.6`, SPA fallback to `index.html` | `svelte.config.js` |
| Bundler / dev server | Vite 6 (`vite ^6.0.3`), fixed port `1420` | `vite.config.js` |
| Styling | Tailwind CSS 3 (`tailwindcss ^3.4.19`) + PostCSS + Autoprefixer | `app.css`, PostCSS/Tailwind config |
| Language | TypeScript `~5.6.2`, `strict: true`, `moduleResolution: "bundler"` | `tsconfig.json` |
| Tauri bridge | `@tauri-apps/api ^2`; frontend imports only `@tauri-apps/plugin-dialog` | `package.json` |
| Type-check | `svelte-check ^4.0.0` | `npm run check` |

> **Plugin usage note.** `package.json` declares four Tauri plugins — `dialog`, `fs`, `global-shortcut`, `opener` — but only `@tauri-apps/plugin-dialog` is actually imported by frontend code (the native file picker in `LibraryView.svelte`). The other three are driven from the **Rust side** (`src-tauri/src/lib.rs`): the `Ctrl+Shift+Space` global hotkey and tray are wired in `lib.rs`, and `fs`/`opener` functionality is used by the backend, not the WebView.

### Commands

These are the frontend-relevant scripts (full toolchain in [`../../CLAUDE.md`](../../CLAUDE.md)). Windows is the primary platform.

```powershell
npm install            # install JS deps once after clone
npm run tauri dev      # run the desktop app (dev) with hot reload
npm run tauri build    # produce a release MSI/NSIS installer (Windows)
npm run check          # svelte-kit sync + svelte-check (type-check the frontend)
```

`npm run dev` / `npm run build` / `npm run preview` run Vite directly without the Tauri shell, and are mostly useful for isolated UI iteration; the real app is always launched through the `tauri` wrapper so the Rust backend and IPC commands exist.

### Why a static SPA for a desktop shell

The frontend is built as a **single-page application with no server-side rendering**. Two configuration points enforce this:

- `svelte.config.js` uses `adapter-static` with `fallback: "index.html"`, which emits a pure-client bundle and routes every unknown path back to the SPA entry.
- `src/routes/+layout.ts` sets `export const ssr = false;`, disabling SSR for the whole route tree.

The reasoning (captured verbatim in the comments of both files): *Tauri doesn't have a Node.js server to do proper SSR, so we use `adapter-static` with a fallback to `index.html` to put the site in SPA mode.* The Tauri WebView loads pre-built static HTML/JS/CSS from disk; there is no Node runtime at runtime, so SSR is impossible and unnecessary. Vite serves the same bundle over `http://localhost:1420` during `tauri dev` (strict port, so the build fails fast if the port is taken) and Tauri points the WebView at it.

This also fits the **8 GB-RAM hardware ceiling** (Ryzen 3400G target): no Node server process, no SSR render workers — just static assets in an embedded WebView2 browser. The heavy lifting (embeddings, LLM streaming, SQLite) all lives in the Rust process, not the browser.

### Global styling

`src/app.css` pulls in Tailwind's three layers and sets a dark base theme:

```css
@tailwind base;
@tailwind components;
@tailwind utilities;

html, body, #app { height: 100%; margin: 0; }
body { background: #0b0d10; color: #e6e8eb; font-family: ui-sans-serif, system-ui, sans-serif; }
```

`src/routes/+layout.svelte` imports `app.css` once and renders `<slot />`, so every page inherits the dark, full-height shell. Component-level styling is done inline with Tailwind utility classes (e.g. `bg-zinc-900`, `text-xs`, `rounded-lg`); there are no `.svelte` `<style>` blocks.

---

## 2. Application shell — the three tabs

The entire UI is one route, `src/routes/+page.svelte`. It is a flat tab switcher — there is no client-side router beyond this single page.

```svelte
<script lang="ts">
  import ChatPanel from '../lib/components/ChatPanel.svelte';
  import LibraryView from '../lib/components/LibraryView.svelte';
  import SettingsView from '../lib/components/SettingsView.svelte';
  type Tab = 'chat' | 'library' | 'settings';
  let tab: Tab = 'chat';
</script>
```

The layout is a flex column filling the viewport (`h-screen`):

- A **header** (`CogniTrade` brand + a `<nav>` of three buttons). The active tab is indicated with a Tailwind `class:underline={tab === '...'}` directive.
- A **main** region (`flex-1 overflow-hidden`) that mounts exactly one of three components based on `tab`:

| Tab | Component | Default |
| --- | --- | --- |
| Chat | `ChatPanel` | ✅ (initial `tab = 'chat'`) |
| Library | `LibraryView` | |
| Settings | `SettingsView` | |

Tabs are mounted with `{#if tab === '...'}`, so switching tabs **destroys and recreates** the previous view. The Chat transcript survives this because messages live in a module-level store (see §4), not in component state — `ChatPanel.onMount` re-reads the store and re-subscribes to events each time it mounts.

---

## 3. Components and their roles

All components live in `src/lib/components/`. Roles below; the data plumbing they rely on is described in §4–§5.

### `ChatPanel.svelte`
The Chat tab's root. Responsibilities:
- On mount: `refreshMessages()` (load the last 50 messages) and `initEvents()` (subscribe to backend stream events — idempotent).
- Renders the transcript by iterating `$messages` into `MessageBubble`s (keyed by `m.id`).
- While a request is in flight, renders the live `$streaming` text in a left-aligned bubble.
- When a structured result arrives, renders a `PredictionCard` from `$analysisResult`.
- Hosts the composer: a **symbol hint** input (placeholder `symbol (e.g. BTCUSDT)`), a **prompt textarea**, and an **Analyze** button that calls `send()`. The button is disabled while `$isAnalyzing` and shows `Analyzing…`.
- Embeds `<ScreenshotPaste>` and listens to its `saved` event to capture a `pendingScreenshot` path; shows a `📎 screenshot ready: <filename>` hint when one is staged. The filename is derived with `pendingScreenshot.split('\\').pop()` — a **Windows-path-only** split. Since Windows is the primary platform this is correct for saved screenshot paths (`%USERPROFILE%\CogniTrade\screenshots\…`), but on POSIX it would show the full path. (By contrast `LibraryView` splits on `[\\/]`, handling both separators.)
- `send()` snapshots `{text, screenshotPath, symbolHint}`, clears the inputs immediately, then calls `sendAnalyze(...)`.

### `MessageBubble.svelte`
Renders one persisted `Message`. User messages are right-aligned/blue; assistant (and system) messages are left-aligned/zinc. If the message has an `image_path`, it renders the screenshot inline via `<img src={'file://' + m.image_path}>`. Content is shown in a `<pre class="whitespace-pre-wrap font-sans">` so model whitespace/newlines survive.

### `PredictionCard.svelte`
Renders the structured `AnalysisJson` recommendation that the backend parsed from the model's trailing JSON block. Shows:
- An **action** badge — `buy` (green) / `sell` (red) / `hold` (zinc).
- **P(up)** as a percentage (`Math.round(probability_up * 100)`) and the `horizon`.
- **SL / TP** percentages when present (`stop_loss_pct` / `take_profit_pct`), each falling back to `—`.
- A bullet list of `key_signals`.
- A `Cited:` line built from `citations`, formatted as `doc#page` (or just `doc` when no page).

> This card is **advisory display only.** The SL/TP numbers are the model's suggestion shown to the user; the frontend does not place orders or enforce these levels. Order execution and SL/TP enforcement are **Planned — not yet implemented**.

### `ScreenshotPaste.svelte`
A headless component (renders only `<svelte:window on:paste={onPaste} />`). It captures clipboard paste events anywhere in the window, finds the first `image/*` clipboard item, base64-encodes the bytes in the browser, and calls `api.saveScreenshot(b64, 'paste')`. The backend returns a saved file path, which the component re-emits as a `saved` event for `ChatPanel` to stage. See §6 for the full path.

### `LibraryView.svelte`
The Library tab. Lets the user add RAG documents (PDFs and chart images):
- **Add documents** button opens the native Tauri file picker (`@tauri-apps/plugin-dialog`'s `open`) filtered to `pdf, png, jpg, jpeg, webp`, then calls `ingest(path)` per file.
- Notes that files dropped into `~/CogniTrade/library` are auto-ingested by the backend watcher (no UI action needed).
- Renders an ingest status list from the `ingestStatus` store: `✅ <file> (<n> chunks)` on success, `❌ <file> — <err>` on failure.

### `SettingsView.svelte`
The Settings tab. On mount, `refreshAll()` loads settings, API-key-set flag, and today's cost. It exposes:
- **API key** — masked input + `Save key`, or a `key is set` indicator + `Clear` when one exists. The key is stored in the OS keyring by the backend, never in the DB or in the frontend.
- **Daily cap (USD)** — numeric input (default `2`); also shows `today: $<cost>` from the cost store.
- **Main model** — a `<select>` between `claude-sonnet-4-6` (default) and `claude-opus-4-7`.
- A **Save** button that persists the merged `Settings` object.

---

## 4. The chat store — `lib/stores/chat.ts`

This module is the heart of the frontend's state flow. It holds four Svelte `writable` stores at module scope (so they outlive tab switches) and three async helpers.

### Stores

| Store | Type | Meaning |
| --- | --- | --- |
| `messages` | `Writable<Message[]>` | The persisted transcript (loaded from the DB). |
| `streaming` | `Writable<string>` | Assistant text being accumulated live during a stream. |
| `analysisResult` | `Writable<AnalysisJson \| null>` | The structured recommendation from the last completed analysis. |
| `isAnalyzing` | `Writable<boolean>` | Whether a request is in flight (drives the disabled/`Analyzing…` button). |

### `refreshMessages()`
`messages.set(await api.listRecentMessages(50))` — replaces the transcript with the most recent 50 persisted messages from `chat.db`.

### `initEvents()` — listening to backend stream events
The backend pushes two **Tauri events** during `analyze`; this function subscribes to both via `@tauri-apps/api/event`'s `listen`. It is **guarded against double-subscription** (`if (unlistenChunk) return;`), so `ChatPanel` can call it on every mount safely.

| Event | Payload | Handler |
| --- | --- | --- |
| `analysis_chunk` | `string` (a text delta) | `streaming.update((s) => s + e.payload)` — appends the delta. |
| `analysis_done` | `{ message_id: number; analysis: AnalysisJson \| null }` | sets `analysisResult`, clears `streaming`, sets `isAnalyzing=false`, then `await refreshMessages()`. |

This is the **streaming text accumulation** loop: the backend forwards each Anthropic SSE text delta as one `analysis_chunk`, and the store concatenates them into `streaming`, which `ChatPanel` renders live. When the stream finishes, the backend has already persisted the final assistant message, so `analysis_done` simply swaps the live `streaming` bubble for a freshly reloaded transcript (`refreshMessages`) and surfaces the parsed `analysis` as a `PredictionCard`. The transient `streaming` string is discarded — the durable copy is the persisted message.

### `sendAnalyze({ text, screenshotPath?, symbolHint? })` — the send flow
```ts
export async function sendAnalyze(opts) {
  isAnalyzing.set(true);
  streaming.set('');
  analysisResult.set(null);
  try {
    await api.analyze({
      user_text: opts.text,
      screenshot_path: opts.screenshotPath,
      symbol_hint: opts.symbolHint,
    });
  } catch (e) {
    isAnalyzing.set(false);
    streaming.set(`\n\n[error: ${String(e)}]`);
  } finally {
    if (get(isAnalyzing)) { isAnalyzing.set(false); await refreshMessages(); }
  }
}
```
Step by step:
1. Flip `isAnalyzing` on and reset `streaming` / `analysisResult` so the UI starts clean.
2. `await api.analyze(...)` — fires the single backend `analyze` command. While it runs, `analysis_chunk` events stream in and fill `streaming` via `initEvents`.
3. On error (e.g. `CostCapReached`, `NoApiKey`, network), clear `isAnalyzing` and surface the error inline by writing it into `streaming`. The `AppError` is serialized by the backend and arrives as the thrown value.
4. The `finally` block is a **safety net**: normally `analysis_done` has already set `isAnalyzing=false`, but if the call failed *before* any event fired, this forces the flag off and refreshes the transcript so the UI never wedges in the analyzing state.

Note the timing: the backend emits `analysis_done` **before** the `analyze` command returns `Ok` (in `ipc.rs` the emit deterministically precedes the return). So by the time `api.analyze(...)` resolves, the listener's `refreshMessages()` is already in flight. Both refresh paths (the listener's and the `finally` safety net) are async, so their *completion* order isn't guaranteed — but they're idempotent (`messages.set` of the same recent rows), so a double refresh is harmless.

---

## 5. IPC wrapper and shared types

### `lib/ipc.ts` — typed `invoke` wrappers
A single `api` object wraps every backend `#[tauri::command]` behind `invoke<T>(...)` with explicit return types. This is the **only** place the frontend touches `@tauri-apps/api/core`'s `invoke`; everything else calls `api.*`.

| `api` method | Tauri command | Args → Returns |
| --- | --- | --- |
| `listRecentMessages(limit = 50)` | `list_recent_messages` | `{ limit }` → `Message[]` |
| `getSettings()` | `get_settings` | — → `Settings` |
| `saveSettings(value)` | `save_settings` | `{ value }` → `void` |
| `getApiKeySet()` | `get_api_key_set` | — → `boolean` |
| `setApiKey(value)` | `set_api_key` | `{ value }` → `void` |
| `clearApiKey()` | `clear_api_key` | — → `void` |
| `costToday()` | `cost_today` | — → `DailyCost` |
| `saveScreenshot(base64Png, filenameHint?)` | `save_screenshot` | `{ base64Png, filenameHint }` → `string` (file path) |
| `analyze(req)` | `analyze` | `{ req: { user_text, screenshot_path?, symbol_hint? } }` → `{ message_id }` |
| `ingestPath(path)` | `ingest_path` | `{ path }` → `number` (chunk count) |

Two argument-shape details worth noting because they must match the Rust signatures in `src-tauri/src/ipc.rs`:
- `analyze` wraps its payload in a `req` key: `invoke('analyze', { req })`, matching `pub async fn analyze(..., req: AnalyzeReq)`.
- `saveScreenshot` passes `base64Png` / `filenameHint` (camelCase); Tauri maps these to the snake_case Rust params `base64_png` / `filename_hint`.

Errors thrown by any of these are the serialized `AppError` enum from the backend (`Anthropic`, `Invalid`, `Keyring`, `NoApiKey`, `CostCapReached`, `Internal`); the frontend surfaces them as `String(e)`.

### `lib/types.ts` — shared types
The TypeScript mirror of the backend's serialized structs. These types must stay in sync with the Rust `serde` shapes.

```ts
export type Role = 'user' | 'assistant' | 'system';
export interface Message {
  id: number; ts: number; role: Role; content: string;
  image_path?: string | null; prediction_id?: number | null;
}

export type Action = 'buy' | 'sell' | 'hold';
export type Horizon = '1h' | '4h' | '1d' | '3d' | '1w';
export interface Citation { doc: string; page?: number; }

export interface AnalysisJson {
  action: Action;
  probability_up: number;       // 0..1; rendered as a %
  horizon: Horizon;
  stop_loss_pct?: number;       // advisory display only
  take_profit_pct?: number;     // advisory display only
  key_signals: string[];
  citations: Citation[];
}

export interface DailyCost {
  date: string; input_tokens: number; cached_input_tokens: number;
  output_tokens: number; cost_usd: number;
}

export interface Settings {
  daily_cost_cap_usd: number; model_main: string; model_extract: string;
  library_path: string | null; hotkey: string;
}
```

`AnalysisJson` is exactly the structured tail the backend parses from the model's response (a trailing fenced ```json block, not tool-use) and feeds into `PredictionCard`. The `Horizon` union matches the backend's self-calibration horizons (`1h/4h/1d/3d/1w`).

### Other stores (`lib/stores/`)
- `library.ts` — `ingestStatus` writable + `ingest(path)`, which calls `api.ingestPath` and appends a success/failure row. Used by `LibraryView`.
- `settings.ts` — `settings`, `apiKeySet`, `cost` writables + `refreshAll()`, `save()`, `setKey()`, `clearKey()`. Used by `SettingsView`.

---

## 6. Screenshot paste → save → analyze path

This is the signature UX of the app: paste a chart image and ask about it. The flow crosses the frontend/backend boundary twice (once to save, once to analyze).

```
User presses Ctrl+V in the window
        │
        ▼
ScreenshotPaste.onPaste (browser)
  • find first image/* clipboard item
  • blob.arrayBuffer() → btoa(...) → base64 PNG string
  • api.saveScreenshot(b64, 'paste')  ──invoke──►  save_screenshot (Rust)
                                                     • base64-decode bytes
                                                     • write %USERPROFILE%\CogniTrade\
                                                       screenshots\paste-<uuid>.png
                                                     • return absolute path
        │  dispatch('saved', { path })  ◄── path ──┘
        ▼
ChatPanel.onPaste → pendingScreenshot = path
  • composer shows "📎 screenshot ready: <filename>"
        │
        ▼
User types a question (and optional symbol hint), clicks Analyze
        │
        ▼
ChatPanel.send → sendAnalyze({ text, screenshotPath, symbolHint })
        │
        ▼
api.analyze({ user_text, screenshot_path, symbol_hint })  ──invoke──►  analyze (Rust)
        │
        │   backend pipeline (see src-tauri/src/ipc.rs::analyze):
        │     1. persist user message (with image_path)
        │     2. resize screenshot if oversized
        │     3. resolve symbol: user hint → else Haiku vision extract
        │        from the screenshot → else fallback "BTCUSDT"
        │     4. RAG: embed query, brute-force cosine top-6 over doc_chunks
        │     5. build cached Anthropic request (history + retrieved
        │        knowledge + image + user text), stream SSE
        │     6. forward each text delta ──emit──►  'analysis_chunk'  ──► streaming store
        │     7. parse trailing ```json block → AnalysisJson, store a
        │        prediction row, record CoinGecko price, persist assistant msg
        │     8. emit 'analysis_done' { message_id, analysis }  ──► analysisResult + refresh
        ▼
ChatPanel renders the streamed text live, then the final transcript + PredictionCard
```

Key points:

- **Two IPC round-trips.** Saving the screenshot (`save_screenshot`) is separate from analyzing it (`analyze`). The frontend only ever passes a **file path** to `analyze` — the image bytes are read back from disk by the backend, base64-encoded there, and attached to the Anthropic request. This keeps large blobs out of the second IPC payload.
- **Symbol resolution is backend-side.** The frontend's `symbol_hint` is optional. If absent and a screenshot is present, the backend runs a cheap Haiku (`model_extract`, `claude-haiku-4-5-20251001`) vision call to read the ticker off the chart, falling back to `BTCUSDT`. The frontend does not implement symbol detection.
- **Inline image rendering.** Because the saved path is persisted on the user `Message` as `image_path`, `MessageBubble` later renders it inline via `file://`.
- **Paste is window-global.** `ScreenshotPaste` listens on `<svelte:window>`, so a paste works regardless of which input is focused, as long as the Chat tab (which mounts the component) is active.

> **Visual chart reading vs. numeric indicators.** The screenshot path is how the model "sees" a chart. Dedicated numeric indicator widgets (RSI/MACD/Bollinger panels in the UI) described in the spec are **Planned — not yet implemented** on the frontend; today the chart is read visually by the model from the pasted image plus whatever the user types.

---

## 7. State-flow summary

**Frontend (WebView, SPA)**

- `+page.svelte` — tab switch → `ChatPanel` / `LibraryView` / `SettingsView`.
- `ChatPanel` uses the `chat.ts` module-scope stores: `messages`, `streaming`, `analysisResult`, `isAnalyzing`.
- Outbound: `ChatPanel.send` → `sendAnalyze` → `api.analyze` (`ipc.ts`) → `invoke('analyze', …)`.
- Inbound: `initEvents()` registers `listen('analysis_chunk')` and `listen('analysis_done')` once.

**Tauri event bridge (backend → frontend)**

- `analysis_chunk` (text delta) → appended to the `streaming` store.
- `analysis_done` (`{ message_id, analysis }`) → sets `analysisResult`, clears `streaming`, then `refreshMessages()`.

**Backend (Rust process)**

- `ipc::analyze`: persist user msg → resolve symbol → RAG retrieve → stream Anthropic SSE (emits the two events above) → store prediction + assistant msg.
- `chat.db` — `messages`, `predictions`, `cost_daily`, `settings`.
- `vec.db` — `doc_chunks` (embeddings searched by brute-force cosine in Rust; embeddings produced by the fastembed BGE-small singleton).
- API key — OS keyring (never in the DB).

```
 Frontend (SPA)                          Backend (Rust)
 ──────────────                          ──────────────
 ChatPanel.send                          ipc::analyze
   → sendAnalyze                           persist → symbol → RAG → stream
   → api.analyze ──── invoke ───────────►  (chat.db, vec.db, keyring, BGE-small)
                                              │
   streaming  ◄── 'analysis_chunk' ──────────┤  (text deltas, repeated)
   analysisResult,                            │
   refreshMessages  ◄── 'analysis_done' ──────┘  (once, before Ok returns)
```

- **Source of truth for the transcript** is `chat.db` (backend). The frontend mirrors it via `refreshMessages()`; the `streaming` store is a transient live view only.
- **No client-side persistence.** Reloading the WebView re-reads the last 50 messages from the backend. Stores are module-scoped, surviving tab switches but not a process restart.
- **One command does the work.** All analysis flows through the single `analyze` IPC command; the frontend's job is purely to gather inputs, stream output, and render.

---

## 8. Where to look in the code

| You want… | File |
| --- | --- |
| The tab shell | `src/routes/+page.svelte` |
| SPA / SSR-off config | `svelte.config.js`, `src/routes/+layout.ts` |
| Global theme | `src/app.css`, `src/routes/+layout.svelte` |
| Tailwind / PostCSS config | `tailwind.config.js`, `postcss.config.js` |
| Chat state + event listeners | `src/lib/stores/chat.ts` |
| Typed IPC calls | `src/lib/ipc.ts` |
| Shared types | `src/lib/types.ts` |
| Components | `src/lib/components/*.svelte` |
| Backend commands (other side of IPC) | `src-tauri/src/ipc.rs` |
