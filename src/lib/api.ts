// Talks to Kladde's Rust backend (src-tauri/src/commands.rs) through Tauri commands.
// The rest of the app still calls apiGet("/dashboard"), apiPatch(`/notes/${id}`, …) etc.;
// the small route table below turns those into the matching command.
import { invoke } from "@tauri-apps/api/core";

export class ApiError extends Error {
  status: number;
  body: unknown;

  constructor(status: number, body: unknown) {
    super(`request failed with ${status}`);
    this.name = "ApiError";
    this.status = status;
    this.body = body;
  }
}

/** Errors come back as { detail: "message" }; this returns that message or a fallback. */
export const errorDetail = (e: unknown, fallback: string) =>
  e instanceof ApiError &&
  e.body &&
  typeof e.body === "object" &&
  "detail" in e.body &&
  typeof e.body.detail === "string"
    ? e.body.detail
    : fallback;

export const currentLang = () => {
  try {
    return localStorage.getItem("kladde-language") || "da";
  } catch {
    return "da";
  }
};

type Method = "GET" | "POST" | "PATCH" | "DELETE";
type Route = [Method, RegExp, string, (match: RegExpMatchArray, body: unknown) => Record<string, unknown>];

const noArgs = () => ({});
const withBody = (_m: RegExpMatchArray, body: unknown) => ({ body: body ?? {} });
const withId = (m: RegExpMatchArray) => ({ id: m[1] });
const withIdAndBody = (m: RegExpMatchArray, body: unknown) => ({ id: m[1], body: body ?? {} });

const routes: Route[] = [
  ["GET", /^\/dashboard$/, "dashboard", noArgs],
  ["GET", /^\/weekly-summary$/, "weekly_summary_cmd", noArgs],
  ["POST", /^\/subjects$/, "create_subject", withBody],
  ["PATCH", /^\/subjects\/([^/]+)$/, "rename_subject", withIdAndBody],
  ["DELETE", /^\/subjects\/([^/]+)$/, "delete_subject", withIdAndBody],
  ["POST", /^\/notes$/, "create_note", withBody],
  ["PATCH", /^\/notes\/([^/]+)$/, "update_note", withIdAndBody],
  ["DELETE", /^\/notes\/([^/]+)$/, "delete_note", withId],
  ["POST", /^\/tasks\/batch$/, "create_project_tasks", withBody],
  ["POST", /^\/tasks$/, "create_task", withBody],
  ["PATCH", /^\/tasks\/([^/]+)$/, "update_task", withIdAndBody],
  ["DELETE", /^\/tasks\/([^/]+)$/, "delete_task", withId],
  ["POST", /^\/blocks$/, "create_block", withBody],
  ["DELETE", /^\/blocks\/([^/]+)$/, "delete_block", withId],
  ["POST", /^\/study-plan$/, "make_study_plan_cmd", noArgs],
  ["DELETE", /^\/study-plan$/, "clear_study_plan", noArgs],
  ["POST", /^\/imports\/preview$/, "import_preview", withBody],
  ["POST", /^\/imports\/commit$/, "import_commit", withBody],
  ["GET", /^\/calendar-feeds$/, "list_feeds", noArgs],
  ["POST", /^\/calendar-feeds$/, "add_feed", withBody],
  ["POST", /^\/calendar-feeds\/([^/]+)\/sync$/, "sync_feed", withId],
  ["DELETE", /^\/calendar-feeds\/([^/]+)$/, "delete_feed", withId],
  // School window (Ludus / Mit Niels Brock)
  ["POST", /^\/school\/open$/, "open_school", withBody],
  // Hosted AI and Claude Desktop
  ["GET", /^\/ai\/status$/, "ai_status", noArgs],
  ["POST", /^\/ai\/key$/, "ai_save_key", withBody],
  ["DELETE", /^\/ai\/key$/, "ai_forget_key", noArgs],
  ["POST", /^\/ai\/codex$/, "ai_codex", withBody],
  ["POST", /^\/claude$/, "claude_action", withBody],
  ["POST", /^\/ai\/note$/, "ai_note", withBody],
  ["POST", /^\/ai\/project-plan$/, "ai_project_plan", withBody],
  ["GET", /^\/ai\/messages$/, "ai_messages", noArgs],
  ["POST", /^\/ai\/chat$/, "ai_chat", withBody],
  ["DELETE", /^\/ai\/messages$/, "ai_clear_chat", noArgs],
];

/** Calls a Rust command and turns its { status, detail } errors into ApiError. */
async function call<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, { ...args, lang: currentLang() });
  } catch (e) {
    if (e && typeof e === "object" && "status" in e && "detail" in e) {
      const { status, detail } = e as { status: number; detail: string };
      throw new ApiError(status, { detail });
    }
    throw new ApiError(500, { detail: String(e) });
  }
}

function request<T>(method: Method, path: string, body?: unknown): Promise<T> {
  for (const [m, pattern, command, args] of routes) {
    const match = m === method ? path.match(pattern) : null;
    if (match) return call<T>(command, args(match, body));
  }
  return Promise.reject(new ApiError(404, { detail: `Unknown endpoint: ${method} ${path}` }));
}

export const apiGet = <T>(path: string) => request<T>("GET", path);
export const apiPost = <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {});
export const apiPatch = <T>(path: string, body?: unknown) => request<T>("PATCH", path, body ?? {});
export const apiDelete = <T>(path: string, body?: unknown) => request<T>("DELETE", path, body);

/** Opens a Save dialog (starting in Documents) and writes a backup. Resolves to the saved path, or null if cancelled. */
export const exportBackup = () => call<string | null>("export_backup", {});
