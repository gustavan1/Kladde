# Kladde: decisions and pitfalls

Lessons from earlier sessions. Each one cost real time to find.

## Things that broke and why

- **Do not turn on Tauri's global `freezePrototype`.**
  - It runs in every webview, including the school window, and Microsoft's
    sign-in page (used by Mit Niels Brock) then shows an empty white box.
  - The freeze is added only to the main window, through an initialization
    script in `lib.rs::create_window`.
  - The desktop test checks both sides.
- **Keep `about:blank` and `about:srcdoc` allowed in the school window.** On
  macOS and Linux, wry also passes embedded-frame navigations to
  `on_navigation`. Ludus embeds an empty frame, and blocking it can break the
  page.
- **CodeQL `init` and `analyze` must use the same version,** or analysis stops
  with "Loaded a configuration file for version X, but running version Y".
  Dependabot groups them so they move together.
- **Release signing:**
  - The key is `KLADDE_UPDATE_SIGNING_KEY` (plus its password if set). It
    lives only in the main-only `release-signing` environment.
  - The old repository secret `TAURI_SIGNING_PRIVATE_KEY` was deleted.
  - Never change the updater public key or endpoint in `tauri.conf.json`:
    installed copies trust that key.
  - The `release` job never runs on PRs, so changes to it are only tested
    after merging.
- **ureq 3:** `PublicResolver` uses ureq's `unversioned` API, which semver
  doesn't cover. Re-check it on every ureq update. Cloudflare error bodies are
  read with status errors turned off, so quota messages still reach the user.
- **TypeScript 7 has no `baseUrl`.** The `@/` alias is a relative path in
  `tsconfig.json`.
- **Deadline keywords:** "due" must match as a whole word, or lessons like
  "Individuel vejledning" become homework.
- **Dates:** use the local date, not UTC, for "today" fallbacks. Students are
  in Europe/Copenhagen.

## Decisions

- **AI:** Cloudflare Workers AI (`@cf/openai/gpt-oss-20b`) on both platforms,
  with the token in the OS keyring. On Mac, ChatGPT runs through the bundled
  Codex app server. Gemini was the first choice and was dropped.
- **School window:** private, with no capabilities. Pages are read only on the
  two school sites, and nothing is saved before the student confirms. The AI
  reads a page only after a click and a confirmation.
- **Calendar links** are secrets: they are removed before data reaches the
  frontend and cleared in backups.
- **Data compatibility:** `kladde.json` keeps unknown fields (`serde(flatten)
  extra`). `importers::ext_id` must stay byte-compatible with the old Node
  version.
- **Subjects:** renaming or deleting one updates notes, tasks, blocks and chat
  messages in one transaction. The last subject can't be deleted.
- **Dependabot PRs that fail to build:** fix them in a Claude branch, then
  close the Dependabot PR.
