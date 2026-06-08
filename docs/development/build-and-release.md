# Build & Release (Windows)

This document describes how to produce and distribute a Windows installer for **CogniTrade** — a Tauri 2 desktop app (Rust backend + SvelteKit/TypeScript frontend). Windows is the **primary** target platform; all examples use PowerShell / `cmd` conventions and the Windows MSVC toolchain.

> CogniTrade is an **advisory** crypto-chart analyst. It streams analysis from the Anthropic API and records self-calibrating predictions; it does **not** execute trades. None of that changes the build process, but it does affect what the shipped binary needs at runtime (an Anthropic API key in the OS keyring, plus network access to api.anthropic.com and api.coingecko.com).

Related docs:

- Project overview and constraints: [`../../CLAUDE.md`](../../CLAUDE.md)
- Design plans (aspirational, diverges from current code): [`../superpowers/`](../superpowers/)

---

## 1. Prerequisites (Windows build host)

You need a native toolchain on the build machine. None of this ships inside the installer except WebView2 (see [§7](#7-webview2-runtime)).

| Requirement | Why | Notes |
|---|---|---|
| **Rust (MSVC toolchain)** | Compiles `src-tauri/` | Install via `rustup`; use the default `x86_64-pc-windows-msvc` target. Do **not** use the GNU toolchain. |
| **Visual Studio C++ Build Tools** | Native linker + Windows SDK | "Desktop development with C++" workload. Required by `rusqlite` (bundled SQLite, compiled from C) and Tauri's WiX/NSIS bundlers. |
| **Node.js + npm** | Builds the SvelteKit frontend | Frontend is a static SPA (`@sveltejs/adapter-static`). |
| **WebView2 Runtime** | Renders the UI at runtime | Pre-installed on Windows 11 and current Windows 10. Needed to *run*, not to *build*. |

**Auto-provisioned by Tauri (no manual install needed):** the **WiX Toolset v3** (MSI generation) and **NSIS** (`.exe` setup generation) are downloaded automatically by the Tauri CLI on the first MSI/NSIS build, respectively. You do not install these yourself; they are listed here only so you know what the bundler is fetching. See [§2](#2-the-build-command) and [§7](#7-webview2-runtime).

The Rust dependency `fastembed 4` (BGE-small embeddings) downloads its model **at runtime on first use**, not at build time — so the build itself does not require the model file. See [`../../CLAUDE.md`](../../CLAUDE.md) for the 8 GB RAM hardware constraint that drove the BGE-small / CPU-only choice.

One-time after clone:

```powershell
npm install
```

---

## 2. The build command

```powershell
npm run tauri build
```

This is wired through `package.json` (`"tauri": "tauri"`) to the Tauri CLI. The CLI runs, in order:

1. `beforeBuildCommand` from `src-tauri/tauri.conf.json` → `npm run build` → `vite build`, emitting the static frontend to `build/` (referenced by `frontendDist: "../build"`).
2. `cargo build --release` for the `cognitrade-app` crate (`src-tauri/`).
3. The configured **bundlers** package the release binary into installers. On the first MSI/NSIS build, Tauri downloads WiX/NSIS automatically (see [§1](#1-prerequisites-windows-build-host)).

Useful flags:

```powershell
# Build only a specific bundle type (faster, no full "all" sweep)
npm run tauri build -- --bundles msi
npm run tauri build -- --bundles nsis

# Verbose output (helpful when WiX/NSIS download or signing fails)
npm run tauri build -- --verbose
```

> `npm run tauri build` always compiles in **release** mode. There is no separate `--release` flag. For an unbundled debug binary use `npm run tauri dev`.

---

## 3. Where artifacts land

All build output goes under `src-tauri/target/`. The compiled executable and bundles:

| Artifact | Path (relative to repo root) |
|---|---|
| Release executable | `src-tauri/target/release/cognitrade-app.exe` |
| MSI installer (WiX) | `src-tauri/target/release/bundle/msi/cognitrade-app_0.1.0_x64_en-US.msi` |
| NSIS setup (`.exe`) | `src-tauri/target/release/bundle/nsis/cognitrade-app_0.1.0_x64-setup.exe` |

The version segment (`0.1.0`) and architecture (`x64`) come from `tauri.conf.json` and the build target. The product file name derives from `productName` in `tauri.conf.json`, which is currently **`cognitrade-app`**.

> **Release UX note — `productName` is user-facing.** `productName` is the lowercase developer slug `cognitrade-app`, but it is also what end users see: the installer wizard title, the installed application name, and the **Add/Remove Programs** entry all read `cognitrade-app`, *not* the friendly window title `CogniTrade` (defined separately under `app.windows[].title`). Before a public release, consider setting `productName` to `CogniTrade` for a clean user-facing name. Be aware that doing so **changes the installer file names** (e.g. `CogniTrade_0.1.0_x64_en-US.msi`) and can affect MSI in-place upgrades from any previously shipped `cognitrade-app` build — test upgrades after the rename. The `identifier` ([§4](#4-tauriconfjson-bundle-settings-windows-relevant)), not `productName`, governs the upgrade relationship, so plan the rename together with a versioning decision.

> Note: the bundle file names above match the standard Tauri 2 naming scheme. If you change `productName` or `version`, the file names change accordingly.

---

## 4. `tauri.conf.json` bundle settings (Windows-relevant)

The current `src-tauri/tauri.conf.json` (top-level + bundle block; tray icon shown because it matters for icon regeneration):

```json
{
  "productName": "cognitrade-app",
  "version": "0.1.0",
  "identifier": "com.cognitrade.app",
  "app": {
    "trayIcon": {
      "iconPath": "icons/icon.png"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

| Field | Current value | Effect on Windows |
|---|---|---|
| `productName` | `cognitrade-app` | Drives installer file names, the installed application name, and the Add/Remove Programs entry (user-facing — see [§3](#3-where-artifacts-land)). |
| `version` | `0.1.0` | Embedded in the MSI/NSIS file name and installer metadata. Must stay in sync — see [§8](#8-versioning). |
| `identifier` | `com.cognitrade.app` | App identifier (also used for the MSI `UpgradeCode` derivation, app-data isolation, and single-instance). **Do not change after release** — changing it breaks in-place MSI upgrades. |
| `bundle.active` | `true` | Bundling is enabled. |
| `bundle.targets` | `"all"` | On Windows this produces **both** MSI (WiX) **and** NSIS. Restrict with `--bundles` or set this to `["msi"]` / `["nsis"]`. |
| `bundle.icon` | array incl. `icons/icon.ico` | `icons/icon.ico` is the one used for the Windows `.exe`, the installer, and Add/Remove Programs. The `.icns` entry is macOS-only and ignored on Windows. |
| `app.trayIcon.iconPath` | `icons/icon.png` | The **system tray** icon used at runtime (see [§5](#5-runtime-expectations-of-the-installed-app)). **Not** a member of `bundle.icon` — see the warning in [§4.1](#41-icons-bundle-vs-tray). |

### 4.1 Icons: bundle vs. tray

The icon set lives in `src-tauri/icons/` and includes:

- `icon.ico` — Windows executable / installer / Add-Remove-Programs icon (member of `bundle.icon`).
- `icon.icns` — macOS only (member of `bundle.icon`, ignored on Windows).
- `32x32.png`, `128x128.png`, `128x128@2x.png` — additional bundle icons.
- `icon.png` — **the system tray icon** referenced by `app.trayIcon.iconPath`. **This is NOT in the `bundle.icon` array.**
- `Square*Logo.png`, `StoreLogo.png` — MSIX/Store assets, not currently targeted.

> **Warning — preserve `icons/icon.png`.** The runtime system tray (installed by `lib.rs::run()`; see [§5](#5-runtime-expectations-of-the-installed-app) and [§9](#9-release-checklist)) loads `icons/icon.png`, which is referenced by `app.trayIcon.iconPath` and is **independent of the `bundle.icon` array**. If you regenerate icons with `npm run tauri icon` or trim the icon set to only the `bundle.icon` members, you can silently lose `icon.png` and break the tray icon at runtime. Always keep `icons/icon.png` present (and regenerate or hand-place it after running `tauri icon`).

To regenerate all icons from a single source PNG:

```powershell
npm run tauri icon path\to\source-1024.png
```

After running this, confirm `icons/icon.png` still exists (and matches the intended tray artwork) before building.

### 4.2 Windows-specific bundle tuning (optional, not currently set)

There is **no `bundle.windows` block** in the config today. If you need to tune WiX/NSIS behavior, add one. Common keys:

```json
"bundle": {
  "windows": {
    "webviewInstallMode": { "type": "downloadBootstrapper" },
    "wix": { "language": ["en-US"] },
    "nsis": { "installMode": "currentUser" }
  }
}
```

See [§7](#7-webview2-runtime) for `webviewInstallMode` and [§6](#6-code-signing-authenticode) for the signing keys that also live under `bundle.windows`.

---

## 5. Runtime expectations of the installed app

These do not affect the build but determine whether a freshly installed app *works*. They are covered here so the release checklist can verify them.

- **User data directory**: on first run, `lib.rs::run()` creates `%USERPROFILE%\CogniTrade\` with subfolders `library\`, `screenshots\`, `data\`, `logs\`. Two SQLite databases live under `data\`: `chat.db` (messages, predictions, cost_daily, settings) and `vec.db` (the `doc_chunks` embedding store). **Never** bundle or commit these.
- **Anthropic API key**: stored in the **Windows Credential Manager** via the `keyring` crate (service `cognitrade`, user `anthropic_api_key`). The installer does not provision it; the user enters it in the Settings tab on first launch. It is never written to any DB.
- **Network**: the app calls `api.anthropic.com` (analysis + Haiku vision/extraction) and `api.coingecko.com` (price snapshots and prediction resolution).
- **Global hotkey + tray**: `Ctrl+Shift+Space` toggles the panel (registered via `tauri-plugin-global-shortcut`); the app also installs a **system tray icon** loaded from `icons/icon.png` (see [§4.1](#41-icons-bundle-vs-tray)). Both are runtime behaviors, but worth smoke-testing post-install.

---

## 6. Code signing (Authenticode)

> **Status: Planned — not yet implemented.** There is no signing configuration in `tauri.conf.json` today, and no certificate is wired into the build. Installers produced by `npm run tauri build` are currently **unsigned**. Unsigned installers trigger SmartScreen / "Unknown publisher" warnings on end-user machines.

Authenticode signing (performed via `signtool.exe`) covers both the `cognitrade-app.exe` and the MSI/NSIS installer. To wire it up later, Tauri signs automatically during `tauri build` when a `bundle.windows.certificateThumbprint` (or related keys) is present.

### 6.1 How to wire it (when a certificate is obtained)

1. Acquire an Authenticode code-signing certificate (OV or, preferably for SmartScreen reputation, EV — EV certs are hardware-token / HSM based).
2. Install the certificate into the host's certificate store and note its **SHA-1 thumbprint**.
3. Add a Windows signing block to `src-tauri/tauri.conf.json`:

   ```json
   "bundle": {
     "windows": {
       "certificateThumbprint": "REPLACE_WITH_SHA1_THUMBPRINT",
       "digestAlgorithm": "sha256",
       "timestampUrl": "http://timestamp.digicert.com"
     }
   }
   ```

4. Run `npm run tauri build`. Tauri invokes `signtool.exe` on the produced binaries and installers.

**Recommendations when implementing:**

- Always set a `timestampUrl` so signatures remain valid after the certificate expires.
- For CI/HSM-based EV signing, prefer Tauri's `signCommand` hook (custom command) over an embedded thumbprint so the private key never touches config files.
- Keep certificate material out of the repo and out of `tauri.conf.json` in version control; inject via environment/CI secrets.

Until this is done, communicate to testers that the warning is expected.

---

## 7. WebView2 runtime

CogniTrade renders its UI in **WebView2**. The runtime is present by default on Windows 11 and up-to-date Windows 10, but a clean/old machine may lack it. Tauri controls how the installer handles this via `bundle.windows.webviewInstallMode`.

**Current behavior:** no `webviewInstallMode` is set, so Tauri's default applies — `downloadBootstrapper`: the installer ships a tiny bootstrapper that downloads and installs WebView2 at install time **if missing** (requires internet during install).

Options if you need different behavior (set under `bundle.windows.webviewInstallMode`):

| `type` | Behavior | Trade-off |
|---|---|---|
| `downloadBootstrapper` (default) | Small installer; fetches WebView2 online if absent. | Needs internet at install time. |
| `embedBootstrapper` | Embeds the bootstrapper; still downloads the runtime. | Slightly larger installer; still online. |
| `offlineInstaller` | Embeds the **full** WebView2 installer. | Largest installer (~150 MB); works fully offline. |
| `skip` | Assumes WebView2 already present. | Smallest installer; app fails to launch if runtime is missing. |

Example (fully offline-capable installer):

```json
"bundle": { "windows": { "webviewInstallMode": { "type": "offlineInstaller" } } }
```

Given the constrained-PC / possibly-offline target audience described in [`../../CLAUDE.md`](../../CLAUDE.md), `offlineInstaller` is worth considering for distributed builds; the default `downloadBootstrapper` is fine for online installs.

---

## 8. Versioning

The version **must be kept in sync across three files**. They are independent today and nothing enforces consistency, so a mismatch is easy to introduce and produces confusing installer metadata.

| File | Field | Current |
|---|---|---|
| `package.json` | `"version"` | `0.1.0` |
| `src-tauri/Cargo.toml` | `[package] version` | `0.1.0` |
| `src-tauri/tauri.conf.json` | `"version"` | `0.1.0` |

The installer file name and product metadata come from **`tauri.conf.json`**. The frontend `package.json` version and the Rust crate version should match it to avoid drift in logs, support, and `cargo` output.

**Bump procedure:**

1. Update all three fields to the identical new SemVer value.
2. Confirm the `identifier` (`com.cognitrade.app`) is **unchanged** — it governs MSI in-place upgrades.
3. Rebuild and verify the file name in `src-tauri/target/release/bundle/` reflects the new version.

> Tip: a single-source pattern is available — set `tauri.conf.json` `"version": "../package.json"` to inherit from `package.json`. This is **not** configured today; it would still leave `Cargo.toml` separate.

---

## 9. Release checklist

Run on a clean build host (or after `cargo clean` in `src-tauri/` and deleting `build/`).

- [ ] All three version fields synced (`package.json`, `Cargo.toml`, `tauri.conf.json`) — see [§8](#8-versioning).
- [ ] `identifier` unchanged from previously shipped releases.
- [ ] `productName` decision confirmed for this release (slug `cognitrade-app` vs. friendly `CogniTrade`) — see [§3](#3-where-artifacts-land).
- [ ] `icons/icon.png` (tray icon) present after any icon regeneration — see [§4.1](#41-icons-bundle-vs-tray).
- [ ] `npm install` run; `npm run check` (svelte-check) passes.
- [ ] `cd src-tauri && cargo test` passes. (Integration tests `tests/ingest_integration.rs` and `tests/llm_integration.rs` are `#[ignore]`d and downloads/network-dependent — run intentionally with `cargo test -- --ignored` if validating the full pipeline.)
- [ ] `npm run tauri build` completes with no warnings about missing bundlers/icons.
- [ ] Artifacts present in `src-tauri/target/release/bundle/msi/` and/or `nsis/`.
- [ ] **Fresh-machine install test** (or clean VM): installer runs, WebView2 resolves, app launches.
- [ ] First-run smoke test: `%USERPROFILE%\CogniTrade\{library,screenshots,data,logs}` created; Settings tab accepts an Anthropic API key (lands in Credential Manager, not in any DB); a chat analysis streams (`analysis_chunk` → `analysis_done`) and a prediction row is written.
- [ ] `Ctrl+Shift+Space` toggles the window; tray icon present (loaded from `icons/icon.png`).
- [ ] (Once signing is implemented — [§6](#6-code-signing-authenticode)) installer and `.exe` are Authenticode-signed and timestamped; no SmartScreen "Unknown publisher" warning.
- [ ] Release notes drafted; artifacts archived alongside their checksums.

---

## 10. Reproducibility

The build is **not yet bit-for-bit reproducible**, and a few specific factors prevent it. Document the build environment with every release so a binary can be re-derived as closely as possible.

Known sources of non-determinism / drift:

- **Toolchain versions**: Rust, the MSVC linker/SDK, and Node/npm versions all influence output. Pin and record them. Consider committing a `rust-toolchain.toml` to pin the Rust version.
- **Unpinned crate versions**: `Cargo.toml` uses caret ranges (e.g. `tauri = "2"`, `rusqlite = "0.31"`). The committed **`Cargo.lock`** is the actual reproducibility anchor. `src-tauri/Cargo.lock` **already exists in the tree** — it just isn't under version control yet. Use `cargo build --locked` semantics for releases.
- **Unpinned npm deps**: `package.json` uses `^` ranges; `package-lock.json` is the anchor. `package-lock.json` **already exists in the tree** (generated by an earlier `npm install`) — it likewise just needs to be committed. Use `npm ci` for release builds.
- **Not a git repository yet**: this repo is **not currently a git repository**, so neither lockfile is tracked. Run `git init` and commit the existing `Cargo.lock` and `package-lock.json` before relying on reproducibility — they do not need to be regenerated, only committed.
- **WiX/NSIS tool versions**: Tauri downloads these; pin/cache the same versions across release machines.
- **Code-signing**: signatures embed a timestamp and certificate, so a signed installer is never byte-identical to an unsigned one (expected).
- **Runtime model download is out of band**: the fastembed BGE-small model is fetched on first run, not baked into the installer — so it does not affect installer reproducibility, but it does mean two installs can differ in cached state.

Minimum to record per release: Rust version, Node/npm versions, `Cargo.lock` + `package-lock.json` hashes, Tauri CLI version, and the produced artifact SHA-256 checksums.
