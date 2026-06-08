# CogniTrade — User Guide (Windows)

CogniTrade is a small desktop app that reads a cryptocurrency **chart screenshot** and gives you a plain‑English analysis ending in a structured recommendation card. It reads the chart with Claude (vision), pulls relevant passages from any trading PDFs/notes you have added (RAG), and streams the result back into a chat panel.

> **CogniTrade is an advisor, not a trader.** It does **not** connect to any exchange, does **not** place orders, and does **not** manage live positions. Its own system prompt is explicit: *"You are an advisor. The user places all trades themselves."* Every recommendation is a suggestion for **you** to act on (or ignore). See [Understand the recommendation](#6-understand-the-recommendation) for the disclaimer in full.

This guide covers installing, configuring, and using CogniTrade on Windows.

---

## Contents

1. [Install](#1-install)
2. [First launch](#2-first-launch)
3. [Set your Anthropic API key and daily cost cap](#3-set-your-anthropic-api-key-and-daily-cost-cap)
4. [Add knowledge (PDFs and images)](#4-add-knowledge-pdfs-and-images)
5. [Run an analysis](#5-run-an-analysis)
6. [Understand the recommendation](#6-understand-the-recommendation)
7. [The Library and Settings tabs](#7-the-library-and-settings-tabs)
8. [How predictions get scored over time](#8-how-predictions-get-scored-over-time)
9. [Privacy: what stays local and what goes out](#9-privacy-what-stays-local-and-what-leaves-your-pc)
10. [Where your data lives](#10-where-your-data-lives)
11. [Troubleshooting](#11-troubleshooting)

---

## 1. Install

CogniTrade ships as a Windows installer produced by `npm run tauri build` (WiX **MSI** and/or NSIS `.exe`, per `bundle.targets: "all"` in `src-tauri/tauri.conf.json`). If you are building it yourself rather than receiving a prebuilt installer, see [Build & release](../development/build-and-release.md) and [Windows developer setup](../development/windows-setup.md).

### Requirements

| Requirement | Notes |
| --- | --- |
| Windows 10 / 11 (x64) | Primary supported platform. |
| **WebView2 Runtime** | The app renders its UI in Microsoft Edge WebView2. Most up‑to‑date Windows installs already have it; if not, the installer or Windows Update provides it (see [granting WebView2](#webview2)). |
| An Anthropic API key | Required for analysis and for ingesting **images**. PDFs can be ingested without a key. Get one from the [Anthropic Console](https://console.anthropic.com/). |
| Internet access | For Claude (Anthropic) and live prices (CoinGecko). Everything else runs locally. |

CogniTrade is designed to run comfortably on modest hardware (the reference target is a Ryzen 3400G / 8 GB RAM machine), so it keeps models and the embedding index small and CPU‑only.

### Install steps

1. Double‑click the installer (`cognitrade-app_<version>_x64_en-US.msi`, or the NSIS `.exe`).
2. Accept the prompts and finish the wizard.
3. Launch **CogniTrade** from the Start menu.

The first time it runs, CogniTrade creates its data folder tree under your profile (see [Where your data lives](#10-where-your-data-lives)). No data is created until first launch.

---

## 2. First launch

CogniTrade runs as a **tray application with a floating panel** — it does not behave like a normal windowed program.

- On launch the main window is **hidden** (`visible: false` in the Tauri config) and a CogniTrade **icon appears in your system tray** (the notification area at the bottom‑right of the taskbar).
- The panel is a narrow, borderless, always‑on‑top window that docks near the top‑right of your screen. **Each time you open the panel, CogniTrade repositions it to the top‑right and resizes it to 420 × 800** (`tray.rs::position_panel`), so any manual move/resize you did is reset on the next show.

### Showing and hiding the panel

There are two equivalent ways to toggle the panel:

| Method | How |
| --- | --- |
| **Global hotkey** | Press **`Ctrl+Shift+Space`** anywhere — even while another app is focused. Press again to hide. |
| **Tray icon** | **Left‑click** the tray icon to toggle, or **right‑click** for a menu with **Toggle Panel** and **Quit**. |

The hotkey is registered globally at startup (`lib.rs`), so you can pop the panel open over your charting app, paste a screenshot, and get an analysis without switching windows.

> To fully exit CogniTrade, right‑click the tray icon and choose **Quit**. Closing the panel only hides it.

<a id="webview2"></a>
### Granting / installing WebView2

CogniTrade's interface is drawn by the **Microsoft Edge WebView2 Runtime**. If the panel opens blank or the app refuses to start with a WebView2 error:

1. Download the **Evergreen WebView2 Runtime** from Microsoft: <https://developer.microsoft.com/microsoft-edge/webview2/>.
2. Run the bootstrapper and let it install (it is a small, system component).
3. Relaunch CogniTrade.

WebView2 is a standard Windows component and does not require a developer account or any special permission grant beyond running its installer. Once installed it is shared system‑wide; you only do this once.

---

## 3. Set your Anthropic API key and daily cost cap

Open the panel and click the **Settings** tab in the header (`Chat · Library · Settings`).

### API key

CogniTrade calls the Claude API, so you must provide an Anthropic API key before running an analysis.

1. In **Settings → API key**, paste your key (it looks like `sk-ant-…`) into the password field.
2. Click **Save key**.
3. The section now shows **"key is set"** with a **Clear** button.

**Where the key is stored:** Your key is saved in the **Windows Credential Manager** via the OS keyring (service `cognitrade`, account `anthropic_api_key`). It is **never** written to CogniTrade's databases, config files, or logs. Clearing it removes it from Credential Manager.

### Daily cost cap

CogniTrade meters every Claude call and totals the spend **per calendar day** (UTC). Before each main analysis call it checks the day's running total against your cap and **refuses the call** if you are at or over it (you'll see a `CostCapReached` error in the chat).

1. In **Settings → Daily cap (USD)**, set the limit (default **$2.00**, minimum $0.50, steps of $0.50).
2. Below the field, **"today: $X.XXX"** shows how much you have spent so far today.
3. Click **Save** at the bottom to persist your changes.

The cap protects you from runaway spend; it does not refund anything. Token pricing is applied per model from a built‑in rate table:

| Model role | Default model | Approx. price (input / cached / output per million tokens) |
| --- | --- | --- |
| **Main** (analysis) | `claude-sonnet-4-6` | $3 / $0.30 / $15 |
| Main (optional) | `claude-opus-4-7` | $15 / $1.50 / $75 — slower, costlier |
| **Extract** (vision symbol detection + image description) | `claude-haiku-4-5-20251001` | $1 / $0.10 / $5 |

> **About the model ids.** The extract model uses a dated id (`claude-haiku-4-5-20251001`), while the optional main model is listed as the un‑dated `claude-opus-4-7`. That opus id is simply the value currently hardcoded in the app's Settings dropdown (`SettingsView.svelte`) and in the cost meter (`cost.rs`); it is **not** independently verified here and may need updating to a current Anthropic model id. If a call fails with an "unknown model" error, set the model to the live id Anthropic publishes for that model.

> Prices above are the rates hardcoded in the app's cost meter (`cost::rates_for`) and are used only to estimate spend against your cap. Always confirm current pricing in the Anthropic Console.

CogniTrade reduces cost by **prompt caching** the system prompt and the retrieved‑knowledge block (cached input tokens are billed at the much lower "cached" rate).

### Choosing the main model

In **Settings → Main model** you can switch between:

- `claude-sonnet-4-6` *(default)* — the recommended balance of quality and cost.
- `claude-opus-4-7` — higher quality, noticeably slower and more expensive. (See the note above — this id may need updating to a current Anthropic model id.)

Click **Save** after changing it.

---

## 4. Add knowledge (PDFs and images)

CogniTrade can ground its analysis in your own material — trading books, strategy PDFs, research notes, annotated chart images. This is the **RAG (retrieval‑augmented generation)** library. When you run an analysis, CogniTrade searches this library for relevant passages and feeds the best matches to Claude, which can then **cite** them in the recommendation.

### The library folder

Everything lives in:

```
%USERPROFILE%\CogniTrade\library
```

(For example `C:\Users\you\CogniTrade\library`.) This folder is created automatically on first launch.

### Auto‑ingestion: just drop files in

CogniTrade **watches the library folder** and ingests files automatically whenever you add or modify them (a file‑system watcher with an 800 ms debounce). You don't have to click anything — copy a PDF or image into `%USERPROFILE%\CogniTrade\library` and it gets processed in the background.

Supported file types:

| Type | Extensions | Needs API key? | How it's processed |
| --- | --- | --- | --- |
| PDF | `.pdf` | **No** | Text is extracted, split into ~500‑word chunks (50‑word overlap), embedded locally, and stored. |
| Image | `.png`, `.jpg`, `.jpeg`, `.webp` | **Yes** | Claude Haiku describes the image; that description is chunked, embedded, and stored. |

> **Images are skipped silently if no API key is set.** If you drop chart images into the library before saving your key, they will not be ingested and you'll see no error — set the key first, then re‑drop the images (modifying them re‑triggers the watcher).

### Adding files from inside the app

You can also use the **Library** tab:

1. Click **Add documents**.
2. Pick one or more `.pdf` / `.png` / `.jpg` / `.jpeg` / `.webp` files.
3. Each file shows a status line — ✅ with a chunk count on success, ❌ with the error otherwise.

Note that picking a file through **Add documents** ingests it from wherever it sits on disk; it does **not** copy it into the library folder. To have a file auto‑managed by the watcher, put it in `%USERPROFILE%\CogniTrade\library`.

### How embedding works (and the first‑run delay)

CogniTrade embeds text **on your PC** using a small local model (BGE‑small, 384‑dimensional). The **first time** anything is embedded — your first ingest or your first analysis — the model is downloaded (a few hundred MB) and there will be a one‑time delay. After that, embedding is fast, offline, and CPU‑only. Embeddings are stored locally; nothing about your documents is uploaded to embed them (image *descriptions* are produced by Claude, but PDFs never leave your machine).

---

## 5. Run an analysis

The **Chat** tab is where you work. The typical flow is: grab a chart screenshot → paste it → optionally hint the symbol → click **Analyze** → read the streamed explanation and the recommendation card.

### Step by step

1. **Capture a chart.** Use any tool (e.g. **Win + Shift + S** Snipping Tool) to copy a chart screenshot to your clipboard. CogniTrade works best with a clear candlestick chart showing the symbol, timeframe, and price.

2. **Open the panel** with **`Ctrl+Shift+Space`**.

3. **Paste the screenshot** with **`Ctrl+V`** while the panel is focused. CogniTrade saves the image to `%USERPROFILE%\CogniTrade\screenshots\` and shows a `📎 screenshot ready: …` line above the input. Whatever format the clipboard image is in, CogniTrade **always saves and sends it as a PNG** (`save_screenshot` writes a `.png`, and the analysis call sends it with media type `image/png`).

4. **(Optional) Symbol hint.** In the `symbol (e.g. BTCUSDT)` box you can type the ticker. This is recommended:
   - If you **provide** a hint, CogniTrade uses it directly.
   - If you **leave it blank *and* pasted a screenshot**, CogniTrade asks Haiku to read the symbol from the image (this requires an API key and adds a small charge); if that read fails it falls back to **`BTCUSDT`**.
   - If you **leave it blank with no screenshot**, there is nothing to read, so CogniTrade uses **`BTCUSDT`** directly (no Haiku call).
   - Live‑price features (recording the price now and at the horizon) only work for a built‑in set of ~12 coins: **BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC** (the `USDT`/`USD`/`USDC` suffix is stripped). Symbols outside this list still get an analysis, but **no starting price is recorded and the prediction can't be scored** (see [Section 8](#8-how-predictions-get-scored-over-time)).

5. **(Optional) Type a question** in the text area — e.g. *"is this a breakout or a fakeout?"*. You can analyze with just an image, just text, or both. (You can't submit an empty message with no screenshot.)

6. **Click `Analyze`.** The button shows **Analyzing…** while it works.

### What you see while it runs

- The explanation **streams in live**, word by word, as Claude writes it (driven by the `analysis_chunk` event).
- When it finishes, the streamed text becomes a saved chat message and a **recommendation card** (the `PredictionCard`) appears below it.

Behind the scenes CogniTrade resolves the symbol, searches your library for the 6 most relevant passages, sends the chat history + retrieved passages + your image + your text to Claude, streams the reply, parses the structured recommendation from the end of the reply, records the live price (for supported symbols), and saves everything. For the full request flow see [IPC API reference](../architecture/ipc-api-reference.md).

---

## 6. Understand the recommendation

Every analysis ends with a fenced ```json block that CogniTrade parses into the **recommendation card**. The card summarizes Claude's structured opinion:

| Field | Card label | Meaning |
| --- | --- | --- |
| `action` | the colored pill (**BUY** green / **SELL** red / **HOLD** grey) | The suggested stance. |
| `probability_up` | **P(up) NN%** | Claude's estimated probability that price rises over the horizon, shown as a percentage (always between 0% and 100%). |
| `horizon` | next to P(up) | The time window the call is about: **`1h` · `4h` · `1d` · `3d` · `1w`**. |
| `stop_loss_pct` | **SL: N%** | Suggested stop‑loss distance, as a percentage. |
| `take_profit_pct` | **TP: N%** | Suggested take‑profit distance, as a percentage. |
| `key_signals` | bulleted list | Short reasons (patterns, indicators) behind the call. |
| `citations` | **Cited:** … | Which of *your* library documents informed the call, shown as `document.pdf#page`. |

The streamed **plain‑English explanation above the card** is the human‑readable rationale — it should reference the chart it read and the passages it cited. The card is just the structured summary of that reasoning.

### Read this — the advisor‑only disclaimer

> **CogniTrade is an advisor. You place all trades yourself.**
>
> - The app has **no exchange integration** (no Binance, no Kraken, nothing) and **cannot execute orders**.
> - **SL / TP are suggestions, not enforced.** CogniTrade does not place stop‑loss or take‑profit orders and does not monitor your positions. Those percentages are advice for you to set on your own exchange if you choose to.
> - `probability_up` is the model's **estimate**, not a guarantee. Treat it as one opinion among many.
> - Nothing here is financial advice. Crypto trading carries real risk of loss. Verify everything independently and trade only what you can afford to lose.

---

## 7. The Library and Settings tabs

The panel header has three tabs: **Chat**, **Library**, **Settings**.

### Library tab

- **Add documents** — pick PDFs/images to ingest on the spot (see [Adding files from inside the app](#adding-files-from-inside-the-app)).
- A reminder that files dropped into `~/CogniTrade/library` are picked up automatically.
- A running status list of recent ingests (filename + ✅ chunk count or ❌ error).

There is currently no in‑app browser/delete view for ingested documents — manage the source files in the `library` folder on disk. Re‑ingesting a PDF/image replaces its previous chunks rather than duplicating them.

### Settings tab

Covered above in [Section 3](#3-set-your-anthropic-api-key-and-daily-cost-cap):

- **API key** — save or clear (stored in Windows Credential Manager).
- **Daily cap (USD)** — with today's running spend.
- **Main model** — `claude-sonnet-4-6` (default) or `claude-opus-4-7` (the opus id is the value hardcoded in the dropdown; see the [model‑id note in Section 3](#3-set-your-anthropic-api-key-and-daily-cost-cap)).

Click **Save** to persist the cap and model. Non‑secret settings are stored in the local `chat.db` database; the API key never is.

---

## 8. How predictions get scored over time

CogniTrade keeps itself honest with a **self‑evaluation loop**. Every analysis is stored as a **prediction** that the app later grades against real price movement.

### What gets recorded

When an analysis completes, CogniTrade saves a prediction row with: the symbol, the **horizon**, the predicted `probability_up`, the recommended action, and the suggested SL/TP percentages. It also tries to fetch the **live price right now** from CoinGecko and store it as the starting price.

> **Only the ~12 supported coins get a starting price.** For any symbol outside the built‑in list (BTC, ETH, SOL, BNB, XRP, ADA, DOGE, AVAX, MATIC, LINK, DOT, LTC), the CoinGecko lookup fails and **no starting price is recorded**. Such a prediction can never produce a meaningful score — even if it later shows up as `resolved`, there is no usable start price to compare against, so it is effectively un‑scorable.

The prediction starts with status **`pending`**.

### How it resolves

A background loop runs every **60 seconds**. When a prediction's horizon has elapsed (its timestamp + horizon seconds is in the past), CogniTrade (`predictions::resolve_one`):

1. Fetches the **current price** from CoinGecko.
2. Records it as the **price at horizon** and marks the prediction **`resolved`** (or **`failed`** if the price lookup didn't succeed — e.g. an unsupported symbol or a network hiccup).

This compares **price at prediction time vs. price at horizon end**. Two important nuances:

> - The scorer checks the price **only at the end of the horizon**. It does **not** simulate whether your suggested stop‑loss or take‑profit would have been hit *during* the window. So a "resolved" prediction tells you which way price went over the period, not whether an SL/TP trade would have triggered.
> - A `resolved` status does **not** guarantee a usable score. For an unsupported symbol the *starting* price was never recorded, so a row can end up `resolved` (horizon price fetched) yet have no start price to score against. Stick to the ~12 supported coins if you want real calibration.

### Housekeeping

- A separate background loop **prunes chat history older than 7 days** (runs every 6 hours). Predictions themselves are retained for scoring.
- Predictions and their outcomes live in the local `chat.db` database; there is no aggregate "calibration score" surfaced in the UI yet. For the table layout see [Data model](../architecture/data-model.md).

---

## 9. Privacy: what stays local and what leaves your PC

CogniTrade is **local‑first**. The only two outbound destinations are Anthropic and CoinGecko.

| Data | Where it lives / goes |
| --- | --- |
| Chat messages, predictions, cost totals, settings | **Local** SQLite (`chat.db`) under your profile. Never uploaded. |
| Ingested document text & embeddings | **Local** SQLite vector store (`vec.db`). Embedded on your PC. |
| Your API key | **Local** Windows Credential Manager only. Never in any DB, file, or log. |
| Screenshots you paste | **Local** `screenshots\` folder, always written as PNG. Sent to Anthropic **only** as part of an analysis you trigger (and always sent as `image/png`). |
| Logs | **Local** `logs\` folder. |
| **→ Anthropic (Claude API)** | Your screenshot (as PNG), your typed question, recent chat history, and the retrieved library passages are sent to Claude **when you click Analyze**. Image files you ingest are also sent to Claude to be described. Per [Anthropic's current commercial terms](https://www.anthropic.com/legal/commercial-terms) (which you should verify yourself, as third‑party terms can change), API inputs are not used to train their models. |
| **→ CoinGecko** | Only the **ticker symbol** is sent (to look up a USD price) when recording or scoring a prediction. No personal data, no chart, no documents. |

Nothing else leaves your machine. PDF *text* is never uploaded (it's embedded locally); only image *descriptions* involve a Claude call.

---

## 10. Where your data lives

All user data sits under your Windows profile:

```
%USERPROFILE%\CogniTrade\
├── library\        ← drop PDFs / chart images here (auto-ingested)
├── screenshots\    ← pasted chart screenshots are saved here (always as PNG)
├── data\
│   ├── chat.db     ← messages, predictions, cost, settings
│   └── vec.db      ← document chunks + embeddings (the RAG store)
└── logs\           ← application logs
```

These folders are created automatically on first launch. **Back up** `data\` if you want to keep your chat history and prediction record; back up `library\` to keep your knowledge base.

Your **API key is not here** — it's in Windows Credential Manager (search "Credential Manager" in the Start menu → Windows Credentials → look for `cognitrade`).

---

## 11. Troubleshooting

| Symptom | Likely cause / fix |
| --- | --- |
| Panel won't open / app won't start, WebView2 error | Install the [WebView2 Runtime](#webview2) and relaunch. |
| Panel keeps jumping to the top‑right / resizing | Expected — CogniTrade repositions to the top‑right and resizes to 420 × 800 every time you open the panel. |
| `Ctrl+Shift+Space` does nothing | Another app may have grabbed the shortcut, or CogniTrade isn't running. Use the **tray icon** to toggle the panel; check the tray for the CogniTrade icon. |
| Error mentioning **NoApiKey** | No API key saved. Go to **Settings → API key** and save your `sk-ant-…` key. (This also appears if you leave the symbol blank with a screenshot but no key — the Haiku symbol read needs a key.) |
| Error mentioning **CostCapReached** | You hit your daily spend cap. Raise it in **Settings → Daily cap**, or wait for the next UTC day. |
| Error mentioning an **unknown model** | The configured main model id may be stale. Set **Settings → Main model** back to `claude-sonnet-4-6`, or update the opus id to a current Anthropic model id. |
| Analysis returns but symbol is wrong / defaults to BTCUSDT | The symbol couldn't be read from the image (or you ran with no screenshot and no hint). Type the ticker in the **symbol hint** box and re‑run. |
| Card shows no SL/TP or no citations | The model omitted them — typically a `hold` or when no library passage was relevant. Add more relevant PDFs/notes to improve citations. |
| Prediction stuck `pending`, resolves to `failed`, or "resolves" but shows no score | The symbol isn't in the supported ~12‑coin price list, or CoinGecko was unreachable. Only listed symbols record a start price and can be price‑scored. |
| Dropped an image into `library\` but nothing ingested | Images need an API key. Save your key first, then modify/re‑drop the image to re‑trigger ingestion. PDFs work without a key. |
| First ingest or first analysis is slow | One‑time embedding‑model download (BGE‑small). Subsequent runs are fast and offline. |

---

## Related docs

- [IPC API reference](../architecture/ipc-api-reference.md) — every command behind the UI.
- [Data model](../architecture/data-model.md) — the `chat.db` / `vec.db` schemas.
- [Tech stack](../architecture/tech-stack.md) — what CogniTrade is built on.
- [Windows developer setup](../development/windows-setup.md) and [Build & release](../development/build-and-release.md) — if you're building the installer yourself.
- [Vision & scope](../project/vision-and-scope.md) — what CogniTrade is and explicitly is **not**.
