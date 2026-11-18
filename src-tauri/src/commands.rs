//! The commands the frontend calls (they replace the old HTTP routes one to one).
//! Errors come back as { status, detail } so the frontend shows the same messages as before.

use std::sync::Mutex;

use chrono::Local;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::feeds;
use crate::importers::{clean_subject, js_round, parse_import};
use crate::planner::{make_study_plan, weekly_summary, StudyPlan, WeeklySummary};
use crate::store::{
    add_days, day_name, local_date, new_id, now_iso, to_pretty_json, Block, CalendarFeed, Note, Store, Subject, Task,
};

pub struct AppState {
    pub store: Mutex<Store>,
}

#[derive(Serialize, Debug)]
pub struct ApiErr {
    pub status: u16,
    pub detail: String,
}

pub type Res<T> = Result<T, ApiErr>;

pub fn err(status: u16, detail: impl Into<String>) -> ApiErr {
    ApiErr { status, detail: detail.into() }
}

impl From<String> for ApiErr {
    fn from(detail: String) -> Self {
        err(500, detail)
    }
}

pub fn is_da(lang: &Option<String>) -> bool {
    lang.as_deref() != Some("en")
}

pub fn tr(lang: &Option<String>, da: &str, en: &str) -> String {
    if is_da(lang) { da } else { en }.to_string()
}

fn not_found(lang: &Option<String>) -> ApiErr {
    err(404, tr(lang, "Findes ikke (længere).", "Not found."))
}

/// Text of a field, cut to `max` characters; anything that is not a string counts as "".
pub fn str_of(v: Option<&Value>, max: usize) -> String {
    match v {
        Some(Value::String(s)) => s.chars().take(max).collect(),
        _ => String::new(),
    }
}

fn field<'a>(body: &'a Value, key: &str) -> Option<&'a Value> {
    body.as_object().and_then(|o| o.get(key))
}

fn has(body: &Value, key: &str) -> bool {
    field(body, key).is_some()
}

/// Truthiness as in JavaScript.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Number(value) as in JavaScript (NaN when it is not a number).
pub fn js_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse().unwrap_or(f64::NAN)
            }
        }
        Some(_) => f64::NAN,
    }
}

pub fn is_date(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

fn is_hhmm(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 5 && b[2] == b':' && [0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit())
}

fn lock<'a>(state: &'a State<'_, AppState>) -> std::sync::MutexGuard<'a, Store> {
    state.store.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------- Overview ----------

#[derive(Serialize)]
pub struct Dashboard {
    today: String,
    notes: Vec<Note>,
    tasks: Vec<Task>,
    subjects: Vec<Subject>,
    study_blocks: Vec<Block>,
    reminders: Vec<Task>,
}

pub fn sorted_tasks(tasks: &[Task]) -> Vec<Task> {
    let mut out = tasks.to_vec();
    out.sort_by(|a, b| {
        let (ad, bd) = (a.status == "done", b.status == "done");
        if ad != bd {
            return ad.cmp(&bd);
        }
        if ad {
            return b.completed_at.as_deref().unwrap_or("").cmp(a.completed_at.as_deref().unwrap_or(""));
        }
        a.deadline.cmp(&b.deadline)
    });
    out
}

#[tauri::command]
pub fn dashboard(state: State<'_, AppState>) -> Res<Dashboard> {
    let store = lock(&state);
    let d = &store.data;
    let today = local_date(&Local::now());
    let tomorrow = add_days(&today, 1);
    let mut notes = d.notes.clone();
    notes.sort_by(|a, b| b.is_pinned.cmp(&a.is_pinned).then(b.updated_at.cmp(&a.updated_at)));
    let tasks = sorted_tasks(&d.tasks);
    let reminders =
        tasks.iter().filter(|t| t.status != "done" && t.deadline >= today && t.deadline <= tomorrow).cloned().collect();
    Ok(Dashboard { today, notes, tasks, subjects: d.subjects.clone(), study_blocks: d.blocks.clone(), reminders })
}

#[tauri::command]
pub fn weekly_summary_cmd(state: State<'_, AppState>) -> Res<WeeklySummary> {
    Ok(weekly_summary(&lock(&state).data, &Local::now()))
}

/// "Download backup": asks where to save (starting in Documents) and writes a copy of the data.
/// Returns the saved path, or null when the user cancelled.
#[tauri::command]
pub async fn export_backup(app: AppHandle, state: State<'_, AppState>, lang: Option<String>) -> Res<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let today = local_date(&Local::now());
    let mut dialog = app
        .dialog()
        .file()
        .set_title(tr(&lang, "Gem backup af Kladde", "Save a backup of Kladde"))
        .set_file_name(format!("kladde-backup-{today}.json"))
        .add_filter(tr(&lang, "Kladde backup", "Kladde backup"), &["json"]);
    if let Ok(docs) = app.path().document_dir() {
        dialog = dialog.set_directory(docs);
    }
    let Some(chosen) = dialog.blocking_save_file() else { return Ok(None) };
    let path = chosen.into_path().map_err(|e| err(500, e.to_string()))?;
    let json = to_pretty_json(&lock(&state).data)?;
    std::fs::write(&path, json).map_err(|e| {
        err(500, format!("{} {}: {e}", tr(&lang, "Kunne ikke gemme", "Could not save"), path.display()))
    })?;
    Ok(Some(path.display().to_string()))
}

// ---------- Subjects ----------

#[tauri::command]
pub fn create_subject(state: State<'_, AppState>, lang: Option<String>, body: Value) -> Res<Subject> {
    let name = str_of(field(&body, "name"), 60).trim().to_string();
    if name.is_empty() {
        return Err(err(400, tr(&lang, "Faget skal have et navn.", "The subject needs a name.")));
    }
    let mut store = lock(&state);
    let (subject, _) = store.data.ensure_subject(&name);
    store.save()?;
    Ok(subject)
}

// ---------- Notes ----------

#[derive(Default)]
struct NoteFields {
    title: Option<String>,
    content: Option<String>,
    subject: Option<Subject>,
    is_pinned: Option<bool>,
}

fn note_fields(store: &mut Store, lang: &Option<String>, b: &Value) -> NoteFields {
    let mut out = NoteFields::default();
    if has(b, "title") {
        let t = str_of(field(b, "title"), 300).trim().to_string();
        out.title = Some(if t.is_empty() { tr(lang, "Uden titel", "Untitled") } else { t });
    }
    if has(b, "content") {
        out.content = Some(str_of(field(b, "content"), 200_000));
    }
    if has(b, "subject") && !str_of(field(b, "subject"), 200_000).trim().is_empty() {
        out.subject = Some(store.data.ensure_subject(&str_of(field(b, "subject"), 60)).0);
    }
    if has(b, "is_pinned") {
        out.is_pinned = Some(truthy(field(b, "is_pinned")));
    }
    out
}

fn apply_note(note: &mut Note, f: NoteFields) {
    if let Some(t) = f.title {
        note.title = t;
    }
    if let Some(c) = f.content {
        note.content = c;
    }
    if let Some(s) = f.subject {
        note.subject = s.name;
        note.subject_key = s.key;
    }
    if let Some(p) = f.is_pinned {
        note.is_pinned = p;
    }
}

#[tauri::command]
pub fn create_note(state: State<'_, AppState>, lang: Option<String>, body: Value) -> Res<Note> {
    let mut store = lock(&state);
    let now = now_iso();
    let mut note = Note {
        id: new_id(),
        title: tr(&lang, "Uden titel", "Untitled"),
        content: String::new(),
        subject: "Dansk".into(),
        subject_key: "dansk".into(),
        is_pinned: false,
        created_at: now.clone(),
        updated_at: now,
    };
    let f = note_fields(&mut store, &lang, &body);
    apply_note(&mut note, f);
    store.data.notes.push(note.clone());
    store.save()?;
    Ok(note)
}

#[tauri::command]
pub fn update_note(state: State<'_, AppState>, lang: Option<String>, id: String, body: Value) -> Res<Note> {
    let mut store = lock(&state);
    if !store.data.notes.iter().any(|n| n.id == id) {
        return Err(not_found(&lang));
    }
    let f = note_fields(&mut store, &lang, &body);
    // Pinning alone does not count as editing the note.
    let only_pin = f.title.is_none() && f.content.is_none() && f.subject.is_none();
    let note = store.data.notes.iter_mut().find(|n| n.id == id).expect("checked above");
    apply_note(note, f);
    if !only_pin {
        note.updated_at = now_iso();
    }
    let note = note.clone();
    store.save()?;
    Ok(note)
}

#[tauri::command]
pub fn delete_note(state: State<'_, AppState>, lang: Option<String>, id: String) -> Res<Value> {
    let mut store = lock(&state);
    let before = store.data.notes.len();
    store.data.notes.retain(|n| n.id != id);
    if store.data.notes.len() == before {
        return Err(not_found(&lang));
    }
    store.save()?;
    Ok(json!({ "deleted": true }))
}

// ---------- Tasks ----------

#[derive(Default)]
struct TaskFields {
    title: Option<String>,
    deadline: Option<String>,
    priority: Option<String>,
    estimated_minutes: Option<i64>,
    subject: Option<Subject>,
    status: Option<String>,
}

fn task_fields(store: &mut Store, b: &Value) -> TaskFields {
    let mut out = TaskFields::default();
    if has(b, "title") {
        out.title = Some(str_of(field(b, "title"), 300).trim().to_string());
    }
    if let Some(Value::String(d)) = field(b, "deadline") {
        if is_date(d) {
            out.deadline = Some(d.clone());
        }
    }
    if let Some(Value::String(p)) = field(b, "priority") {
        if ["high", "medium", "low"].contains(&p.as_str()) {
            out.priority = Some(p.clone());
        }
    }
    if has(b, "estimated_minutes") {
        let n = js_number(field(b, "estimated_minutes"));
        let n = if n == 0.0 || n.is_nan() { 45.0 } else { n };
        out.estimated_minutes = Some(js_round(n).clamp(5, 1440));
    }
    if has(b, "subject") && !str_of(field(b, "subject"), 200_000).trim().is_empty() {
        out.subject = Some(store.data.ensure_subject(&str_of(field(b, "subject"), 60)).0);
    }
    if let Some(Value::String(s)) = field(b, "status") {
        if ["todo", "doing", "done"].contains(&s.as_str()) {
            out.status = Some(s.clone());
        }
    }
    out
}

fn apply_task(task: &mut Task, f: TaskFields) {
    if let Some(t) = f.title {
        task.title = t;
    }
    if let Some(d) = f.deadline {
        task.deadline = d;
    }
    if let Some(p) = f.priority {
        task.priority = p;
    }
    if let Some(m) = f.estimated_minutes {
        task.estimated_minutes = m;
    }
    if let Some(s) = f.subject {
        task.subject = s.name;
        task.subject_key = s.key;
    }
    if let Some(s) = f.status {
        task.status = s;
    }
}

#[tauri::command]
pub fn create_task(state: State<'_, AppState>, lang: Option<String>, body: Value) -> Res<Task> {
    let mut store = lock(&state);
    let f = task_fields(&mut store, &body);
    if f.title.as_deref().unwrap_or("").is_empty() {
        return Err(err(400, tr(&lang, "Opgaven skal have en titel.", "The task needs a title.")));
    }
    let mut task = Task {
        id: new_id(),
        deadline: local_date(&Local::now()),
        created_at: now_iso(),
        ..Task::default()
    };
    apply_task(&mut task, f);
    store.data.tasks.push(task.clone());
    store.save()?;
    Ok(task)
}

#[tauri::command]
pub fn update_task(state: State<'_, AppState>, lang: Option<String>, id: String, body: Value) -> Res<Task> {
    let mut store = lock(&state);
    if !store.data.tasks.iter().any(|t| t.id == id) {
        return Err(not_found(&lang));
    }
    let mut f = task_fields(&mut store, &body);
    if f.title.as_deref() == Some("") {
        f.title = None;
    }
    let task = store.data.tasks.iter_mut().find(|t| t.id == id).expect("checked above");
    if let Some(status) = &f.status {
        if *status != task.status {
            task.completed_at = (status == "done").then(now_iso);
        }
    }
    apply_task(task, f);
    let task = task.clone();
    store.save()?;
    Ok(task)
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, lang: Option<String>, id: String) -> Res<Value> {
    let mut store = lock(&state);
    let before = store.data.tasks.len();
    store.data.tasks.retain(|t| t.id != id);
    if store.data.tasks.len() == before {
        return Err(not_found(&lang));
    }
    store.data.blocks.retain(|b| !(b.source.as_deref() == Some("plan") && b.external_id.as_deref() == Some(id.as_str())));
    store.save()?;
    Ok(json!({ "deleted": true }))
}

// ---------- Planner blocks ----------

#[tauri::command]
pub fn create_block(state: State<'_, AppState>, lang: Option<String>, body: Value) -> Res<Block> {
    let b = &body;
    let label = str_of(field(b, "label"), 200).trim().to_string();
    let day = str_of(field(b, "day"), 20);
    let time = str_of(field(b, "time"), 5);
    let days = ["Mandag", "Tirsdag", "Onsdag", "Torsdag", "Fredag", "Lørdag", "Søndag"];
    if label.is_empty() || !days.contains(&day.as_str()) || !is_hhmm(&time) {
        return Err(err(400, tr(&lang, "Udfyld titel, dag og tidspunkt.", "Fill in title, day and time.")));
    }
    let mut store = lock(&state);
    let subject_name = match str_of(field(b, "subject"), 60) {
        s if s.is_empty() => "Selvstudie".to_string(),
        s => s,
    };
    let (subject, _) = store.data.ensure_subject(&subject_name);
    let minutes = js_number(field(b, "minutes"));
    let block = Block {
        id: new_id(),
        day,
        time,
        label,
        subject: subject.name,
        color: subject.color,
        source: Some("manual".into()),
        minutes: (minutes > 0.0).then(|| js_round(minutes).min(600)),
        external_id: None,
    };
    store.data.blocks.push(block.clone());
    store.save()?;
    Ok(block)
}

#[tauri::command]
pub fn delete_block(state: State<'_, AppState>, lang: Option<String>, id: String) -> Res<Value> {
    let mut store = lock(&state);
    let before = store.data.blocks.len();
    store.data.blocks.retain(|b| b.id != id);
    if store.data.blocks.len() == before {
        return Err(not_found(&lang));
    }
    store.save()?;
    Ok(json!({ "deleted": true }))
}

// ---------- Study plan ----------

#[tauri::command]
pub fn make_study_plan_cmd(state: State<'_, AppState>, lang: Option<String>) -> Res<StudyPlan> {
    let mut store = lock(&state);
    let plan = make_study_plan(&store.data, is_da(&lang), &Local::now()).ok_or_else(|| {
        err(409, tr(&lang, "Ingen åbne deadlines de næste 14 dage.", "No open deadlines in the next 14 days."))
    })?;
    store.data.blocks.retain(|b| b.source.as_deref() != Some("plan"));
    store.data.blocks.extend(plan.blocks.iter().cloned());
    store.save()?;
    Ok(plan)
}

#[tauri::command]
pub fn clear_study_plan(state: State<'_, AppState>) -> Res<Value> {
    let mut store = lock(&state);
    let before = store.data.blocks.len();
    store.data.blocks.retain(|b| b.source.as_deref() != Some("plan"));
    let deleted = before - store.data.blocks.len();
    store.save()?;
    Ok(json!({ "deleted": deleted }))
}

// ---------- School import ----------

#[tauri::command]
pub fn import_preview(state: State<'_, AppState>, lang: Option<String>, body: Value) -> Res<Value> {
    let source = match str_of(field(&body, "source"), 20).as_str() {
        s @ ("lectio" | "ludus" | "mitnielsbrock") => s.to_string(),
        _ => "lectio".to_string(),
    };
    let content = str_of(field(&body, "content"), 600_000);
    if content.chars().count() > 500_000 {
        return Err(err(
            413,
            tr(&lang, "Teksten er for lang (maks. 500.000 tegn).", "The text is too long (max 500,000 characters)."),
        ));
    }
    let mut result = parse_import(&source, &content, is_da(&lang)).map_err(|m| err(400, m))?;
    // Already-imported rows are shown but not ticked.
    let store = lock(&state);
    let known: std::collections::HashSet<&str> = store
        .data
        .tasks
        .iter()
        .filter_map(|t| t.external_id.as_deref())
        .chain(store.data.blocks.iter().filter_map(|b| b.external_id.as_deref()))
        .collect();
    let mut dupes = 0;
    for item in result.items.iter_mut() {
        if item.external_id.as_deref().is_some_and(|e| known.contains(e)) {
            item.selected = false;
            dupes += 1;
        }
    }
    if dupes > 0 {
        result.warnings.push(tr(
            &lang,
            &format!("{dupes} findes allerede og er ikke valgt."),
            &format!("{dupes} already exist and are not selected."),
        ));
    }
    let label = match source.as_str() {
        "lectio" => "Lectio",
        "ludus" => "Ludus",
        _ => "Mit Niels Brock",
    };
    let mut out = serde_json::to_value(&result).map_err(|e| err(500, e.to_string()))?;
    out["source_label"] = json!(label);
    Ok(out)
}

pub fn minutes_between(start: &str, end: &str) -> Option<i64> {
    let to_min = |t: &str| is_hhmm(t).then(|| t[..2].parse::<i64>().unwrap() * 60 + t[3..].parse::<i64>().unwrap());
    match (to_min(start), to_min(end)) {
        (Some(a), Some(b)) if b > a => Some(b - a),
        _ => None,
    }
}

#[tauri::command]
pub fn import_commit(state: State<'_, AppState>, lang: Option<String>, body: Value) -> Res<Value> {
    let items: Vec<Value> = field(&body, "items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut store = lock(&state);
    let today = local_date(&Local::now());
    let (mut tasks, mut blocks, mut subjects, mut skipped) = (0, 0, 0, 0);
    for item in items.iter().filter(|i| truthy(field(i, "selected"))) {
        let title = str_of(field(item, "title"), 300).trim().to_string();
        if title.is_empty() {
            continue;
        }
        let (subject, created) = store.data.ensure_subject(&clean_subject(&str_of(field(item, "subject"), 60)));
        if created {
            subjects += 1;
        }
        let ext = Some(str_of(field(item, "external_id"), 64)).filter(|e| !e.is_empty());
        let kind = str_of(field(item, "kind"), 20);
        let date = str_of(field(item, "date"), 20);
        if kind == "deadline" {
            let deadline = if is_date(&date) { date } else { today.clone() };
            let d = &store.data;
            if ext.as_ref().is_some_and(|e| d.tasks.iter().any(|t| t.external_id.as_ref() == Some(e)))
                || d.tasks.iter().any(|t| t.title == title && t.deadline == deadline)
            {
                skipped += 1;
                continue;
            }
            let est = js_number(field(item, "estimated_minutes"));
            let past = deadline < today;
            store.data.tasks.push(Task {
                id: new_id(),
                title,
                subject: subject.name,
                subject_key: subject.key,
                status: if past { "done" } else { "todo" }.into(),
                priority: if deadline <= add_days(&today, 3) { "high" } else { "medium" }.into(),
                estimated_minutes: if est > 0.0 { js_round(est) } else { 60 },
                created_at: now_iso(),
                completed_at: past.then(now_iso),
                external_id: ext,
                deadline,
            });
            tasks += 1;
        } else if kind == "lesson" {
            let day = match str_of(field(item, "day"), 20) {
                d if !d.is_empty() => Some(d),
                _ => is_date(&date).then(|| day_name(&date)).flatten().map(String::from),
            };
            let time = match str_of(field(item, "time"), 5) {
                t if t.is_empty() => "08:00".to_string(),
                t => t,
            };
            let Some(day) = day else {
                skipped += 1;
                continue;
            };
            if store.data.blocks.iter().any(|b| b.day == day && b.time == time && b.label == title) {
                skipped += 1;
                continue;
            }
            let minutes = minutes_between(&time, &str_of(field(item, "end_time"), 5));
            store.data.blocks.push(Block {
                id: new_id(),
                day,
                time,
                label: title,
                subject: subject.name,
                color: subject.color,
                source: Some("import".into()),
                minutes,
                external_id: ext,
            });
            blocks += 1;
        }
    }
    store.save()?;
    let message = if is_da(&lang) {
        format!(
            "Importeret: {tasks} deadlines, {blocks} lektioner{}{}",
            if subjects > 0 { format!(", {subjects} nye fag") } else { String::new() },
            if skipped > 0 { format!(" · {skipped} dubletter sprunget over") } else { String::new() }
        )
    } else {
        format!(
            "Imported: {tasks} deadlines, {blocks} lessons{}{}",
            if subjects > 0 { format!(", {subjects} new subjects") } else { String::new() },
            if skipped > 0 { format!(" · {skipped} duplicates skipped") } else { String::new() }
        )
    };
    Ok(json!({
        "imported_tasks": tasks,
        "imported_blocks": blocks,
        "imported_subjects": subjects,
        "skipped_duplicates": skipped,
        "message": message,
    }))
}

// ---------- Calendar links ----------

#[tauri::command]
pub fn list_feeds(state: State<'_, AppState>) -> Res<Vec<Value>> {
    Ok(lock(&state).data.feeds.iter().map(feeds::feed_out).collect())
}

#[tauri::command]
pub async fn add_feed(app: AppHandle, lang: Option<String>, body: Value) -> Res<Value> {
    let url = feeds::normalize_url(str_of(field(&body, "url"), 2000).trim());
    if url::Url::parse(&url).is_err() {
        return Err(err(400, tr(&lang, "Det ligner ikke et gyldigt link.", "That does not look like a valid link.")));
    }
    let id = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().unwrap_or_else(|p| p.into_inner());
        if store.data.feeds.iter().any(|f| f.url == url) {
            return Err(err(409, tr(&lang, "Det link er allerede tilføjet.", "That link is already added.")));
        }
        let label = match str_of(field(&body, "label"), 80).trim() {
            "" => tr(&lang, "Skolekalender", "School calendar"),
            l => l.to_string(),
        };
        let feed = CalendarFeed {
            id: new_id(),
            label,
            url,
            created_at: now_iso(),
            last_synced_at: None,
            last_status: "pending".into(),
            last_error: None,
            imported_tasks: 0,
            imported_blocks: 0,
        };
        let id = feed.id.clone();
        store.data.feeds.push(feed);
        id
    };
    feeds::sync_feed(app, id, is_da(&lang)).await.ok_or_else(|| not_found(&lang))
}

#[tauri::command]
pub async fn sync_feed(app: AppHandle, lang: Option<String>, id: String) -> Res<Value> {
    feeds::sync_feed(app, id, is_da(&lang)).await.ok_or_else(|| not_found(&lang))
}

#[tauri::command]
pub fn delete_feed(state: State<'_, AppState>, lang: Option<String>, id: String) -> Res<Value> {
    let mut store = lock(&state);
    let before = store.data.feeds.len();
    store.data.feeds.retain(|f| f.id != id);
    if store.data.feeds.len() == before {
        return Err(not_found(&lang));
    }
    // Its lessons go; its deadlines stay (they may already be worked on).
    let source = format!("feed:{id}");
    store.data.blocks.retain(|b| b.source.as_deref() != Some(source.as_str()));
    store.save()?;
    Ok(json!({ "deleted": true }))
}
