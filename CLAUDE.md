# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Kladde is a Tauri 2 desktop study app (notes, homework, weekly study plan) for Danish upper-secondary students, shipped for Windows x64 and Apple Silicon Macs. The frontend is React 19 + Tailwind 4 + TanStack Query (`src/`). The backend is Rust (`src-tauri/src/`). The UI is bilingual Danish/English and defaults to Danish.

## Memory between sessions

Notes from earlier sessions are in `.claude/memory/`: session histories, decisions and pitfalls, and how the owner likes to work. A SessionStart hook (`.claude/hooks/claude-mem-memory.mjs`) makes claude-mem import them as searchable memory, and a Stop hook (`.claude/hooks/claude-mem-export.mjs`) saves each session's claude-mem observations to `.claude/memory/auto/`. Search them before relearning something. Commit the `auto/` files with the session's work. At the end of a session that changed something worth remembering, add or update a note there and commit it (see `.claude/memory/README.md`).

## Commands

| Command | Purpose |
| --- | --- |
| `npm ci` | Install frontend packages |
| `npm run app` | Run the app in dev mode (Vite on port 1420 + `tauri dev`) |
| `npm run typecheck` | `tsc -b` (TypeScript 7; the `@/` alias maps to `./src/*`) |
| `npm run build:web` | Typecheck and build the frontend into `dist/` |
| `npm test` | All Rust unit tests (`cargo test --manifest-path src-tauri/Cargo.toml`) |
| `cargo test --manifest-path src-tauri/Cargo.toml <name>` | One test or module, e.g. `feeds::` or `subject` |
| `npm run test:desktop` | Builds the debug app and drives the real UI end to end (see below). On Linux: `xvfb-run -a npm run test:desktop` |
| `npm run dist:win` / `npm run dist:mac` | Local installers. `dist:mac` first runs `scripts/prepare-mac.sh`, which downloads the pinned, hash-checked Codex binary |

- **Linux:** building needs the Tauri WebKitGTK packages (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `libsoup-3.0-dev`).
- **Expected dead-code warnings:** on Linux, `claude.rs` (`rpc`, `stdio`, …) warns as dead code because it is macOS-only.
- **CI audits:** CI also runs `npm audit --audit-level=low` and `cargo audit --file src-tauri/Cargo.lock`, and both must pass.
- **Ignored test:** `bundled_server_starts_without_credentials` only runs with `KLADDE_TEST_CODEX=<path to bundled codex> cargo test … -- --ignored`.

**Desktop test** (`scripts/test-desktop.mjs` + `scripts/test-desktop-ui.mjs`):
- It starts Vite with a plugin that injects `test-desktop-ui.mjs` into the main window.
- It serves a fake school page and launches `src-tauri/target/debug/kladde` with an isolated `KLADDE_DATA_DIR`.
- It runs twice: a first launch, and a reopen to check that data persisted.
- Assertions on `kladde.json` live in `test-desktop.mjs`. UI steps use `data-testid` selectors.
- `KLADDE_SCHOOL_TEST_ORIGIN` (debug builds only, loopback only) points the school window at the fake page.

## Architecture

### Frontend → backend calls
- The frontend still calls REST-style paths (`apiGet("/dashboard")`, `apiPatch("/notes/:id")`). `src/lib/api.ts` maps each one to a Tauri command through a route table and adds `lang` (from `localStorage["kladde-language"]`) to every call.
- A new command needs three things:
  - a `#[tauri::command]` in Rust;
  - an entry in the `generate_handler!` list in `src-tauri/src/lib.rs`;
  - a route in `api.ts`.
- `lib.rs` wraps the dispatcher so commands are rejected unless they come from the window labelled `main`.
- Commands return `Res<T> = Result<T, ApiErr { status, detail }>`. The frontend turns these into `ApiError`, and `errorDetail()` reads the message.
  - User-facing text comes from the backend through `tr(&lang, da, en)`.
  - The frontend uses `text[lang]` and `da ? … : …`.
- `src/lib/queryClient.ts` has a global `MutationCache.onError` that shows a toast for any mutation without its own `onError`. Mutations that show their error inline set `meta: { errorShownInline: true }`.
- Nearly all UI is in `src/pages/Home.tsx`, as views switched by `active`: overview, notes, homework, planner, ai, imports. `ImportCenter` stays mounted (hidden) so school-window results are not lost.

### Data file (`store.rs`)
- All data lives in one JSON file, `kladde.json`, in the app data folder or `KLADDE_DATA_DIR`. It is held as `Mutex<Store>` in `AppState`.
- **Writing data:** take the lock, call `store.transaction()` (a cloned working copy), change `tx.data`, then `tx.commit()`. The commit writes the file atomically (temp file + rename) and only then replaces the live data. A failed write leaves memory and disk unchanged.
- **Unknown fields are preserved:** every record type has `#[serde(flatten)] extra`, and so does `Data`. Don't drop or rename fields; the format must stay readable by older and newer versions.
- **Opening the file:**
  - `tidy()` repairs small type mistakes.
  - Duplicate or missing IDs, invalid values, or a newer `version` make startup fail. The file is never overwritten in that case.
- **Records:**
  - Notes and tasks store both `subject` (name) and `subject_key` (`subject_key(name)`, lowercase).
  - Blocks and chat messages store only the subject name. So renaming or deleting a subject (`rename_in_data`, `delete_in_data` in `commands.rs`) must update all four.
- **Blocks:**
  - `day` is a Danish weekday name.
  - `source` is `manual`, `import`, `plan`, `feed:<feed id>`, or null for old data.
  - Study-plan blocks have `scheduled_on` and `external_id = <task id>`.
- **External IDs:**
  - Imports use `importers::ext_id` (first 16 hex digits of sha1 of the parts joined with `|`). This must stay byte-compatible with the earlier Node version, so re-imports are detected as duplicates.
  - Calendar tasks use `feed:<id>:event:<hash>`. `feeds::apply_events` migrates older ID formats.

### Threads and blocking
- Synchronous commands run on the main thread, so keep them short.
- Network and AI work runs in `async` commands through `tauri::async_runtime::spawn_blocking` (the `blocking` helper in `ai.rs`).
- Never hold the store lock during network calls. Read what you need, release the lock, fetch, then lock again and apply (see `feeds::sync_feed_blocking` and `commands::add_feed`).
- `feeds::start_auto_sync` re-checks calendar links every 3 hours on a background thread.

### Network surfaces and their safeguards
- **Calendar links (`feeds.rs`):**
  - Links are treated as secrets: `feed_out` removes `url` before data goes to the frontend, and backups clear it.
  - Downloads use ureq 3 with HTTPS only, no system proxy, manual redirects (5 at most), a 20 s deadline and an 8 MB limit.
  - A custom `PublicResolver` only lets connections reach public addresses (`public_address`). It uses ureq's `unversioned` API, which isn't covered by semver, so check it when updating ureq.
- **AI (`ai.rs`):**
  - Cloudflare Workers AI (`@cf/openai/gpt-oss-20b`). The Cloudflare token is stored in the OS keyring via the `keyring` crate.
  - On macOS, ChatGPT runs through the bundled Codex app server over stdio (`codex_client.rs`).
  - `generate()` picks the provider. Structured output passes a JSON schema.
- **Claude Desktop (macOS, `claude.rs`):** the same binary started with `--kladde-mcp <dir>` is a stdio MCP server. It exposes `read_shared_item` and `suggest_draft`. Drafts go to an inbox folder, and only the open app writes the study store.
- **Updater:**
  - Updates are signed. `tauri.conf.json` holds the public key that installed copies trust.
  - Never change that key or the endpoint.

### School window (`school.rs`)
- "Åbn Mit Niels Brock / Ludus" opens a second window labelled `school`:
  - It is private (incognito).
  - It has no capabilities, and its commands are rejected by the label check.
- **Allowed sites:** `site_of()` lists the school sites (`www.mitnielsbrock.dk`, `nielsbrock.luduseg.dk`).
- **How a page reaches Kladde:**
  1. An initialization script adds a "Hent afleveringer" button and auto-detects deadline tables.
  2. It hands the page over by navigating to `https://kladde-extract.invalid/<nonce>?d=<json>`.
  3. `on_navigation` intercepts and cancels that navigation, then checks that the window's real address is a school site.
  4. `handle_page` parses the tables (`importers::parse_import`), or asks the AI after a confirmation dialog.
  5. It emits `school-import` / `school-import-error` to the main window.
- **Navigation filter:** on macOS and Linux, wry also passes the navigations of embedded frames to `on_navigation`, which is why `about:blank` and `about:srcdoc` are allowed. Login redirects go through `login.microsoftonline.com` and `adfs.nielsbrock.dk`.
- **Never re-enable Tauri's global `freezePrototype`:** it is injected into every webview, including the school window, and breaks Microsoft's sign-in page. The freeze is added only to the main window, as an initialization script in `lib.rs::create_window`. The desktop test checks both sides.

### Main window
- **CSP:** `tauri.conf.json` sets a strict CSP (`connect-src` is IPC only).
- **Outside links:** `src/lib/outsideLinks.ts` intercepts clicks on outside links and `window.open`, and sends them to the `open_url` command. `on_navigation` / `on_new_window` refuse anything that isn't internal.
- **Capabilities:** `src-tauri/capabilities/default.json` only grants event listening, resource close, the updater and restart.

## CI and releases (`.github/workflows/build-windows.yml`)
- Pull requests build and test on `windows-latest` and `macos-15`:
  - `cargo test`;
  - the desktop test;
  - installers uploaded as artifacts;
  - dependency audits;
  - CodeQL (`codeql.yml`).
- Every push to `main` builds version `1.3.<run number>`. The version is rewritten in CI, and `tauri.conf.json` keeps `1.3.0`.
- The separate `release` job then runs in the `release-signing` environment (main only):
  1. It signs with `KLADDE_UPDATE_SIGNING_KEY`.
  2. It checks the signatures with `minisign` against the public key in `tauri.conf.json`.
  3. It writes `latest.json` and publishes `kladde-build-<n>` as the latest release. Installed apps update from that release.
- The `release` job never runs on pull requests, so changes to it (e.g. `download-artifact`) are only exercised after merging.
- Actions are pinned by commit SHA. The `github/codeql-action` `init` and `analyze` steps must use the same version; Dependabot groups them.
