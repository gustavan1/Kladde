# Kladde memory notes

These Markdown files are Kladde's long-term memory for Claude Code sessions.

Cloud sessions run in a temporary container, so claude-mem's own database is
empty at the start of each session and is deleted at the end. To keep memory
without paying for cloud sync, a SessionStart hook
(`.claude/hooks/claude-mem-memory.mjs`) sets claude-mem's
`CLAUDE_MEM_MEMORY_WATCH_ROOTS` to this folder. claude-mem then imports every
`.md` file here (up to 64 KB each) as a searchable memory in the `Kladde`
project, and re-imports a file when it changes.

## Automatic notes (`auto/`)

claude-mem records observations on its own during every session, but they
live in its database and are lost with the container. A UserPromptSubmit hook
(`.claude/hooks/claude-mem-export.mjs`) runs when the owner sends a message.
Once at least 5 new observations have built up, it writes the current
session's observations and summaries to `auto/<date>-<session>.md` (split into
`-partN` files above 60 KB). Simple
secret patterns (tokens, keys, private keys, e-mail addresses) are redacted
first. Commit these files with the session's work.

They are claude-mem's unreviewed notes and can contain wrong guesses. The
hand-written notes below take precedence.

## Keeping it up to date

At the end of a session that changed something worth remembering:

- Add `history-YYYY-MM-DD.md` for the session: what was asked, what changed
  (PRs, releases), and anything left open.
- Put lasting rules, decisions and pitfalls in `decisions-and-pitfalls.md`, and
  how the owner likes to work in `working-with-the-owner.md`.
- Commit the notes with the rest of the session's work.

Never put secrets, tokens, keys or students' personal data in these files.

## Files

| File | Contents |
| --- | --- |
| `history-2026-11-18.md` | Session 1 (Nov 18–20): Tauri port, AI, Mac build, updater, security, school window (PRs #1–#13) |
| `history-2026-11-19.md` | Session 2 (Nov 19–20): signing key move, Dependabot fixes, school login, subject deletion, CLAUDE.md (PRs #14–#18) |
| `history-2026-11-20.md` | Session 3 (Nov 20): claude-mem set up with this folder |
| `decisions-and-pitfalls.md` | Rules and lessons that should not be relearned |
| `working-with-the-owner.md` | How the owner works and likes to be answered |
