//! Hosted study tools. Cloudflare credentials are stored by the OS. Codex owns ChatGPT sign-in.
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::commands::{err, is_da, str_of, tr, ApiErr, AppState, Res};
use crate::store::{add_days, is_valid_date, local_date, new_id, now_iso, ChatMessage, Data};

const CF_MODEL: &str = "@cf/openai/gpt-oss-20b";

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct AiSettings {
    account: String,
    use_codex: bool,
    codex_model: String,
}

pub struct AiState {
    path: PathBuf,
    settings: Mutex<AiSettings>,
}

impl AiState {
    pub fn load(path: PathBuf) -> Self {
        let settings = std::fs::read(&path)
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default();
        Self {
            path,
            settings: Mutex::new(settings),
        }
    }
    pub(crate) fn get(&self) -> AiSettings {
        self.settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    fn update(&self, change: impl FnOnce(&mut AiSettings)) -> Res<Value> {
        let mut guard = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = guard.clone();
        change(&mut next);
        let tmp = self.path.with_extension("tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec(&next).map_err(|e| err(500, e.to_string()))?,
        )
        .and_then(|_| std::fs::rename(tmp, &self.path))
        .map_err(|e| err(500, e.to_string()))?;
        *guard = next;
        Ok(status(&guard))
    }
}

impl AiSettings {
    /// True when some AI provider is set up and can be used.
    pub(crate) fn is_configured(&self) -> bool {
        if cfg!(target_os = "macos") && self.use_codex {
            !self.codex_model.is_empty()
        } else {
            !self.account.is_empty()
        }
    }
}

fn status(s: &AiSettings) -> Value {
    let codex = cfg!(target_os = "macos") && s.use_codex;
    json!({"configured":if codex { !s.codex_model.is_empty() } else { !s.account.is_empty() },
        "provider":if codex {"codex"} else {"cloudflare"}, "mac":cfg!(target_os="macos"), "codex_connected":!s.codex_model.is_empty(),
        "model":if codex { &s.codex_model } else { CF_MODEL }, "account":s.account})
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn credential(account: &str) -> Res<keyring::Entry> {
    keyring::Entry::new("dk.kladde.app.cloudflare", account)
        .map_err(|e| err(500, format!("Credential store: {e}")))
}
fn token(account: &str, change: Option<&str>) -> Res<String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let entry = credential(account)?;
        let result = match change {
            Some("") => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(String::new()),
                Err(e) => Err(e),
            },
            Some(value) => entry.set_password(value).map(|_| String::new()),
            None => entry.get_password(),
        };
        result.map_err(|e| err(500, format!("Credential store: {e}")))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (account, change);
        Err(err(400, "Credential storage requires Windows or macOS."))
    }
}

fn cf_error(code: u16, lang: &Option<String>) -> ApiErr {
    let (da, en) = match code {
        401 | 403 => (
            "Tjek dit Account ID og tokenets Workers AI-tilladelser.",
            "Check your Account ID and your token’s Workers AI permissions.",
        ),
        404 => (
            "Account ID eller model blev ikke fundet.",
            "Cloudflare could not find the account or model.",
        ),
        429 => (
            "Cloudflares grænse er nået. Tjek dit forbrug i dashboardet.",
            "Cloudflare’s quota or rate limit was reached. Check usage in your dashboard.",
        ),
        0 => (
            "Kunne ikke forbinde til Cloudflare. Tjek internetforbindelsen.",
            "Could not reach Cloudflare. Check your connection.",
        ),
        _ => (
            "Cloudflare kunne ikke behandle forespørgslen.",
            "Cloudflare could not process the request.",
        ),
    };
    err(if code == 0 { 503 } else { code }, tr(lang, da, en))
}

fn cloudflare_at(
    url: &str,
    key: &str,
    system: &str,
    messages: Vec<Value>,
    schema: Option<Value>,
    lang: &Option<String>,
) -> Res<String> {
    let mut chat = vec![json!({"role":"system","content":system})];
    if let Some(schema) = schema {
        chat.push(json!({"role":"system","content":format!("Return only a JSON object matching this schema: {schema}")}));
    }
    chat.extend(messages);
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(120))
        .build()
        .post(url)
        .set("Authorization", &format!("Bearer {key}"))
        .send_json(json!({"model":CF_MODEL,"messages":chat,"max_tokens":4096,"stream":false}))
        .map_err(|e| match e {
            ureq::Error::Status(code, response) => {
                let body = response.into_json::<Value>().unwrap_or_default();
                match body["errors"][0]["code"].as_u64() {
                    Some(3036) => err(
                        429,
                        tr(
                            lang,
                            "Din gratis dagskvote er brugt. Prøv igen efter kl. 00.00 UTC.",
                            "Your daily free allowance is used up. Try again after 00:00 UTC.",
                        ),
                    ),
                    Some(3040) => err(
                        429,
                        tr(
                            lang,
                            "Cloudflare har travlt. Prøv igen om lidt.",
                            "Cloudflare is temporarily at capacity. Try again shortly.",
                        ),
                    ),
                    _ => cf_error(code, lang),
                }
            }
            _ => cf_error(0, lang),
        })?;
    let body: Value = response.into_json().map_err(|_| cf_error(502, lang))?;
    body["choices"][0]["message"]["content"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| cf_error(502, lang))
}

fn cf_url(account: &str) -> String {
    format!("https://api.cloudflare.com/client/v4/accounts/{account}/ai/v1/chat/completions")
}

pub(crate) fn generate(
    app: &AppHandle,
    s: &AiSettings,
    system: &str,
    messages: Vec<Value>,
    schema: Option<Value>,
    lang: &Option<String>,
) -> Res<String> {
    if s.use_codex && cfg!(target_os = "macos") {
        return crate::codex_client::answer(app, &s.codex_model, system, messages, schema)
            .map_err(Into::into);
    }
    if s.account.is_empty() {
        return Err(err(
            428,
            tr(
                lang,
                "Opsæt Cloudflare i AI-indstillinger.",
                "Set up Cloudflare in AI settings.",
            ),
        ));
    }
    cloudflare_at(
        &cf_url(&s.account),
        &token(&s.account, None)?,
        system,
        messages,
        schema,
        lang,
    )
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| err(500, e.to_string()))?
}
#[tauri::command]
pub fn ai_status(app: AppHandle) -> Res<Value> {
    Ok(status(&app.state::<AiState>().get()))
}

#[tauri::command]
pub async fn ai_save_key(app: AppHandle, body: Value, lang: Option<String>) -> Res<Value> {
    let account = body["account"].as_str().unwrap_or("").trim().to_owned();
    let key = body["key"].as_str().unwrap_or("").trim().to_owned();
    if account.len() != 32 || !account.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err(
            400,
            tr(
                &lang,
                "Account ID skal være 32 hexadecimale tegn.",
                "Account ID must contain 32 hexadecimal characters.",
            ),
        ));
    }
    blocking(move || {
        let key = if key.is_empty() {
            token(&account, None)?
        } else {
            key
        };
        cloudflare_at(
            &cf_url(&account),
            &key,
            "Reply briefly.",
            vec![json!({"role":"user","content":"Say OK."})],
            None,
            &lang,
        )?;
        token(&account, Some(&key))?;
        let state = app.state::<AiState>();
        let old = state.get().account;
        let status = state.update(|s| {
            s.account = account.clone();
            s.use_codex = false;
        })?;
        if !old.is_empty() && old != account {
            token(&old, Some(""))?;
        }
        Ok(status)
    })
    .await
}

#[tauri::command]
pub async fn ai_forget_key(app: AppHandle) -> Res<Value> {
    blocking(move || {
        let state = app.state::<AiState>();
        let account = state.get().account;
        if !account.is_empty() {
            token(&account, Some(""))?;
        }
        state.update(|s| s.account.clear())
    })
    .await
}

#[tauri::command]
pub async fn ai_codex(app: AppHandle, body: Value) -> Res<Value> {
    if !cfg!(target_os = "macos") {
        return Err(err(400, "Codex is available in the Mac edition."));
    }
    blocking(move || {
        let state = app.state::<AiState>();
        match body["action"].as_str() {
            Some("cloudflare") => state.update(|s| s.use_codex = false),
            Some("logout") => {
                crate::codex_client::logout(&app)?;
                state.update(|s| {
                    s.codex_model.clear();
                    s.use_codex = false;
                })
            }
            Some(action @ ("connect" | "models" | "select")) => {
                let models = crate::codex_client::models(&app, action == "connect")?;
                let requested = body["model"].as_str().unwrap_or("");
                let current = state.get().codex_model;
                let model = if action == "select" {
                    models
                        .iter()
                        .find(|m| m.as_str() == requested)
                        .ok_or_else(|| err(400, "Model unavailable"))?
                } else {
                    models.iter().find(|m| **m == current).unwrap_or(&models[0])
                };
                let mut out = state.update(|s| {
                    s.use_codex = true;
                    s.codex_model = model.clone();
                })?;
                out["models"] = json!(models);
                Ok(out)
            }
            _ => Err(err(400, "Unknown Codex action")),
        }
    })
    .await
}

const PLAIN_TEXT: &str = "Write plain text for a simple text box: no Markdown (no #, no ** or __), no tables. Use short paragraphs, headings as their own line ending with a colon, and lists with \"- \". Formulas in plain text, e.g. f'(x) = 2x.";

pub fn note_prompt(action: &str, da: bool) -> Option<&'static str> {
    let _ = da;
    Some(match action {
        "organize" => "Rewrite the student's note into clear, well-structured study notes. Keep every fact, formula, date and source from the original; fix spelling; group related points under headings; do not add new facts.",
        "summary" => "Write a short summary (5-8 bullet points) of the note's most important points, for revising before a test.",
        "from_text" => "The student pasted source text (a textbook page, article or slides). Turn it into good study notes: key ideas, important terms with short explanations, formulas, and dates. Be concise.",
        "explain" => "Explain the hardest ideas in the note simply, as a good teacher would for an upper-secondary student, with one small example each. Point out anything in the note that looks wrong.",
        "questions" => "Write 5 exam-style practice questions about the note, from simple recall to explaining or applying an idea. After the questions, give short model answers under the heading \"Svar:\" (or \"Answers:\" if the note is in English).",
        _ => return None,
    })
}

#[tauri::command]
pub async fn ai_note(app: AppHandle, lang: Option<String>, body: Value) -> Res<Value> {
    let s = app.state::<AiState>().get();
    let provider_app = app.clone();
    let action = str_of(body.get("action"), 20);
    let Some(task) = note_prompt(&action, is_da(&lang)) else {
        return Err(err(400, "Unknown action"));
    };
    let content = body["content"].as_str().unwrap_or("");
    if content.chars().count() > 60_000 {
        return Err(err(
            400,
            tr(
                &lang,
                "AI kan behandle op til 60.000 tegn ad gangen.",
                "AI can process up to 60,000 characters at a time.",
            ),
        ));
    }
    if content.trim().len() < 20 {
        return Err(err(
            400,
            tr(
                &lang,
                "Skriv eller indsæt lidt mere tekst først.",
                "Write or paste a bit more text first.",
            ),
        ));
    }
    let title = str_of(body.get("title"), 300);
    let subject = str_of(body.get("subject"), 60);
    let system = format!(
        "You help a Danish upper-secondary student (gymnasium/HHX/HTX) with their notes. {task} Write in the same language as the note (usually Danish). {PLAIN_TEXT}"
    );
    let user = format!("Subject: {subject}\nNote title: {title}\n\n<note>\n{content}\n</note>");
    let l = lang.clone();
    let text = blocking(move || {
        generate(
            &provider_app,
            &s,
            &system,
            vec![json!({ "role": "user", "content": user })],
            None,
            &l,
        )
    })
    .await?;
    Ok(json!({ "text": text }))
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PlanStep {
    pub title: String,
    pub deadline: String,
    pub estimated_minutes: i64,
    pub priority: String,
}

/// Cleans the AI's project steps: dates between today and the project deadline, sensible
/// minutes, known priorities, sorted by date.
pub fn clean_steps(raw: &Value, today: &str, deadline: &str) -> Vec<PlanStep> {
    let mut steps: Vec<PlanStep> = raw["steps"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let title = s["title"]
                        .as_str()?
                        .trim()
                        .chars()
                        .take(200)
                        .collect::<String>();
                    if title.is_empty() {
                        return None;
                    }
                    let mut date = s["deadline"].as_str().unwrap_or("").to_string();
                    if !is_valid_date(&date) || date.as_str() > deadline {
                        date = deadline.to_string();
                    }
                    if date.as_str() < today {
                        date = today.to_string();
                    }
                    let minutes = s["estimated_minutes"].as_f64().unwrap_or(60.0).round() as i64;
                    let priority = match s["priority"].as_str() {
                        Some(p @ ("high" | "medium" | "low")) => p.to_string(),
                        _ => "medium".to_string(),
                    };
                    Some(PlanStep {
                        title,
                        deadline: date,
                        estimated_minutes: minutes.clamp(10, 600),
                        priority,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    steps.sort_by(|a, b| a.deadline.cmp(&b.deadline));
    steps.truncate(12);
    steps
}

#[tauri::command]
pub async fn ai_project_plan(app: AppHandle, lang: Option<String>, body: Value) -> Res<Value> {
    let s = app.state::<AiState>().get();
    let provider_app = app.clone();
    let title = str_of(body.get("title"), 300).trim().to_string();
    let details = str_of(body.get("details"), 10_000);
    let subject = str_of(body.get("subject"), 60);
    let deadline = str_of(body.get("deadline"), 10);
    let today = local_date(&Local::now());
    if title.is_empty() || !is_valid_date(&deadline) {
        return Err(err(
            400,
            tr(
                &lang,
                "Skriv hvad projektet er, og vælg en afleveringsdato.",
                "Describe the project and choose a due date.",
            ),
        ));
    }
    if deadline < today {
        return Err(err(
            400,
            tr(
                &lang,
                "Afleveringsdatoen er allerede passeret.",
                "The due date has already passed.",
            ),
        ));
    }
    let days = crate::store::days_between(&today, &deadline);
    let system = format!(
        "You help a Danish upper-secondary student plan a school project or hand-in. Split it into 3-8 concrete steps, each a task they can finish in one sitting or a few. Give each step a deadline (YYYY-MM-DD) between today and the final deadline, spread out sensibly with the final check/hand-in step on the final deadline, a realistic estimate in minutes and a priority (high, medium or low). Also give short advice (2-3 sentences). Write titles and advice in {}.",
        if is_da(&lang) { "Danish" } else { "English" }
    );
    let user = format!(
        "Today is {today}. Final deadline: {deadline} ({days} days from now).\nSubject: {subject}\nProject: {title}\nDetails from the student: {details}"
    );
    let schema = json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "steps": {
                "type": "array",
                "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {
                        "title": { "type": "string" },
                        "deadline": { "type": "string" },
                        "estimated_minutes": { "type": "integer" },
                        "priority": { "type": "string", "enum": ["high", "medium", "low"] }
                    },
                    "required": ["title", "deadline", "estimated_minutes", "priority"]
                }
            },
            "advice": { "type": "string" }
        },
        "required": ["steps", "advice"]
    });
    let l = lang.clone();
    let text = blocking(move || {
        generate(
            &provider_app,
            &s,
            &system,
            vec![json!({ "role": "user", "content": user })],
            Some(schema),
            &l,
        )
    })
    .await?;
    let raw: Value = serde_json::from_str(
        text.trim_start_matches("```json").trim_matches('`').trim(),
    )
    .map_err(|_| {
        err(
            502,
            tr(
                &lang,
                "AI svarede i et uventet format. Prøv igen.",
                "AI answered in an unexpected format. Try again.",
            ),
        )
    })?;
    let steps = clean_steps(&raw, &today, &deadline);
    if steps.is_empty() {
        return Err(err(
            502,
            tr(
                &lang,
                "AI lavede ingen trin. Prøv igen med lidt flere detaljer.",
                "AI made no steps. Try again with a few more details.",
            ),
        ));
    }
    Ok(json!({ "steps": steps, "advice": raw["advice"].as_str().unwrap_or("") }))
}

/// What the chat helper knows about: today's date, open homework and note titles, plus the
/// full notes of the chosen subject.
pub fn chat_context(d: &Data, subject: &str, today: &str) -> String {
    let mut out = format!("Today is {today}.\n\nThe student's open homework and deadlines:\n");
    let mut open: Vec<_> = d.tasks.iter().filter(|t| t.status != "done").collect();
    open.sort_by(|a, b| a.deadline.cmp(&b.deadline));
    if open.is_empty() {
        out += "(none)\n";
    }
    for t in open.iter().take(40) {
        let overdue = if t.deadline.as_str() < today {
            ", OVERDUE"
        } else {
            ""
        };
        out += &format!(
            "- {} ({}) due {}{}, about {} min, priority {}\n",
            t.title, t.subject, t.deadline, overdue, t.estimated_minutes, t.priority
        );
    }
    let soon = add_days(today, 7);
    let done_soon = d
        .tasks
        .iter()
        .filter(|t| t.status == "done" && t.deadline.as_str() >= today && t.deadline <= soon)
        .count();
    out += &format!("({done_soon} tasks due this coming week are already done.)\n");
    if subject != "Generelt" && !subject.is_empty() {
        let notes: String = d
            .notes
            .iter()
            .filter(|n| n.subject == subject)
            .map(|n| format!("\n\n## {}\n{}", n.title, n.content))
            .collect();
        out += &format!(
            "\n\nThe student's own notes for {subject} (use them when relevant):{}",
            notes.chars().take(30_000).collect::<String>()
        );
    }
    out
}

const TUTOR: &str = "You are \"LektieHjælper\", a friendly, patient study buddy inside the app Kladde for a Danish upper-secondary student (gymnasium/HHX/HTX).
- Reply in the language the student writes in (usually Danish).
- Help them understand: explain step by step with examples. For homework, guide with hints and questions and let them do the key step, then check their work. If they ask for a full explanation or a worked example, give it.
- You can see their homework and deadlines; help plan their time and prioritise when asked. Be realistic and kind.
- Keep answers focused and reasonably short. Be honest when unsure.";

#[tauri::command]
pub fn ai_messages(state: tauri::State<'_, AppState>) -> Res<Vec<ChatMessage>> {
    Ok(state
        .store
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .data
        .messages
        .clone())
}

#[tauri::command]
pub fn ai_clear_chat(state: tauri::State<'_, AppState>) -> Res<Value> {
    let mut guard = state.store.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = guard.transaction();
    store.data.messages.clear();
    store.commit()?;
    Ok(json!({ "deleted": true }))
}

#[tauri::command]
pub async fn ai_chat(app: AppHandle, lang: Option<String>, body: Value) -> Res<ChatMessage> {
    let s = app.state::<AiState>().get();
    let provider_app = app.clone();
    let prompt = str_of(body.get("prompt"), 20_000).trim().to_string();
    let subject = match str_of(body.get("subject"), 60) {
        x if x.is_empty() => "Generelt".to_string(),
        x => x,
    };
    if prompt.is_empty() {
        return Err(err(
            400,
            tr(
                &lang,
                "Skriv et spørgsmål først.",
                "Write a question first.",
            ),
        ));
    }
    let provider = if s.use_codex { "codex" } else { "cloudflare" };
    let (system, contents) = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap_or_else(|p| p.into_inner());
        let today = local_date(&Local::now());
        let system = format!(
            "{TUTOR}\n- {PLAIN_TEXT}\n\n{}",
            chat_context(&store.data, &subject, &today)
        );
        let mut contents: Vec<Value> = vec![];
        let history = &store.data.messages;
        for m in history
            .iter()
            .skip(history.len().saturating_sub(20))
            .filter(|m| m.subject == subject && m.provider == provider)
        {
            if contents.is_empty() && m.role != "user" {
                continue;
            }
            let role = if m.role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            contents.push(json!({ "role": role, "content": m.content }));
        }
        contents.push(json!({ "role": "user", "content": prompt }));
        (system, contents)
    };
    let l = lang.clone();
    let answer = blocking(move || generate(&provider_app, &s, &system, contents, None, &l)).await?;
    let state = app.state::<AppState>();
    let mut guard = state.store.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = guard.transaction();
    store.data.messages.push(ChatMessage {
        provider: provider.into(),
        id: new_id(),
        role: "user".into(),
        content: prompt,
        subject: subject.clone(),
        created_at: now_iso(),
    });
    let reply = ChatMessage {
        provider: provider.into(),
        id: new_id(),
        role: "assistant".into(),
        content: answer,
        subject,
        created_at: now_iso(),
    };
    store.data.messages.push(reply.clone());
    let len = store.data.messages.len();
    if len > 200 {
        store.data.messages.drain(..len - 200);
    }
    store.commit()?;
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Task;

    #[test]
    fn cloudflare_setup_errors_distinguish_quota_and_credentials() {
        assert_eq!(cf_error(429, &None).status, 429);
        assert!(cf_error(403, &Some("en".into()))
            .detail
            .contains("permissions"));
        assert_eq!(cf_error(0, &None).status, 503);
    }

    #[test]
    fn cloudflare_transport_sends_authenticated_chat_and_reads_reply() {
        use std::io::{BufRead, BufReader, Read, Write};
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/ai/v1/chat/completions",
            socket.local_addr().unwrap()
        );
        let handler = std::thread::spawn(move || {
            let (mut stream, _) = socket.accept().unwrap();
            let mut reader = BufReader::new(&stream);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line.trim().is_empty() {
                    break;
                }
                headers.push_str(&line.to_lowercase());
            }
            assert!(headers.starts_with("post /ai/v1/chat/completions http/1.1"));
            assert!(headers.contains("authorization: bearer test-token"));
            let len = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["model"], CF_MODEL);
            assert_eq!(
                body["messages"][1],
                json!({"role":"user","content":"Say OK."})
            );
            let reply = r#"{"choices":[{"message":{"content":"OK"}}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                reply.len(),
                reply
            )
            .unwrap();
        });
        assert_eq!(
            cloudflare_at(
                &url,
                "test-token",
                "Reply briefly.",
                vec![json!({"role":"user","content":"Say OK."})],
                None,
                &None
            )
            .unwrap(),
            "OK"
        );
        handler.join().unwrap();
    }

    #[test]
    fn project_steps_are_cleaned() {
        let raw = json!({ "steps": [
            { "title": "Skriv indledning", "deadline": "2026-11-25", "estimated_minutes": 90, "priority": "high" },
            { "title": "Find kilder", "deadline": "2026-11-10", "estimated_minutes": 5, "priority": "urgent" },
            { "title": "Aflever", "deadline": "2027-01-01", "estimated_minutes": 30.4, "priority": "low" },
            { "title": "  ", "deadline": "2026-11-20" },
            { "title": "Rettelser", "deadline": "ikke en dato" },
        ]});
        let steps = clean_steps(&raw, "2026-11-18", "2026-11-30");
        let got: Vec<(&str, &str, i64, &str)> = steps
            .iter()
            .map(|s| {
                (
                    s.title.as_str(),
                    s.deadline.as_str(),
                    s.estimated_minutes,
                    s.priority.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("Find kilder", "2026-11-18", 10, "medium"),
                ("Skriv indledning", "2026-11-25", 90, "high"),
                ("Aflever", "2026-11-30", 30, "low"),
                ("Rettelser", "2026-11-30", 60, "medium"),
            ]
        );
    }

    #[test]
    fn chat_knows_deadlines_and_subject_notes() {
        let mut d = Data::default();
        d.tasks.push(Task {
            title: "Essay".into(),
            subject: "Engelsk".into(),
            deadline: "2026-11-17".into(),
            ..Task::default()
        });
        d.tasks.push(Task {
            title: "Færdig".into(),
            deadline: "2026-11-19".into(),
            status: "done".into(),
            ..Task::default()
        });
        d.notes.push(crate::store::Note {
            title: "Fotosyntese".into(),
            subject: "Biologi".into(),
            content: "Lys + CO2".into(),
            ..Default::default()
        });
        let general = chat_context(&d, "Generelt", "2026-11-18");
        assert!(general.contains("- Essay (Engelsk) due 2026-11-17, OVERDUE"));
        assert!(!general.contains("Færdig ("));
        assert!(!general.contains("Fotosyntese"));
        assert!(!chat_context(&d, "Dansk", "2026-11-18").contains("Fotosyntese"));
        assert!(!general.contains("Lys + CO2"));
        assert!(chat_context(&d, "Biologi", "2026-11-18").contains("Lys + CO2"));
    }

    #[test]
    fn settings_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let state = AiState::load(path.clone());
        state.update(|s| s.account = "a".repeat(32)).unwrap();
        let again = AiState::load(path);
        assert_eq!(again.get().account, "a".repeat(32));
        assert_eq!(status(&again.get())["provider"], "cloudflare");
    }
}
