//! Claude Desktop launches Kladde in stdio mode. The connector can read one explicitly
//! shared snapshot and submit drafts. Only the open desktop app writes the study store.
use crate::commands::{err, AppState, Res};
use crate::store::{is_valid_date, new_id, now_iso, Note, Task};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

type Result<T> = std::result::Result<T, String>;
fn read(file: &Path) -> Result<Value> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn write(file: &Path, value: &Value) -> Result<()> {
    let tmp = file.with_file_name(format!(".{}.tmp", new_id()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        serde_json::to_writer_pretty(&mut f, value)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result.map_err(|e| e.to_string())
}
fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn draft_path(dir: &Path, id: &str) -> Result<PathBuf> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| "Invalid draft ID")?;
    Ok(dir.join(format!("draft-{id}.json")))
}
fn text<'a>(obj: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    let v = obj[key].as_str().unwrap_or("").trim();
    if v.is_empty() || v.chars().count() > max {
        return Err(format!("Invalid or missing {key}"));
    }
    Ok(v)
}
fn submit(dir: &Path, args: &Value) -> Result<String> {
    let share = read(&dir.join("shared.json")).map_err(|_| "Share an item in Kladde first")?;
    if share["shareId"] != args["shareId"] {
        return Err("The shared item changed. Read it again.".into());
    }
    let kind = text(args, "kind", 10)?;
    let title = text(args, "title", 300)?;
    let id = new_id();
    let mut draft = json!({"id":id,"kind":kind,"title":title,"subject":share["subject"]});
    match kind {
        "note" => draft["content"] = json!(text(args, "content", 200_000)?),
        "task" => {
            let date = text(args, "deadline", 10)?;
            if !is_valid_date(date) {
                return Err("Use YYYY-MM-DD for the deadline".into());
            }
            draft["deadline"] = json!(date)
        }
        _ => return Err("Choose note or task".into()),
    }
    write(&draft_path(dir, &id)?, &draft)?;
    Ok(id)
}
fn tools_list() -> Value {
    json!({"tools":[{"name":"read_shared_item","description":"Read the single note or homework snapshot the user shared in Kladde.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"suggest_draft","description":"Send a proposed note or homework task to the user's Kladde inbox for review. Use the shareId returned by read_shared_item.",
        "inputSchema":{"type":"object","properties":{"shareId":{"type":"string"},"kind":{"type":"string","enum":["note","task"]},"title":{"type":"string"},"content":{"type":"string","description":"Required for notes"},"deadline":{"type":"string","description":"YYYY-MM-DD, required for tasks"}},"required":["shareId","kind","title"],"additionalProperties":false}}]})
}
fn rpc(dir: &Path, msg: Value) -> Option<Value> {
    let id = msg.get("id")?;
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => {
            let requested = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = match requested {
                "2024-11-05" | "2025-03-26" | "2025-06-18" => requested,
                _ => "2025-11-25",
            };
            json!({"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":"Kladde","version":env!("CARGO_PKG_VERSION")}})
        }
        "ping" => json!({}),
        "tools/list" => tools_list(),
        "tools/call" => {
            let res = match msg["params"]["name"].as_str() {
                Some("read_shared_item") => read(&dir.join("shared.json"))
                    .map(|v| v.to_string())
                    .map_err(|_| "No shared item. Select Share with Claude in Kladde.".into()),
                Some("suggest_draft") => submit(dir, &msg["params"]["arguments"])
                    .map(|_| "Draft queued for review in Kladde.".into()),
                _ => Err("Unknown tool".into()),
            };
            let is_error = res.is_err();
            json!({"content":[{"type":"text","text":res.unwrap_or_else(|e|e)}],"isError":is_error})
        }
        _ => {
            return Some(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
            )
        }
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
pub fn stdio(dir: &Path) {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin()
        .lock()
        .lines()
        .map_while(std::result::Result::ok)
    {
        let reply = match serde_json::from_str(&line) {
            Ok(msg) => rpc(dir, msg),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
            ),
        };
        if let Some(reply) = reply {
            if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
}
fn install(app: &AppHandle, dir: &Path) -> Result<()> {
    let path = app
        .path()
        .home_dir()
        .map_err(|e| e.to_string())?
        .join("Library/Application Support/Claude/claude_desktop_config.json");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut config = if path.exists() {
        read(&path)?
    } else {
        json!({})
    };
    let root = config
        .as_object_mut()
        .ok_or("Invalid Claude Desktop configuration")?;
    let servers = root
        .entry("mcpServers")
        .or_insert(json!({}))
        .as_object_mut()
        .ok_or("Invalid MCP server configuration")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    servers.insert(
        "kladde".into(),
        json!({"command":exe,"args":["--kladde-mcp",dir]}),
    );
    write(&path, &config)
}
#[tauri::command]
pub fn claude_action(app: AppHandle, body: Value) -> Res<Value> {
    if !cfg!(target_os = "macos") {
        return Err(err(400, "Claude Desktop connection requires macOS"));
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| err(500, e.to_string()))?
        .join("claude");
    std::fs::create_dir_all(&dir).map_err(|e| err(500, e.to_string()))?;
    let shared = dir.join("shared.json");
    let state = app.state::<AppState>();
    let id = body["id"].as_str().unwrap_or("");
    match body["action"].as_str() {
        Some("install") => install(&app, &dir)?,
        Some("revoke") => remove(&shared)?,
        Some("share") => {
            let store = state.store.lock().unwrap_or_else(|p| p.into_inner());
            let snapshot=match body["kind"].as_str(){
                Some("note")=>store.data.notes.iter().find(|n|n.id==id).map(|n|json!({"kind":"note","title":n.title,"subject":n.subject,"content":n.content})),
                Some("task")=>store.data.tasks.iter().find(|t|t.id==id).map(|t|json!({"kind":"task","title":t.title,"subject":t.subject,"deadline":t.deadline,"estimated_minutes":t.estimated_minutes})),
                _=>None
            };
            let mut snapshot = snapshot.ok_or_else(|| err(404, "Study item not found"))?;
            snapshot["shareId"] = json!(new_id());
            write(&shared, &snapshot)?;
        }
        Some("discard") => remove(&draft_path(&dir, id)?)?,
        Some("accept") => {
            let path = draft_path(&dir, id)?;
            let draft = read(&path)?;
            let mut store = state.store.lock().unwrap_or_else(|p| p.into_inner());
            let mut tx = store.transaction();
            let subject = tx.data.ensure_subject(text(&draft, "subject", 60)?).0;
            let title = text(&draft, "title", 300)?.to_string();
            let now = now_iso();
            if draft["kind"] == "note" {
                if !tx.data.notes.iter().any(|n| n.id == id) {
                    tx.data.notes.push(Note {
                        id: id.into(),
                        title,
                        content: text(&draft, "content", 200_000)?.into(),
                        subject: subject.name,
                        subject_key: subject.key,
                        created_at: now.clone(),
                        updated_at: now,
                        ..Default::default()
                    });
                }
            } else if draft["kind"] == "task"
                && is_valid_date(draft["deadline"].as_str().unwrap_or(""))
            {
                if !tx.data.tasks.iter().any(|t| t.id == id) {
                    tx.data.tasks.push(Task {
                        id: id.into(),
                        title,
                        deadline: draft["deadline"].as_str().unwrap().into(),
                        subject: subject.name,
                        subject_key: subject.key,
                        created_at: now,
                        ..Default::default()
                    })
                }
            } else {
                return Err(err(400, "Invalid draft"));
            }
            tx.commit()?;
            remove(&path)?;
        }
        Some("status") => {}
        _ => return Err(err(400, "Unknown Claude action")),
    }
    let mut drafts = vec![];
    for entry in std::fs::read_dir(dir).map_err(|e| err(500, e.to_string()))? {
        let entry = entry.map_err(|e| err(500, e.to_string()))?;
        if entry.file_name().to_string_lossy().starts_with("draft-") {
            drafts.push(read(&entry.path())?)
        }
    }
    Ok(json!({"shared":read(&shared).ok().map(|s|s["title"].clone()),"drafts":drafts}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcp_requires_explicit_share_and_only_queues_a_draft() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let args = json!({"shareId":"one","kind":"note","title":"Review","content":"Draft text"});
        assert!(submit(dir, &args).is_err());
        write(
            &dir.join("shared.json"),
            &json!({"shareId":"one","subject":"Dansk"}),
        )
        .unwrap();
        let id = submit(dir, &args).unwrap();
        assert_eq!(
            read(&draft_path(dir, &id).unwrap()).unwrap()["content"],
            "Draft text"
        );
        assert!(!dir.join("kladde.json").exists());
        remove(&dir.join("shared.json")).unwrap();
        assert!(submit(dir, &args).is_err());
        assert!(draft_path(dir, "../kladde").is_err());
    }
}
