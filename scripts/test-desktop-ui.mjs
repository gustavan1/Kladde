// Only the test server injects this script into the main webview.
const byId = id => document.querySelector('[data-testid="' + id + '"]');
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(read, description) {
  for (let i = 0; i < 150; i++) {
    const value = read();
    if (value) return value;
    await pause(100);
  }
  throw new Error("Timed out waiting for " + description);
}
const control = id => until(() => byId(id), id);
const click = async id => (await control(id)).click();
async function fill(id, value) {
  const node = await control(id);
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(node), "value").set;
  setter.call(node, value);
  node.dispatchEvent(new Event(node.tagName === "SELECT" ? "change" : "input", { bubbles: true }));
  await pause(0);
}
async function saveSubject(name) {
  await fill("subject-add-input", name);
  byId("subject-add-input").form.requestSubmit();
  await until(() => !byId("subject-add-input"), "subject save");
}

async function run() {
  await control("workspace-shell");
  await click("sidebar-nav-imports");
  await click("school-open-mitnielsbrock");
  await click("sidebar-nav-homework");
  await click("new-task-btn");
  await fill("task-title-input", "Unsaved homework");
  await fetch("/__test/show-table", { method: "POST" });
  await until(() => byId("import-preview-item")?.textContent.includes("Fixture worksheet"), "school preview");
  if (byId("task-title-input")?.value !== "Unsaved homework") throw new Error("School preview lost the homework draft");
  await click("sidebar-nav-imports");
  await click("import-commit-button");
  await until(() => !byId("import-preview-item"), "confirmed import");

  await click("sidebar-nav-notes");
  await click("notes-new-button");
  await fill("note-editor-title-input", "Desktop regression");
  await fill("note-editor-content-textarea", "Keep this draft");
  window.dispatchEvent(new Event("offline"));
  await click("note-new-subject");
  await saveSubject("Robotics");
  if (byId("note-editor-subject-select").value !== "robotics") throw new Error("New subject was not assigned");
  await click("note-editor-save-btn");
  await control("selected-note-title");
  await click("note-edit-button");
  await fill("note-editor-subject-select", "engelsk");
  await click("note-editor-save-btn");
  await until(() => byId("note-editor-panel")?.querySelector('[data-testid="subject-tag-engelsk"]'), "note reassignment");
  await click("subject-rename-button");
  await saveSubject("Literature");
  await until(() => byId("note-editor-panel")?.querySelector('[data-testid="subject-tag-literature"]'), "renamed subject on saved note");
}

try {
  await run();
  await fetch("/__test/result", { method: "POST", body: "ok" });
} catch (error) {
  await fetch("/__test/result", { method: "POST", body: String(error?.stack || error) });
}
