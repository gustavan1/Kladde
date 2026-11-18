//! Short-lived, private stdio sessions with the bundled Codex app server.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

type Result<T> = std::result::Result<T, String>;
struct Client {
    child: Child,
    input: ChildStdin,
    events: Receiver<Value>,
    id: u64,
    deadline: Instant,
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Client {
    fn start(app: &AppHandle) -> Result<Self> {
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("kladde-codex");
        let home = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("codex");
        Self::spawn(&exe, &home)
    }
    fn spawn(exe: &Path, home: &Path) -> Result<Self> {
        let cwd = home.join("study-session");
        std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.arg("app-server").args(["--listen", "stdio://"]).args([
            "-c",
            "cli_auth_credentials_store=\"keyring\"",
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "web_search=\"disabled\"",
        ]);
        for feature in [
            "shell_tool",
            "unified_exec",
            "apply_patch_freeform",
            "apps",
            "multi_agent",
            "code_mode",
            "js_repl",
            "view_image",
            "image_generation",
        ] {
            cmd.args(["--disable", feature]);
        }
        let mut child = cmd
            .env("CODEX_HOME", home)
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot start bundled Codex: {e}"))?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if let Ok(value) = serde_json::from_str(&line) {
                    if tx.send(value).is_err() {
                        break;
                    }
                }
            }
        });
        let mut c = Self {
            child,
            input,
            events,
            id: 0,
            deadline: Instant::now() + Duration::from_secs(300),
        };
        c.request("initialize",json!({"clientInfo":{"name":"kladde","title":"Kladde","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}))?;
        c.send(json!({"method":"initialized"}))?;
        Ok(c)
    }
    fn send(&mut self, msg: Value) -> Result<()> {
        writeln!(self.input, "{msg}")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())
    }
    fn next(&mut self) -> Result<Value> {
        loop {
            let v = self
                .events
                .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
                .map_err(|e| format!("Codex session ended or timed out: {e}"))?;
            if v.get("method").is_some() && v.get("id").is_some() {
                self.send(json!({"id":v["id"],"error":{"code":-32601,"message":"Kladde does not provide tools or approvals"}}))?;
            } else {
                return Ok(v);
            }
        }
    }
    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.id += 1;
        self.send(json!({"id":self.id,"method":method,"params":params}))?;
        loop {
            let msg = self.next()?;
            if msg["id"].as_u64() == Some(self.id) {
                if let Some(e) = msg.get("error") {
                    return Err(e["message"]
                        .as_str()
                        .unwrap_or("Codex request failed")
                        .into());
                }
                return Ok(msg["result"].clone());
            }
        }
    }
    fn signed_in(&mut self) -> Result<bool> {
        Ok(
            self.request("account/read", json!({"refreshToken":false}))?["account"]["type"]
                == "chatgpt",
        )
    }
}
pub fn models(app: &AppHandle, login: bool) -> Result<Vec<String>> {
    let mut c = Client::start(app)?;
    if !c.signed_in()? {
        if !login {
            return Err("Connect your ChatGPT account first.".into());
        }
        let result = c.request("account/login/start", json!({"type":"chatgpt"}))?;
        let url = result["authUrl"]
            .as_str()
            .ok_or("Codex did not return a sign-in link")?;
        if !url.starts_with("https://") {
            return Err("Invalid Codex sign-in URL".into());
        }
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())?;
        loop {
            let msg = c.next()?;
            if msg["method"] == "account/login/completed"
                && msg["params"]["loginId"] == result["loginId"]
            {
                if msg["params"]["success"] != true {
                    return Err("ChatGPT sign-in was cancelled or failed.".into());
                }
                break;
            }
        }
    }
    let mut found = vec![];
    let mut cursor = Value::Null;
    loop {
        let page = c.request("model/list", json!({"cursor":cursor,"limit":100}))?;
        for model in page["data"].as_array().ok_or("Invalid Codex model list")? {
            if let Some(id) = model["model"].as_str() {
                if model["isDefault"] == true {
                    found.insert(0, id.into())
                } else {
                    found.push(id.into())
                }
            }
        }
        cursor = page["nextCursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    if found.is_empty() {
        return Err("No Codex models available for this account.".into());
    }
    Ok(found)
}
pub fn logout(app: &AppHandle) -> Result<()> {
    Client::start(app)?
        .request("account/logout", json!({}))
        .map(|_| ())
}

pub fn answer(
    app: &AppHandle,
    model: &str,
    system: &str,
    messages: Vec<Value>,
    schema: Option<Value>,
) -> Result<String> {
    let mut c = Client::start(app)?;
    if !c.signed_in()? {
        return Err("Reconnect ChatGPT in AI settings.".into());
    }
    let thread=c.request("thread/start",json!({"model":model,"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only",
        "environments":[],"selectedCapabilityRoots":[],"dynamicTools":[],"baseInstructions":system}))?;
    let id = thread["thread"]["id"]
        .as_str()
        .ok_or("Codex returned no thread")?;
    let mut turn = json!({"threadId":id,"environments":[],"input":[{"type":"text","text":serde_json::to_string(&messages).map_err(|e|e.to_string())?}]});
    if let Some(schema) = schema {
        turn["outputSchema"] = schema
    }
    c.request("turn/start", turn)?;
    let mut answer = String::new();
    loop {
        let msg = c.next()?;
        let p = &msg["params"];
        if p["threadId"] != id {
            continue;
        }
        if msg["method"] == "item/completed" && p["item"]["type"] == "agentMessage" {
            if let Some(t) = p["item"]["text"].as_str() {
                answer = t.into()
            }
        }
        if msg["method"] == "turn/completed" {
            if p["turn"]["status"] != "completed" {
                return Err(p["turn"]["error"]["message"]
                    .as_str()
                    .unwrap_or("Codex turn failed")
                    .into());
            }
            if answer.trim().is_empty() {
                return Err("Codex returned no answer.".into());
            }
            return Ok(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Set to the pinned executable when running the installer verification job.
    #[test]
    #[ignore = "requires the bundled Codex executable"]
    fn bundled_server_starts_without_credentials() {
        let binary = std::env::var_os("KLADDE_TEST_CODEX").expect("provide bundled Codex");
        let home = tempfile::tempdir().unwrap();
        let mut client = Client::spawn(Path::new(&binary), home.path()).unwrap();
        assert!(!client.signed_in().unwrap());
    }
}
