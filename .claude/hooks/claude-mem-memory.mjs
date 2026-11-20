// SessionStart hook: makes claude-mem import .claude/memory/*.md as searchable
// memory for this project. Cloud sessions start with an empty claude-mem
// database, so the notes must be re-imported in every new container.
//
// claude-mem's worker does not see environment variables from Claude's
// settings, so this writes CLAUDE_MEM_MEMORY_WATCH_ROOTS into
// ~/.claude-mem/settings.json and restarts the worker if it is already
// running with other settings. It never fails the session: any error is
// printed to stderr and the hook exits 0.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";

try {
  const projectDir = process.env.CLAUDE_PROJECT_DIR || process.cwd();
  const memoryDir = path.join(projectDir, ".claude", "memory");
  if (!fs.existsSync(memoryDir)) process.exit(0);

  const dataDir = process.env.CLAUDE_MEM_DATA_DIR || path.join(os.homedir(), ".claude-mem");
  const settingsPath = path.join(dataDir, "settings.json");
  const roots = JSON.stringify([{ path: memoryDir, project: "Kladde" }]);

  let settings = {};
  if (fs.existsSync(settingsPath)) settings = JSON.parse(fs.readFileSync(settingsPath, "utf8"));
  // claude-mem accepts a flat file or one nested under "env".
  const target = settings.env && typeof settings.env === "object" ? settings.env : settings;
  if (target.CLAUDE_MEM_MEMORY_WATCH_ROOTS === roots) process.exit(0);

  target.CLAUDE_MEM_MEMORY_WATCH_ROOTS = roots;
  fs.mkdirSync(dataDir, { recursive: true });
  fs.writeFileSync(settingsPath, JSON.stringify(settings, null, 2) + "\n", { mode: 0o600 });

  // Restart a worker that is already running so it picks up the watcher.
  // If none is running, claude-mem starts it later with the new settings.
  const pidFile = path.join(dataDir, "worker.pid");
  let running = false;
  try {
    const pid = Number(fs.readFileSync(pidFile, "utf8").trim().match(/\d+/)?.[0]);
    if (pid) { process.kill(pid, 0); running = true; }
  } catch {}
  if (!running) process.exit(0);

  const pluginRoots = [path.join(os.homedir(), ".claude", "plugins")];
  const worker = findWorker(pluginRoots);
  const bun = [path.join(os.homedir(), ".bun", "bin", "bun"), "bun"].find(
    (b) => !b.includes(path.sep) || fs.existsSync(b),
  );
  if (worker && bun) {
    const child = spawn(bun, ["--no-install", worker, "restart"], { detached: true, stdio: "ignore" });
    child.unref();
  }
} catch (err) {
  console.error(`claude-mem memory hook: ${err instanceof Error ? err.message : String(err)}`);
}
process.exit(0);

function findWorker(roots) {
  const stack = roots.map((r) => [r, 0]);
  while (stack.length) {
    const [dir, depth] = stack.pop();
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { continue; }
    for (const e of entries) {
      const p = path.join(dir, e.name);
      if (e.isFile() && e.name === "worker-service.cjs" && dir.endsWith(path.join("claude-mem", "scripts"))) return p;
      if (e.isDirectory() && depth < 6 && e.name !== "node_modules") stack.push([p, depth + 1]);
    }
  }
  return null;
}
