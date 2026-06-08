# Testing Strategy

This document describes how CogniTrade is tested today, how to run the test
suites, and where coverage is weak. It reflects **what the code actually does** —
where a capability is only described in the spec but not implemented, it is
labelled **Planned — not yet implemented**.

> **Scope reminder.** CogniTrade is an *advisory* crypto-chart analyst built on
> Tauri 2 (Rust backend + SvelteKit/TypeScript frontend). It does **not** place
> orders, integrate with Binance/Kraken, or run any autonomous trading or
> Minimax engine. The system prompt is explicit: "You are an advisor. The user
> places all trades themselves." Tests therefore cover analysis, ingestion,
> retrieval, cost accounting, and prediction self-calibration — not order
> execution.

Related docs:

- Architecture and data flow: `../superpowers/specs/2026-05-05-cognitrade-design.md`
- Implementation plan (Phase 1–5): `../superpowers/plans/2026-05-05-cognitrade-v1.md`
- Project root guidance: `../../CLAUDE.md`

---

## Testing at a glance

| Layer | Mechanism | Where | Default `cargo test`? |
|-------|-----------|-------|-----------------------|
| Rust unit tests | inline `#[cfg(test)] mod tests` | next to each module under `src-tauri/src/` | Yes |
| Rust integration (mocked HTTP) | `wiremock` mock server | `src-tauri/tests/llm_integration.rs` | Yes |
| Rust integration (network/model) | `#[ignore]`d function | `src-tauri/tests/ingest_integration.rs` | No — opt-in |
| Frontend type check | `svelte-check` | whole `src/` tree | n/a (separate command) |
| Frontend unit / component / e2e | — | — | **Planned — not yet implemented** |

There is exactly **one** `#[ignore]`d test in the whole tree:
`ingest_pdf_into_lancedb` in `src-tauri/tests/ingest_integration.rs`. Every other
test — including the entire contents of `llm_integration.rs` — runs by default.

> **Cross-reference with `CLAUDE.md`.** The root `CLAUDE.md` lists a command
> `cargo test --test llm_integration -- --ignored` (described there as needing a
> live API key). **That invocation is currently a no-op:** `llm_integration.rs`
> contains a single test (`analyze_streams_text_and_parses_json`) that is *not*
> `#[ignore]`d, so the `--ignored` filter selects zero tests. There is presently
> **no** live-API LLM integration test. The only opt-in `#[ignore]`d test is the
> ingest smoke test below. Use the quick reference at the bottom of this document
> as the authoritative list of working commands.

The backend uses four `dev-dependencies` for testing, declared in
`src-tauri/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
wiremock = "0.6"
tokio = { version = "1", features = ["full", "test-util"] }
pretty_assertions = "1"
```

---

## Backend unit tests — the inline `#[cfg(test)]` convention

Every backend module keeps its tests **in the same file**, in a trailing
`#[cfg(test)] mod tests { use super::*; ... }` block. There is no separate
`unit/` tree and no shared test-helper crate. Tests exercise the module's public
functions directly and use real (temp-backed) SQLite databases rather than mocks
wherever a DB is involved.

The convention in practice:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;          // pull in sibling modules as needed

    #[test]
    fn does_the_thing() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("chat.db")).unwrap();
        // ... assert on real behaviour ...
    }
}
```

### Modules with inline tests

The following modules currently carry inline tests. To regenerate this list, run
the grep below **from `src-tauri/`** (the working directory assumed by every
other command block in this document):

```powershell
# cwd = src-tauri
grep -rn "#\[cfg(test)\]" src
```

| Module | File | What the tests pin down |
|--------|------|--------------------------|
| Cost accounting | `src-tauri/src/cost.rs` | rate tables and per-day accumulation (see below) |
| Predictions | `src-tauri/src/predictions.rs` | insert-from-analysis and "becomes due" logic |
| Paths | `src-tauri/src/paths.rs` | `ensure_all()` creates `library/screenshots/data/logs` |
| Settings | `src-tauri/src/settings.rs` | defaults + save/load round-trip |
| Database | `src-tauri/src/db.rs` | `open()` runs migration `001_initial.sql` |
| Prompt builder | `src-tauri/src/llm/prompt.rs` | message assembly, cache blocks, history ordering |
| CoinGecko | `src-tauri/src/coingecko.rs` | ticker→id normalization |
| JSON-tail parser | `src-tauri/src/llm/parse.rs` | `extract_json_tail` extraction + validation |
| SSE parser | `src-tauri/src/llm/streaming.rs` | `parse_event` over Anthropic event types |
| Chunker | `src-tauri/src/ingest/chunker.rs` | `chunk_text` sizing/overlap behaviour |
| PDF extract | `src-tauri/src/ingest/pdf.rs` | missing-file error path |
| Image util | `src-tauri/src/image_util.rs` | resize-needed detection and in-place resize |
| Chat store | `src-tauri/src/chat_store.rs` | insert/recent ordering and prune |

### Worked examples

**`cost.rs`** — pure arithmetic plus a DB-backed accumulation test. `rates_for`
matches model-name substrings (`opus`/`haiku`/else→`sonnet`); `cost_usd` converts
input/cached/output token counts to USD; `add_usage` writes into `cost_daily` and
sums same-day rows; `is_over_cap` gates the daily cap.

```rust
#[test]
fn cost_arithmetic_sonnet() {
    let r = rates_for("claude-sonnet-4-6");
    assert!((cost_usd(r, 1_000_000, 0, 0) - 3.0).abs() < 1e-9);   // $3 / Mtok input
    assert!((cost_usd(r, 0, 0, 1_000_000) - 15.0).abs() < 1e-9);  // $15 / Mtok output
    assert!((cost_usd(r, 0, 1_000_000, 0) - 0.30).abs() < 1e-9);  // $0.30 / Mtok cached
}

#[test]
fn rates_pick_correct_model() {
    assert_eq!(rates_for("claude-haiku-4-5-20251001").input_per_mtok, 1.0);
    assert_eq!(rates_for("claude-opus-4-7").input_per_mtok, 15.0);
    assert_eq!(rates_for("claude-sonnet-4-6").input_per_mtok, 3.0);
}
```

The `add_usage_accumulates_same_day` test opens a fresh `tempfile` DB, posts two
usages, and asserts the row totals — this verifies the persisted accounting, not
just the in-memory math.

**`predictions.rs`** — `insert_then_pending_due` constructs an `AnalysisJson`,
calls `insert_from_analysis(&db, "BTCUSDT", &a)`, asserts the row is **not** yet
due, then rewrites `ts` back 7200s with raw SQL and asserts `pending_due` now
returns one row. This is the self-calibration loop's gate, tested without waiting
in wall-clock time. (Note: it tests *due-ness*, not resolution — see
[Coverage gaps](#coverage-gaps-and-recommended-additions).)

**`paths.rs`** — `ensures_all_dirs` points `Paths::new` at a `tempfile::tempdir()`
and asserts `library()`, `screenshots()`, `data()`, and `logs()` all exist after
`ensure_all()`. (Reminder: `paths.lancedb()` is a misnomer — it returns
`data/vec.db`, a plain SQLite file, not a LanceDB directory.)

**`settings.rs`** — `defaults_then_save_load_roundtrip` confirms the default
`daily_cost_cap_usd == 2.0`, mutates a clone, `save`s, reloads, and re-reads. Note
this exercises only the non-secret settings table; the Anthropic API key lives in
the OS keyring (Windows Credential Manager) and is intentionally **not** covered
by an automated test (keyring access is environment-bound). The key is stored
under service `cognitrade`, account `anthropic_api_key` (the `KEYRING_SERVICE` /
`KEYRING_USER` constants in `settings.rs`), so a developer can manually inspect or
clear it in Credential Manager when reasoning about the deliberately-untested
keyring path.

**`db.rs`** — `opens_and_migrates` opens a temp `chat.db` and counts the tables
created by `migrations/001_initial.sql`, asserting all four
(`messages`, `predictions`, `cost_daily`, `settings`) exist.

**`prompt.rs`** — three tests assert the Anthropic request shape: an image + a
non-empty retrieved-knowledge chunk list produces a 3-part user content array
(cached text block, image, user text), an empty chunk list omits the cached
block, and prior history is emitted *before* the new user turn. The cached block
carries `cache_control.type == "ephemeral"`.

**`coingecko.rs`** — `symbol_normalization` verifies suffix stripping
(`BTCUSDT`/`ETHUSDC` → `bitcoin`/`ethereum`) and that an unknown ticker maps to
`None`. The id table is hardcoded (~12 coins: BTC, ETH, SOL, BNB, XRP, ADA, DOGE,
AVAX, MATIC, LINK, DOT, LTC); unlisted symbols error at call time.

**`parse.rs`** — `extract_json_tail` tests cover the happy path, missing fence
(→ `None`), an out-of-range `probability_up` rejected by `validate()`, and
*picking the last* `` ```json `` fence when several are present. This is the
function that converts the model's trailing fenced block into `AnalysisJson`.

**`streaming.rs`** — `parse_event` tests cover `content_block_delta` text deltas,
`message_start` usage accounting (note `cache_creation_input_tokens` is folded
into `input_tokens`; `cache_read_input_tokens` becomes `cached_input_tokens`),
`message_stop` → `Done`, and unknown events → `None`.

---

## Backend integration tests — `src-tauri/tests/`

Integration tests compile against the library crate `cognitrade_lib` (the `[lib]`
`name` in `Cargo.toml`) and exercise multiple modules end to end.

> **Package vs. library name.** The Cargo `[package]` name is **`cognitrade-app`**
> (with a hyphen); the `[lib]` target is **`cognitrade_lib`** (with an
> underscore). Use the *package* name for `cargo`'s `-p` flag and builds
> (`cargo build -p cognitrade-app`); use the *library* name only inside
> `use cognitrade_lib::...` paths in the integration tests. `cargo test -p
> cognitrade_lib` will fail because no package by that name exists.

### `llm_integration.rs` — mocked Anthropic streaming

This file's only test, `analyze_streams_text_and_parses_json`, runs by
default (it is **not** `#[ignore]`d). It uses `wiremock` to stand up a fake
`/v1/messages` endpoint returning a canned `text/event-stream` body, points the
client at it via `LlmClient::with_base(server.uri())`, and asserts:

- streamed text is forwarded (`out.full_text.contains("Looks bullish")`),
- the trailing JSON tail parses (`out.analysis.expect(...)`, `probability_up ≈ 0.7`),
- usage tokens are read off the SSE (`cached_input_tokens == 50`, `output_tokens == 80`).

This is the closest thing to an end-to-end backend test that needs no network or
model download — it covers prompt build → SSE parse → text forwarding → JSON-tail
parse → usage accounting in one pass. Because it is self-contained and mocked,
there is **no** live-API variant to opt into; the `--ignored` flag finds nothing
in this file.

### `ingest_integration.rs` — real embeddings (the only ignored test)

`ingest_pdf_into_lancedb` is **`#[ignore]`d** because it downloads the BGE-small
embedding model (`fastembed` `BGESmallENV15`) on first run and embeds for real. It
opens a temp `vec.db` via `lance::open`, ingests `tests/fixtures/sample.pdf`, and
asserts both that chunks were produced and that `retrieval::search(&store, "hello", 3)`
returns hits (brute-force cosine over every stored row). This is the **single**
opt-in `#[ignore]`d test in the repository.

### Running the ignored test

The one `#[ignore]`d test is slow and environment-dependent (model download,
CPU-only synchronous embedding behind a global mutex). Run it explicitly:

```powershell
# cwd = src-tauri

# all ignored integration tests (currently just the ingest smoke test), with live output
cargo test -- --ignored --nocapture

# the ingest smoke test by name
cargo test --test ingest_integration -- --ignored --nocapture
```

The first run will download BGE-small into the model cache; expect it to be slow
and to require network access. On the 8GB-RAM hardware target, run this one at a
time — embedding is CPU-only and serialized behind a `OnceLock<Mutex<TextEmbedding>>`
singleton.

> There is no `cargo test --test llm_integration -- --ignored` to run: that file
> has no `#[ignore]`d test (see the cross-reference note near the top). A live-API
> Anthropic integration test is **Planned — not yet implemented**.

---

## Test doubles

CogniTrade keeps fakes minimal and prefers real components where they are cheap:

| Dependency | Double | How |
|------------|--------|-----|
| SQLite (`chat.db`, `vec.db`) | **Real DB on a temp dir** | `tempfile::tempdir()` → `db::open(...)` / `lance::open(...)`. No SQLite mocking; tests run actual migrations and SQL. |
| Anthropic HTTP API | **`wiremock` mock server** | `MockServer::start()`, mount a `/v1/messages` responder, point the client at it with `LlmClient::with_base(server.uri())`. Used to fake the SSE stream without spending tokens. |
| CoinGecko HTTP API | **`wiremock` (Planned for live calls)** | `coingecko.rs` is unit-tested at the pure `symbol_to_id` level only. `get_usd_price` builds a real `reqwest` client against a hardcoded `BASE` and is **not yet** wiremock-injectable — see gaps below. |
| Embedding model | **Real model (ignored test only)** | No fake embeddings; the only embedding-backed test is the `#[ignore]`d `ingest_integration`. |
| Filesystem / library dir | **Temp dirs** | `tempfile` for paths, screenshots, and ingest fixtures. |

There is **no** standing fixture for the OS keyring; API-key retrieval is left
out of automated coverage on purpose. To inspect the stored key manually, open
Windows Credential Manager and look under service `cognitrade`, account
`anthropic_api_key`.

---

## Frontend testing

### Type checking (the only automated frontend gate today)

The SvelteKit/TypeScript frontend has a working static check via `svelte-check`,
wired into `package.json` as the `check` script:

```powershell
npm install        # once after clone
npm run check      # svelte-kit sync && svelte-check --tsconfig ./tsconfig.json
```

`npm run check` validates types across `src/` — the IPC wrappers (`src/lib/ipc.ts`),
shared types (`src/lib/types.ts`), the chat store and its event listeners
(`src/lib/stores/chat.ts`, which subscribes to the `analysis_chunk` /
`analysis_done` Tauri events), the 3-tab shell (`src/routes/+page.svelte`), and the
components under `src/lib/components/`. There is a watch variant, `npm run check:watch`.

This catches type drift between the Rust IPC surface and the TypeScript wrappers,
but it does **not** execute any behaviour.

### Unit / component / e2e tests

**Planned — not yet implemented.** There is currently **no** Vitest,
Testing-Library, Playwright, or any other frontend test runner configured.
`package.json` declares no `test` script, and there are no `*.test.ts`,
`*.spec.ts`, or component test files. The store's event-handling logic
(`analysis_chunk` accumulation, `analysis_done` finalization) is therefore covered
only indirectly, by manual use of the running app.

---

## Coverage gaps and recommended additions

The current suite is strong on **pure functions and DB-backed units** and has one
solid mocked-HTTP integration test. The notable gaps:

### 1. End-to-end / IPC-level coverage (Planned)

`ipc::analyze` — the single command that drives the whole flow (persist message →
resolve symbol → RAG retrieve → build request → stream → parse JSON tail → store
prediction + price → store assistant message → emit `analysis_done`) — has **no**
direct test. The mocked `llm_integration` test covers the LLM client in isolation,
but the orchestration in `ipc.rs` is untested.

- **Recommended:** an end-to-end smoke test through the Tauri shell using
  **`tauri-driver` + a WebDriver** (e.g. `WebDriverIO`/`selenium`) to drive the
  real chat tab against a wiremock'd Anthropic endpoint, asserting that
  `analysis_chunk` / `analysis_done` events reach the frontend and a prediction
  row is written.

### 2. SSE parser robustness (Planned)

`streaming::parse_event` is tested on well-formed events only. It currently
swallows malformed JSON (`serde_from_str(...).ok()?` → `None`), but there is no
test for split/partial SSE frames, multi-line `data:` payloads, interleaved
`ping` events under load, or truncated streams.

- **Recommended:** **fuzz** `parse_event` (and the line-framing layer that feeds
  it) with `cargo-fuzz`/`arbitrary` over arbitrary byte streams, asserting it
  never panics and only ever yields well-typed events.

### 3. JSON-tail robustness (Planned)

`parse::extract_json_tail` handles the clean cases (last fence wins, invalid
probability rejected). Untested edge cases that occur with real model output:
unterminated fences, nested/escaped triple-backticks inside the JSON, prose that
*looks* like a fence, trailing commas, and a fence whose body validates
structurally but is semantically wrong (e.g. `take_profit_pct < stop_loss_pct`).

- **Recommended:** a table-driven test (or **fuzz** harness) over realistic
  "dirty" model transcripts, plus an explicit `validate()` test matrix for the
  numeric invariants.

### 4. CoinGecko live path (Planned)

Only `symbol_to_id` is tested. `coingecko::get_usd_price` hits a hardcoded `BASE`
URL and is not injectable, so the HTTP/JSON path, non-2xx handling, and the
"price missing in response" branch are uncovered.

- **Recommended:** parameterize `BASE` (mirroring `LlmClient::with_base`) and add
  **wiremock** tests for success, HTTP error, and malformed-body cases. This is
  the prerequisite for testing the resolution loop (gap #5).

### 5. Prediction resolution loop (Planned)

`predictions.rs` tests cover *insertion and due-ness* but not `resolve_one`, which
polls CoinGecko for price-at-horizon and records the outcome. The background
resolver (`prune::spawn_background`, the 60s loop) and the chat-prune loop (7-day
cutoff, every 6h) are untested as loops.

`resolve_one`'s only injectable seam is its price source, `coingecko::get_usd_price`,
which is currently hardcoded to `BASE`. **Gaps #4 and #5 therefore share one fix:**
parameterizing `BASE` (as `LlmClient::with_base` does) is what makes
`get_usd_price` stubbable, and only then can `resolve_one` be driven with a
deterministic price.

- **Recommended:** once `get_usd_price` is injectable, drive `resolve_one` with a
  stub price and assert `outcome_status` transitions from `pending`. (Reminder:
  resolution records price-at-horizon vs price-at-prediction only — it does
  **not** simulate intra-horizon SL/TP hits; tests should encode that limitation,
  not assume SL/TP enforcement.)

### 6. Vector store at scale (Planned)

`lance::search` loads **every** `doc_chunks` row and computes cosine in Rust per
query. There is no test for the 384-dim (`EMBED_DIM`) validation-on-insert path
against mismatched vectors, and no benchmark guarding the brute-force search
against the 8GB-RAM ceiling as the corpus grows.

- **Recommended:** a unit test feeding a wrong-dimension embedding (expect an
  insert error) and a `criterion` benchmark on `search` over a realistic row count.

### 7. Frontend behaviour (Planned)

As noted above — no runtime frontend tests exist. Priority targets: the chat store
event accumulation (`analysis_chunk` → progressive render, `analysis_done` →
finalize), IPC error surfacing (`AppError` variants such as `CostCapReached`,
`NoApiKey` rendering sensibly), and the Library drag-and-drop ingest UX.

---

## Quick reference

```powershell
# Backend: all default tests (unit + mocked integration) — cwd = src-tauri
cd src-tauri
cargo test

# Backend: one module's inline tests
cargo test cost::tests
cargo test parse::tests

# Backend: opt-in network/model test (the ONLY #[ignore]'d test in the tree)
cargo test -- --ignored --nocapture
cargo test --test ingest_integration -- --ignored --nocapture

# Frontend: type check (the only automated frontend gate today)
npm run check
```

> **Note.** `llm_integration.rs` has no `#[ignore]`d test, so the `CLAUDE.md`
> command `cargo test --test llm_integration -- --ignored` runs nothing against
> today's tree. Its one test runs by default via plain `cargo test`.

> The repository is **not** a git repository. There is no CI configuration in the
> tree; the commands above are the full, current verification path. Wiring
> `cargo test` + `npm run check` into CI is itself **Planned — not yet implemented**.
