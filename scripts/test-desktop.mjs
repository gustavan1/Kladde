// Launch the real debug application against a local school fixture. The extra Vite
// entry point exists only in this test server. Production builds use vite.config.ts.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer as httpServer } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createServer as viteServer } from "vite";

const dir = await mkdtemp(join(tmpdir(), "kladde-desktop-"));
let showTable = false;
let pageRendered = false;
const school = httpServer((req, res) => {
  if (req.url === "/ready") pageRendered = true;
  if (req.url === "/mitnielsbrock.html") {
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end(`<!doctype html><title>Test school</title><h1>School login fixture</h1><main></main>
      <script>
        requestAnimationFrame(() => fetch("/ready"));
        const poll = setInterval(async () => {
          if (await (await fetch("/table", {cache:"no-store"})).text() !== "yes") return;
          clearInterval(poll);
          document.querySelector("main").innerHTML = "<table><tr><th>Opgave</th><th>Fag</th><th>Frist</th></tr><tr><td>Fixture worksheet</td><td>Dansk</td><td>2099-11-25 19:15</td></tr></table>";
        }, 200);
      </script>`);
  } else {
    res.end(req.url === "/table" && showTable ? "yes" : "");
  }
});

let complete;
const result = new Promise(resolveResult => { complete = resolveResult; });
const vite = await viteServer({ plugins: [{
  name: "kladde-desktop-regression",
  transformIndexHtml: () => [{ tag: "script", attrs: { type: "module", src: "/scripts/test-desktop-ui.mjs" }, injectTo: "body" }],
  configureServer(server) {
    server.middlewares.use("/__test/show-table", (_req, res) => { showTable = true; res.end(); });
    server.middlewares.use("/__test/result", async (req, res) => {
      let message = "";
      for await (const chunk of req) message += chunk;
      res.end("received");
      complete(message);
    });
  },
}] });
let child;
let closed;
let timer;
try {
  school.listen(0, "127.0.0.1");
  await once(school, "listening");
  await vite.listen();
  const target = process.env.CARGO_TARGET_DIR || "src-tauri/target";
  const executable = resolve(target, "debug", process.platform === "win32" ? "kladde.exe" : "kladde");
  child = spawn(executable, [], { stdio: "inherit", env: {
    ...process.env,
    KLADDE_DATA_DIR: dir,
    XDG_DATA_HOME: join(dir, "data"),
    XDG_CONFIG_HOME: join(dir, "config"),
    XDG_CACHE_HOME: join(dir, "cache"),
    KLADDE_SCHOOL_TEST_ORIGIN: "http://127.0.0.1:" + school.address().port,
    WEBVIEW2_USER_DATA_FOLDER: join(dir, "webview"),
  } });
  closed = once(child, "close");
  const outcome = await Promise.race([
    result, closed.then(([code]) => "App exited with code " + code),
    new Promise(resolveTimeout => { timer = setTimeout(() => resolveTimeout("Desktop test timed out. School rendered: " + pageRendered), 90_000); }),
  ]);
  assert.equal(outcome, "ok", outcome);
  assert.ok(pageRendered, "The school webview must render its first page");
  const saved = JSON.parse(await readFile(join(dir, "kladde.json"), "utf8"));
  assert.equal(saved.notes.length, 1);
  assert.equal(saved.notes[0].subject, "Literature");
  assert.equal(saved.notes[0].subject_key, "literature");
  assert.equal(saved.notes[0].content, "Keep this draft");
  assert.equal(saved.tasks.length, 1);
  assert.equal(saved.tasks[0].deadline_time, "19:15");
  console.log("Desktop regression passed: school launch and preview, offline save, and subject editing.");
} finally {
  clearTimeout(timer);
  if (child?.exitCode === null) child.kill();
  await closed;
  await vite.close();
  school.closeAllConnections();
  school.close();
  await rm(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
}
