//! AI features through the Google Gemini API.
//!
//! The API key is kept in settings.json next to kladde.json, never inside kladde.json, so it
//! is not part of backups. Nothing is sent to Google unless the user uses an AI feature.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::commands::{err, is_da, str_of, tr, ApiErr, AppState, Res};
use crate::store::{add_days, is_valid_date, local_date, new_id, now_iso, ChatMessage, Data};

const DEFAULT_MODEL: &str = "gemini-2.5-flash";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AiSettings {
    pub gemini_api_key: String,
    pub gemini_model: String,
}

pub struct AiState {
    pub path: PathBuf,
    pub settings: Mutex<AiSettings>,
}

impl AiState {
    pub fn load(path: PathBuf) -> AiState {
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        AiState { path, settings: Mutex::new(settings) }
    }

    fn get(&self) -> AiSettings {
        self.settings.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn save(&self, s: AiSettings) -> Result<(), String> {
        let mut settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        write_settings(&self.path, &s)?;
        *settings = s;
        Ok(())
    }
}

fn write_settings(path: &Path, s: &AiSettings) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| e.to_string())
}

fn base_url() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("KLADDE_GEMINI_BASE_URL") {
        return url;
    }
    "https://generativelanguage.googleapis.com".into()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(90)).user_agent("Kladde").build()
}

/// Turns a failed Gemini request into a friendly message (never containing the key).
pub fn gemini_error(status: Option<u16>, body: &str, lang: &Option<String>) -> ApiErr {
    let lower = body.to_lowercase();
    let (code, da, en) = match status {
        Some(400) if lower.contains("api key") || lower.contains("api_key") => {
            (401, "API-nøglen virker ikke. Tjek at den er kopieret helt (se AI-indstillinger).", "The API key does not work. Check that it was copied completely (see AI settings).")
        }
        Some(401) | Some(403) => (401, "API-nøglen har ikke adgang til Gemini. Lav en ny nøgle i Google AI Studio.", "The API key has no access to Gemini. Create a new key in Google AI Studio."),
        Some(404) => (404, "AI-modellen findes ikke længere. Vælg en anden model i AI-indstillinger.", "The AI model no longer exists. Choose another model in AI settings."),
        Some(429) => (429, "Googles kvote er nået. Tjek projektets grænser i AI Studio.", "Google's quota has been reached. Check the project limits in AI Studio."),
        Some(400) if lower.contains("location") || lower.contains("region") => (400, "Gemini er ikke tilgængelig i dit land med denne nøgle.", "Gemini is not available in your country with this key."),
        Some(400) => (400, "Gemini kunne ikke bruge forespørgslen. Prøv med en kortere tekst.", "Gemini could not use the request. Try a shorter text."),
        Some(s) if s >= 500 => (503, "Gemini er overbelastet lige nu. Prøv igen om lidt.", "Gemini is overloaded right now. Try again in a moment."),
        Some(_) => (502, "Gemini svarede med en fejl. Prøv igen.", "Gemini answered with an error. Try again."),
        None => (503, "Ingen forbindelse til Gemini. Tjek internettet.", "No connection to Gemini. Check the internet connection."),
    };
    err(code, tr(lang, da, en))
}

fn call(req: ureq::Request, body: Option<&Value>, lang: &Option<String>) -> Res<Value> {
    let result = match body {
        Some(b) => req.send_json(b.clone()),
        None => req.call(),
    };
    match result {
        Ok(resp) => resp.into_json::<Value>().map_err(|_| gemini_error(Some(502), "", lang)),
        Err(ureq::Error::Status(code, resp)) => Err(gemini_error(Some(code), &resp.into_string().unwrap_or_default(), lang)),
        Err(ureq::Error::Transport(_)) => Err(gemini_error(None, "", lang)),
    }
}

/// Gets the text out of a generateContent answer.
pub fn response_text(v: &Value, lang: &Option<String>) -> Res<String> {
    let parts = v["candidates"][0]["content"]["parts"].as_array();
    let text: String = parts
        .map(|ps| ps.iter().filter(|p| p["thought"] != json!(true)).filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    if text.trim().is_empty() {
        let blocked = v["promptFeedback"]["blockReason"].is_string()
            || matches!(v["candidates"][0]["finishReason"].as_str(), Some("SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "RECITATION"));
        return Err(if blocked {
            err(422, tr(lang, "Gemini ville ikke svare på det. Prøv at formulere det anderledes.", "Gemini declined to answer that. Try phrasing it differently."))
        } else {
            err(502, tr(lang, "Gemini sendte et tomt svar. Prøv igen.", "Gemini sent an empty answer. Try again."))
        });
    }
    Ok(text.trim().to_string())
}

/// One generateContent call. `contents` is the conversation; `schema` asks for JSON output.
fn generate(settings: &AiSettings, system: &str, contents: Vec<Value>, schema: Option<Value>, lang: &Option<String>) -> Res<String> {
    let model = if settings.gemini_model.is_empty() { DEFAULT_MODEL } else { &settings.gemini_model };
    let url = format!("{}/v1beta/models/{}:generateContent", base_url(), model);
    let mut body = json!({
        "systemInstruction": { "parts": [{ "text": system }] },
        "contents": contents,
    });
    if let Some(schema) = schema {
        body["generationConfig"] = json!({ "responseMimeType": "application/json", "responseSchema": schema });
    }
    let req = agent().post(&url).set("x-goog-api-key", &settings.gemini_api_key);
    let v = call(req, Some(&body), lang)?;
    response_text(&v, lang)
}

/// Picks the best general model from the list the key can use: the newest stable "flash".
pub fn choose_models(list: &Value) -> (Vec<String>, Option<String>) {
    let skip = ["image", "tts", "audio", "live", "embedding", "vision", "robotics", "computer", "aqa", "learnlm", "gemma"];
    let mut models: Vec<String> = list["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|m| m["supportedGenerationMethods"].as_array().is_some_and(|s| s.iter().any(|x| x == "generateContent")))
                .filter_map(|m| m["name"].as_str().map(|n| n.trim_start_matches("models/").to_string()))
                .filter(|n| n.starts_with("gemini-") && !skip.iter().any(|s| n.contains(s)))
                .collect()
        })
        .unwrap_or_default();
    let version = |n: &str| n.trim_start_matches("gemini-").split('-').next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let unstable = |n: &str| n.contains("preview") || n.contains("exp") || n.ends_with("-latest");
    models.sort_by(|a, b| version(b).partial_cmp(&version(a)).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b)));
    models.dedup();
    let best = models
        .iter()
        .find(|n| n.contains("flash") && !n.contains("lite") && !unstable(n))
        .or_else(|| models.iter().find(|n| n.contains("flash") && !unstable(n)))
        .or_else(|| models.iter().find(|n| n.contains("flash")))
        .or_else(|| models.iter().find(|n| !unstable(n)))
        .or_else(|| models.first())
        .cloned();
    (models, best)
}

fn list_models(key: &str, lang: &Option<String>) -> Res<Value> {
    let req = agent().get(&format!("{}/v1beta/models?pageSize=200", base_url())).set("x-goog-api-key", key);
    call(req, None, lang)
}

fn status_json(s: &AiSettings, models: Option<Vec<String>>) -> Value {
    let key = s.gemini_api_key.trim();
    let hint = if key.len() > 4 { format!("…{}", &key[key.len() - 4..]) } else { String::new() };
    json!({
        "configured": !key.is_empty(),
        "model": if s.gemini_model.is_empty() { DEFAULT_MODEL } else { &s.gemini_model },
        "key_hint": hint,
        "models": models,
    })
}

fn ai_state(app: &AppHandle) -> tauri::State<'_, AiState> {
    app.state::<AiState>()
}

fn need_key(app: &AppHandle, lang: &Option<String>) -> Res<AiSettings> {
    let s = ai_state(app).get();
    if s.gemini_api_key.trim().is_empty() {
        return Err(err(428, tr(lang, "AI er ikke sat op endnu. Indsæt din Gemini-nøgle under AI-hjælper.", "AI is not set up yet. Add your Gemini key under AI helper.")));
    }
    Ok(s)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| err(500, e.to_string()))?
}

// ---------- Commands ----------

#[tauri::command]
pub fn ai_status(app: AppHandle) -> Res<Value> {
    Ok(status_json(&ai_state(&app).get(), None))
}

/// Checks the key with Google, picks a model and saves both.
#[tauri::command]
pub async fn ai_save_key(app: AppHandle, lang: Option<String>, body: Value) -> Res<Value> {
    let key: String = str_of(body.get("key"), 200).trim().chars().filter(|c| !c.is_whitespace()).collect();
    if key.len() < 20 {
        return Err(err(400, tr(&lang, "Det ligner ikke en API-nøgle. Kopiér hele nøglen fra Google AI Studio.", "That does not look like an API key. Copy the whole key from Google AI Studio.")));
    }
    let l = lang.clone();
    let k = key.clone();
    let list = blocking(move || list_models(&k, &l)).await?;
    let (models, best) = choose_models(&list);
    let model = best.ok_or_else(|| err(422, tr(&lang, "Google viste ingen kompatibel tekstmodel til nøglen.", "Google listed no compatible text model for this key.")))?;
    let settings = AiSettings { gemini_api_key: key, gemini_model: model };
    ai_state(&app).save(settings.clone())?;
    Ok(status_json(&settings, Some(models)))
}

#[tauri::command]
pub async fn ai_models(app: AppHandle, lang: Option<String>) -> Res<Value> {
    let s = need_key(&app, &lang)?;
    let l = lang.clone();
    let list = blocking(move || list_models(&s.gemini_api_key, &l)).await?;
    let (models, _) = choose_models(&list);
    Ok(status_json(&ai_state(&app).get(), Some(models)))
}

#[tauri::command]
pub fn ai_set_model(app: AppHandle, body: Value) -> Res<Value> {
    let model = str_of(body.get("model"), 100);
    let state = ai_state(&app);
    let mut s = state.get();
    if model.starts_with("gemini-") {
        s.gemini_model = model;
        state.save(s.clone())?;
    }
    Ok(status_json(&s, None))
}

#[tauri::command]
pub fn ai_forget_key(app: AppHandle) -> Res<Value> {
    let state = ai_state(&app);
    let s = AiSettings::default();
    state.save(s.clone())?;
    Ok(status_json(&s, None))
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
    let s = need_key(&app, &lang)?;
    let action = str_of(body.get("action"), 20);
    let Some(task) = note_prompt(&action, is_da(&lang)) else {
        return Err(err(400, "Unknown action"));
    };
    let content = body["content"].as_str().unwrap_or("");
    if content.chars().count() > 60_000 {
        return Err(err(400, tr(&lang, "AI kan behandle op til 60.000 tegn ad gangen.", "AI can process up to 60,000 characters at a time.")));
    }
    if content.trim().len() < 20 {
        return Err(err(400, tr(&lang, "Skriv eller indsæt lidt mere tekst først.", "Write or paste a bit more text first.")));
    }
    let title = str_of(body.get("title"), 300);
    let subject = str_of(body.get("subject"), 60);
    let system = format!(
        "You help a Danish upper-secondary student (gymnasium/HHX/HTX) with their notes. {task} Write in the same language as the note (usually Danish). {PLAIN_TEXT}"
    );
    let user = format!("Subject: {subject}\nNote title: {title}\n\n<note>\n{content}\n</note>");
    let l = lang.clone();
    let text = blocking(move || generate(&s, &system, vec![json!({ "role": "user", "parts": [{ "text": user }] })], None, &l)).await?;
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
                    let title = s["title"].as_str()?.trim().chars().take(200).collect::<String>();
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
                    Some(PlanStep { title, deadline: date, estimated_minutes: minutes.clamp(10, 600), priority })
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
    let s = need_key(&app, &lang)?;
    let title = str_of(body.get("title"), 300).trim().to_string();
    let details = str_of(body.get("details"), 10_000);
    let subject = str_of(body.get("subject"), 60);
    let deadline = str_of(body.get("deadline"), 10);
    let today = local_date(&Local::now());
    if title.is_empty() || !is_valid_date(&deadline) {
        return Err(err(400, tr(&lang, "Skriv hvad projektet er, og vælg en afleveringsdato.", "Describe the project and choose a due date.")));
    }
    if deadline < today {
        return Err(err(400, tr(&lang, "Afleveringsdatoen er allerede passeret.", "The due date has already passed.")));
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
        "type": "OBJECT",
        "properties": {
            "steps": {
                "type": "ARRAY",
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "title": { "type": "STRING" },
                        "deadline": { "type": "STRING" },
                        "estimated_minutes": { "type": "INTEGER" },
                        "priority": { "type": "STRING", "enum": ["high", "medium", "low"] }
                    },
                    "required": ["title", "deadline", "estimated_minutes", "priority"]
                }
            },
            "advice": { "type": "STRING" }
        },
        "required": ["steps", "advice"]
    });
    let l = lang.clone();
    let text = blocking(move || generate(&s, &system, vec![json!({ "role": "user", "parts": [{ "text": user }] })], Some(schema), &l)).await?;
    let raw: Value = serde_json::from_str(text.trim_start_matches("```json").trim_matches('`').trim())
        .map_err(|_| err(502, tr(&lang, "Gemini svarede i et uventet format. Prøv igen.", "Gemini answered in an unexpected format. Try again.")))?;
    let steps = clean_steps(&raw, &today, &deadline);
    if steps.is_empty() {
        return Err(err(502, tr(&lang, "Gemini lavede ingen trin. Prøv igen med lidt flere detaljer.", "Gemini made no steps. Try again with a few more details.")));
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
        let overdue = if t.deadline.as_str() < today { ", OVERDUE" } else { "" };
        out += &format!("- {} ({}) due {}{}, about {} min, priority {}\n", t.title, t.subject, t.deadline, overdue, t.estimated_minutes, t.priority);
    }
    let soon = add_days(today, 7);
    let done_soon = d.tasks.iter().filter(|t| t.status == "done" && t.deadline.as_str() >= today && t.deadline <= soon).count();
    out += &format!("({done_soon} tasks due this coming week are already done.)\n");
    if subject != "Generelt" && !subject.is_empty() {
        let notes: String = d
            .notes
            .iter()
            .filter(|n| n.subject == subject)
            .map(|n| format!("\n\n## {}\n{}", n.title, n.content))
            .collect();
        out += &format!("\n\nThe student's own notes for {subject} (use them when relevant):{}", notes.chars().take(30_000).collect::<String>());
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
    Ok(state.store.lock().unwrap_or_else(|p| p.into_inner()).data.messages.clone())
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
    let s = need_key(&app, &lang)?;
    let prompt = str_of(body.get("prompt"), 20_000).trim().to_string();
    let subject = match str_of(body.get("subject"), 60) {
        x if x.is_empty() => "Generelt".to_string(),
        x => x,
    };
    if prompt.is_empty() {
        return Err(err(400, tr(&lang, "Skriv et spørgsmål først.", "Write a question first.")));
    }
    let (system, contents) = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap_or_else(|p| p.into_inner());
        let today = local_date(&Local::now());
        let system = format!("{TUTOR}\n- {PLAIN_TEXT}\n\n{}", chat_context(&store.data, &subject, &today));
        let mut contents: Vec<Value> = vec![];
        let history = &store.data.messages;
        for m in history.iter().skip(history.len().saturating_sub(20)).filter(|m| m.subject == subject) {
            if contents.is_empty() && m.role != "user" {
                continue;
            }
            let role = if m.role == "assistant" { "model" } else { "user" };
            contents.push(json!({ "role": role, "parts": [{ "text": m.content }] }));
        }
        contents.push(json!({ "role": "user", "parts": [{ "text": prompt }] }));
        (system, contents)
    };
    let l = lang.clone();
    let answer = blocking(move || generate(&s, &system, contents, None, &l)).await?;
    let state = app.state::<AppState>();
    let mut guard = state.store.lock().unwrap_or_else(|p| p.into_inner());
    let mut store = guard.transaction();
    store.data.messages.push(ChatMessage { id: new_id(), role: "user".into(), content: prompt, subject: subject.clone(), created_at: now_iso() });
    let reply = ChatMessage { id: new_id(), role: "assistant".into(), content: answer, subject, created_at: now_iso() };
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
    fn picks_newest_stable_flash_model() {
        let list = json!({ "models": [
            { "name": "models/gemini-2.0-flash", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-2.5-flash", "supportedGenerationMethods": ["generateContent", "countTokens"] },
            { "name": "models/gemini-2.5-flash-lite", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-3-flash-preview", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-2.5-pro", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/gemini-2.5-flash-image", "supportedGenerationMethods": ["generateContent"] },
            { "name": "models/text-embedding-004", "supportedGenerationMethods": ["embedContent"] },
        ]});
        let (models, best) = choose_models(&list);
        assert_eq!(best.as_deref(), Some("gemini-2.5-flash"));
        assert_eq!(models[0], "gemini-3-flash-preview");
        assert!(!models.iter().any(|m| m.contains("image") || m.contains("embedding")));
        let (_, best) = choose_models(&json!({ "models": [{ "name": "models/gemini-3.1-flash", "supportedGenerationMethods": ["generateContent"] }, { "name": "models/gemini-2.5-flash", "supportedGenerationMethods": ["generateContent"] }] }));
        assert_eq!(best.as_deref(), Some("gemini-3.1-flash"));
        assert_eq!(choose_models(&json!({})).1, None);
        let pro_only = json!({ "models": [{"name": "models/gemini-3-pro", "supportedGenerationMethods": ["generateContent"]}] });
        assert_eq!(choose_models(&pro_only).1.as_deref(), Some("gemini-3-pro"));
    }

    #[test]
    fn reads_answers_and_explains_problems() {
        let da = Some("da".to_string());
        let ok = json!({ "candidates": [{ "content": { "parts": [{ "text": "tænker", "thought": true }, { "text": "Hej " }, { "text": "med dig" }] } }] });
        assert_eq!(response_text(&ok, &da).unwrap(), "Hej med dig");
        let blocked = json!({ "promptFeedback": { "blockReason": "SAFETY" } });
        assert_eq!(response_text(&blocked, &da).unwrap_err().status, 422);
        assert_eq!(response_text(&json!({ "candidates": [] }), &da).unwrap_err().status, 502);
        assert!(gemini_error(Some(400), "API key not valid. Please pass a valid API key.", &da).detail.contains("API-nøglen virker ikke"));
        assert_eq!(gemini_error(Some(429), "RESOURCE_EXHAUSTED", &da).status, 429);
        assert!(gemini_error(Some(429), "", &Some("en".into())).detail.contains("quota"));
        assert_eq!(gemini_error(None, "", &da).status, 503);
        assert_eq!(gemini_error(Some(503), "", &da).status, 503);
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
        let got: Vec<(&str, &str, i64, &str)> = steps.iter().map(|s| (s.title.as_str(), s.deadline.as_str(), s.estimated_minutes, s.priority.as_str())).collect();
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
        d.tasks.push(Task { title: "Essay".into(), subject: "Engelsk".into(), deadline: "2026-11-17".into(), ..Task::default() });
        d.tasks.push(Task { title: "Færdig".into(), deadline: "2026-11-19".into(), status: "done".into(), ..Task::default() });
        d.notes.push(crate::store::Note { title: "Fotosyntese".into(), subject: "Biologi".into(), content: "Lys + CO2".into(), ..Default::default() });
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
        assert!(state.get().gemini_api_key.is_empty());
        state.save(AiSettings { gemini_api_key: "AIza-test-key-1234567890".into(), gemini_model: "gemini-2.5-flash".into() }).unwrap();
        let again = AiState::load(path);
        assert_eq!(again.get().gemini_model, "gemini-2.5-flash");
        assert_eq!(status_json(&again.get(), None)["key_hint"], "…7890");
        assert!(note_prompt("summary", true).is_some() && note_prompt("hack", true).is_none());
    }
}
