// Stop hook: saves what claude-mem recorded in this session into
// .claude/memory/auto/, so it outlives the temporary cloud container. The
// SessionStart hook (claude-mem-memory.mjs) imports those files again in later
// sessions.
//
// It runs after every reply and rewrites this session's file only when its
// content changed. claude-mem writes observations in the background, so the
// newest ones may only appear after the next reply. It never fails the
// session: errors go to stderr and the hook exits 0.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const MAX_FILE_BYTES = 60 * 1024; // claude-mem imports files up to 64 KB

try {
  const input = JSON.parse(fs.readFileSync(0, "utf8") || "{}");
  const sessionId = input.session_id;
  const projectDir = process.env.CLAUDE_PROJECT_DIR || input.cwd || process.cwd();
  const dataDir = process.env.CLAUDE_MEM_DATA_DIR || path.join(os.homedir(), ".claude-mem");
  const dbPath = path.join(dataDir, "claude-mem.db");
  if (!sessionId || !fs.existsSync(dbPath)) process.exit(0);

  const { DatabaseSync } = await import("node:sqlite");
  const db = new DatabaseSync(dbPath, { readOnly: true });
  const session = db
    .prepare("SELECT memory_session_id, started_at FROM sdk_sessions WHERE content_session_id = ?")
    .get(sessionId);
  if (!session?.memory_session_id) process.exit(0);

  const observations = db
    .prepare(
      `SELECT type, title, subtitle, narrative, facts, files_modified, created_at
         FROM observations WHERE memory_session_id = ? ORDER BY created_at_epoch, id`,
    )
    .all(session.memory_session_id);
  const summaries = db
    .prepare(
      `SELECT request, completed, learned, next_steps, created_at
         FROM session_summaries WHERE memory_session_id = ? ORDER BY created_at_epoch, id`,
    )
    .all(session.memory_session_id);
  db.close();
  if (observations.length === 0 && summaries.length === 0) process.exit(0);

  const day = String(session.started_at || new Date().toISOString()).slice(0, 10);
  const base = `${day}-${sessionId.slice(0, 8)}`;
  const header = [
    `# Session ${day} (${sessionId.slice(0, 8)}), recorded by claude-mem`,
    "",
    "Written automatically by .claude/hooks/claude-mem-export.mjs. These are",
    "claude-mem's own notes on the session, not reviewed by hand.",
    "",
  ].join("\n");

  const sections = [];
  for (const s of summaries) {
    sections.push(
      [
        `## Summary (${s.created_at})`,
        field("Request", s.request),
        field("Completed", s.completed),
        field("Learned", s.learned),
        field("Next steps", s.next_steps),
      ].filter(Boolean).join("\n"),
    );
  }
  for (const o of observations) {
    const facts = list(o.facts);
    const files = list(o.files_modified);
    sections.push(
      [
        `## ${o.title || "(untitled)"}`,
        `_${o.type || "note"} · ${o.created_at}_`,
        o.subtitle ? `\n${o.subtitle}` : "",
        o.narrative ? `\n${o.narrative}` : "",
        facts.length ? `\n${facts.map((f) => `- ${f}`).join("\n")}` : "",
        files.length ? `\nFiles changed: ${files.join(", ")}` : "",
      ].filter(Boolean).join("\n"),
    );
  }

  // Split into files that claude-mem will still import.
  const parts = [];
  let current = header;
  for (const s of sections.map(redact)) {
    if (Buffer.byteLength(current + "\n" + s + "\n") > MAX_FILE_BYTES && current !== header) {
      parts.push(current);
      current = header;
    }
    current += "\n" + s.slice(0, MAX_FILE_BYTES - 2048) + "\n";
  }
  parts.push(current);

  const outDir = path.join(projectDir, ".claude", "memory", "auto");
  fs.mkdirSync(outDir, { recursive: true });
  parts.forEach((text, i) => {
    const file = path.join(outDir, `${base}${i ? `-part${i + 1}` : ""}.md`);
    const old = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : null;
    if (old !== text) fs.writeFileSync(file, text);
  });
} catch (err) {
  console.error(`claude-mem export hook: ${err instanceof Error ? err.message : String(err)}`);
}
process.exit(0);

function field(label, value) {
  return value && String(value).trim() ? `- **${label}:** ${String(value).trim()}` : "";
}

function list(value) {
  if (!value) return [];
  try {
    const parsed = JSON.parse(value);
    return Array.isArray(parsed) ? parsed.map(String) : [String(parsed)];
  } catch {
    return [String(value)];
  }
}

// Remove things that look like secrets before the text is committed.
function redact(text) {
  return text
    .replace(/\b(gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b/g, "[redacted token]")
    .replace(/\b(sk-[A-Za-z0-9_-]{20,}|xox[abpr]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16})\b/g, "[redacted key]")
    .replace(/(Bearer\s+)[A-Za-z0-9._~+/=-]{20,}/gi, "$1[redacted]")
    .replace(/-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g, "[redacted private key]")
    .replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, (m) => (m.endsWith("anthropic.com") ? m : "[email]"));
}
