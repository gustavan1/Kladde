//! The data file (kladde.json) and its types.
//!
//! The format is exactly the one the earlier Node version wrote, so existing data files and
//! backups keep working. Unknown top-level keys are kept as they are, so nothing is lost if a
//! file comes from a newer or older version.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Subject {
    pub id: String,
    pub name: String,
    pub key: String,
    pub color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub content: String,
    pub subject: String,
    pub subject_key: String,
    pub is_pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub subject: String,
    pub subject_key: String,
    pub deadline: String,
    /// "todo" | "doing" | "done"
    pub status: String,
    /// "high" | "medium" | "low"
    pub priority: String,
    pub estimated_minutes: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub external_id: Option<String>,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            id: String::new(),
            title: String::new(),
            subject: "Dansk".into(),
            subject_key: "dansk".into(),
            deadline: String::new(),
            status: "todo".into(),
            priority: "medium".into(),
            estimated_minutes: 45,
            created_at: String::new(),
            completed_at: None,
            external_id: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Block {
    pub id: String,
    /// Danish weekday name, e.g. "Mandag".
    pub day: String,
    /// "HH:MM"
    pub time: String,
    pub label: String,
    pub subject: String,
    pub color: String,
    /// null (old data), "manual", "import", "plan" or "feed:<id>"
    pub source: Option<String>,
    pub minutes: Option<i64>,
    pub external_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct CalendarFeed {
    pub id: String,
    pub label: String,
    pub url: String,
    pub created_at: String,
    pub last_synced_at: Option<String>,
    pub last_status: String,
    pub last_error: Option<String>,
    pub imported_tasks: i64,
    pub imported_blocks: i64,
}

/// One message in the AI helper chat.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ChatMessage {
    pub id: String,
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
    pub subject: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Data {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_subjects")]
    pub subjects: Vec<Subject>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub feeds: Vec<CalendarFeed>,
    /// The AI helper chat (kept so a conversation continues after a restart).
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    /// Anything else in the file is kept untouched.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_version() -> u32 {
    1
}

const DEFAULT_SUBJECTS: [(&str, &str); 7] = [
    ("Dansk", "blue"),
    ("Matematik", "indigo"),
    ("Engelsk", "emerald"),
    ("Samfundsfag", "amber"),
    ("Biologi", "lime"),
    ("Historie", "rose"),
    ("Selvstudie", "sky"),
];
const COLORS: [&str; 10] = [
    "blue", "indigo", "emerald", "amber", "lime", "rose", "sky", "violet", "teal", "orange",
];

fn default_subjects() -> Vec<Subject> {
    DEFAULT_SUBJECTS
        .iter()
        .map(|(name, color)| Subject {
            id: new_id(),
            name: (*name).into(),
            key: subject_key(name),
            color: (*color).into(),
        })
        .collect()
}

impl Default for Data {
    fn default() -> Self {
        Data {
            version: 1,
            subjects: default_subjects(),
            notes: vec![],
            tasks: vec![],
            blocks: vec![],
            feeds: vec![],
            messages: vec![],
            extra: Map::new(),
        }
    }
}

impl Data {
    /// Returns the subject with this name, creating it if it is new. The bool is true when created.
    pub fn ensure_subject(&mut self, name: &str) -> (Subject, bool) {
        let clean = match name.trim() {
            "" => "Selvstudie",
            s => s,
        };
        let key = subject_key(clean);
        if let Some(found) = self.subjects.iter().find(|s| s.key == key) {
            return (found.clone(), false);
        }
        let subject = Subject {
            id: new_id(),
            name: clean.to_string(),
            key,
            color: COLORS[self.subjects.len() % COLORS.len()].to_string(),
        };
        self.subjects.push(subject.clone());
        (subject, true)
    }
}

/// Owns the data and the file it lives in.
pub struct Store {
    path: PathBuf,
    pub data: Data,
}

impl Store {
    /// Opens the data file, creating it (with the default subjects) if it does not exist yet.
    /// A file that cannot be read is never overwritten: an error is returned instead.
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("Kunne ikke oprette mappen {}: {e}", dir.display()))?;
        }
        if path.exists() {
            let text = fs::read_to_string(path).map_err(|e| format!("Kunne ikke læse {}: {e}", path.display()))?;
            let mut value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("Datafilen {} er ikke gyldig JSON ({e}).", path.display()))?;
            tidy(&mut value);
            let data: Data = serde_json::from_value(value)
                .map_err(|e| format!("Datafilen {} har et uventet indhold ({e}).", path.display()))?;
            Ok(Store { path: path.to_path_buf(), data })
        } else {
            let store = Store { path: path.to_path_buf(), data: Data::default() };
            store.save()?;
            Ok(store)
        }
    }

    /// Writes the file safely: first to kladde.json.tmp, then renamed over the real file, so a
    /// crash or power cut never leaves a half-written data file.
    pub fn save(&self) -> Result<(), String> {
        let json = to_pretty_json(&self.data)?;
        let tmp = self.path.with_extension("json.tmp");
        let write = || -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, &self.path)
        };
        write().map_err(|e| format!("Kunne ikke gemme {}: {e}", self.path.display()))
    }
}

enum Kind {
    Text,
    OptText,
    Flag,
    Int,
    OptInt,
}

/// Fields of each list and their types, used to tidy hand-edited or older files.
const FIELDS: [(&str, &[(&str, Kind)]); 5] = [
    ("subjects", &[("id", Kind::Text), ("name", Kind::Text), ("key", Kind::Text), ("color", Kind::Text)]),
    (
        "notes",
        &[
            ("id", Kind::Text),
            ("title", Kind::Text),
            ("content", Kind::Text),
            ("subject", Kind::Text),
            ("subject_key", Kind::Text),
            ("is_pinned", Kind::Flag),
            ("created_at", Kind::Text),
            ("updated_at", Kind::Text),
        ],
    ),
    (
        "tasks",
        &[
            ("id", Kind::Text),
            ("title", Kind::Text),
            ("subject", Kind::Text),
            ("subject_key", Kind::Text),
            ("deadline", Kind::Text),
            ("status", Kind::Text),
            ("priority", Kind::Text),
            ("estimated_minutes", Kind::Int),
            ("created_at", Kind::Text),
            ("completed_at", Kind::OptText),
            ("external_id", Kind::OptText),
        ],
    ),
    (
        "blocks",
        &[
            ("id", Kind::Text),
            ("day", Kind::Text),
            ("time", Kind::Text),
            ("label", Kind::Text),
            ("subject", Kind::Text),
            ("color", Kind::Text),
            ("source", Kind::OptText),
            ("minutes", Kind::OptInt),
            ("external_id", Kind::OptText),
        ],
    ),
    (
        "feeds",
        &[
            ("id", Kind::Text),
            ("label", Kind::Text),
            ("url", Kind::Text),
            ("created_at", Kind::Text),
            ("last_synced_at", Kind::OptText),
            ("last_status", Kind::Text),
            ("last_error", Kind::OptText),
            ("imported_tasks", Kind::Int),
            ("imported_blocks", Kind::Int),
        ],
    ),
];

/// Makes small type mistakes harmless instead of refusing to start: a null or wrong-typed value
/// falls back to the default, numbers given as text or decimals are rounded to whole numbers,
/// and numbers or true/false where text is expected become text.
fn tidy(root: &mut Value) {
    let Some(obj) = root.as_object_mut() else { return };
    for (list, fields) in FIELDS {
        let Some(Value::Array(items)) = obj.get_mut(list) else { continue };
        for item in items.iter_mut() {
            let Some(item) = item.as_object_mut() else { continue };
            for (name, kind) in fields {
                let Some(v) = item.get(*name) else { continue };
                let fixed: Option<Value> = match (kind, v) {
                    (Kind::Text | Kind::OptText, Value::String(_)) => continue,
                    (Kind::OptText | Kind::OptInt, Value::Null) => continue,
                    (Kind::Text | Kind::OptText, Value::Number(n)) => Some(Value::String(n.to_string())),
                    (Kind::Text | Kind::OptText, Value::Bool(b)) => Some(Value::String(b.to_string())),
                    (Kind::Flag, Value::Bool(_)) => continue,
                    (Kind::Int | Kind::OptInt, Value::Number(n)) if n.is_i64() => continue,
                    (Kind::Int | Kind::OptInt, Value::Number(n)) => {
                        n.as_f64().filter(|f| f.is_finite() && f.abs() < 1e15).map(|f| Value::from(f.round() as i64))
                    }
                    (Kind::Int | Kind::OptInt, Value::String(t)) => {
                        t.trim().parse::<f64>().ok().filter(|f| f.is_finite() && f.abs() < 1e15).map(|f| Value::from(f.round() as i64))
                    }
                    _ => None,
                };
                match (fixed, kind) {
                    (Some(f), _) => {
                        item.insert((*name).to_string(), f);
                    }
                    (None, Kind::OptText | Kind::OptInt) => {
                        item.insert((*name).to_string(), Value::Null);
                    }
                    (None, _) => {
                        item.remove(*name);
                    }
                }
            }
        }
    }
}

/// JSON with 2-space indentation, like JSON.stringify(data, null, 2) in the old version.
pub fn to_pretty_json(data: &Data) -> Result<String, String> {
    serde_json::to_string_pretty(data).map_err(|e| e.to_string())
}

// ---------- ids, dates and names ----------

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// "2026-11-18T11:42:06.439Z", the same format as JavaScript's toISOString().
pub fn now_iso() -> String {
    iso(Utc::now())
}

pub fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Local calendar date as YYYY-MM-DD.
pub fn local_date(now: &DateTime<Local>) -> String {
    now.format("%Y-%m-%d").to_string()
}

/// A YYYY-MM-DD date. Like `new Date("YYYY-MM-DDT12:00:00")` in the old version, the year must
/// have four digits ("202-09-14" is not a date); unlike it, impossible dates such as 31 Feb are None.
pub fn parse_date(date: &str) -> Option<NaiveDate> {
    if date.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

/// True for a real calendar date written as YYYY-MM-DD.
pub fn is_valid_date(date: &str) -> bool {
    parse_date(date).is_some()
}

/// Whole days from `from` to `to` (0 if either is not a date).
pub fn days_between(from: &str, to: &str) -> i64 {
    match (parse_date(from), parse_date(to)) {
        (Some(a), Some(b)) => (b - a).num_days(),
        _ => 0,
    }
}

pub fn add_days(date: &str, days: i64) -> String {
    match parse_date(date) {
        Some(d) => (d + Duration::days(days)).format("%Y-%m-%d").to_string(),
        None => date.to_string(),
    }
}

pub const DAY_NAMES: [&str; 7] = ["Søndag", "Mandag", "Tirsdag", "Onsdag", "Torsdag", "Fredag", "Lørdag"];

/// Danish weekday name of a YYYY-MM-DD date.
pub fn day_name(date: &str) -> Option<&'static str> {
    parse_date(date).map(|d| DAY_NAMES[d.weekday().num_days_from_sunday() as usize])
}

/// Local midnight of a date, as an ISO string in UTC (for comparing with created/updated times).
pub fn local_midnight_iso(date: &str) -> String {
    let d = parse_date(date).unwrap_or_else(|| Local::now().date_naive());
    // If midnight does not exist that day (a daylight-saving jump), use 01:00 instead.
    let at = |h: u32| Local.from_local_datetime(&d.and_hms_opt(h, 0, 0).expect("valid time")).earliest();
    let local = at(0).or_else(|| at(1)).unwrap_or_else(Local::now);
    iso(local.with_timezone(&Utc))
}

/// Same rule as the frontend: "Dansk" -> "dansk", "Natur & teknik" -> "natur_teknik".
pub fn subject_key(name: &str) -> String {
    name.trim().to_lowercase().replace(" & ", "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_file_gets_default_subjects_and_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Kladde").join("kladde.json");
        let store = Store::open(&path).unwrap();
        assert!(path.exists());
        assert_eq!(store.data.subjects.len(), 7);
        assert_eq!(store.data.subjects[0].name, "Dansk");
        assert_eq!(store.data.subjects[0].key, "dansk");
        assert_eq!(store.data.version, 1);
    }

    #[test]
    fn reads_a_file_written_by_the_node_version_and_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kladde.json");
        let old = r#"{
  "version": 1,
  "subjects": [{"id": "s1", "name": "Dansk", "key": "dansk", "color": "blue"}],
  "notes": [{"id": "n1", "title": "Fotosyntese", "content": "Lys", "subject": "Dansk", "subject_key": "dansk", "is_pinned": true, "created_at": "2026-11-18T10:00:00.000Z", "updated_at": "2026-11-18T10:05:00.000Z"}],
  "tasks": [{"id": "t1", "title": "Analyse", "subject": "Dansk", "subject_key": "dansk", "deadline": "2026-11-20", "status": "done", "priority": "high", "estimated_minutes": 90, "created_at": "2026-11-18T10:00:00.000Z", "completed_at": "2026-11-18T11:00:00.000Z", "external_id": null}],
  "blocks": [{"id": "b1", "day": "Mandag", "time": "08:15", "label": "Dansk modul", "subject": "Dansk", "color": "blue", "source": "import", "minutes": 90, "external_id": "abc"}],
  "feeds": [],
  "messages": [{"id": "m1", "role": "user", "content": "hej", "subject": "Generelt", "created_at": "2026-11-18T10:00:00.000Z"}],
  "future_feature": {"enabled": true}
}"#;
        fs::write(&path, old).unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.data.notes[0].title, "Fotosyntese");
        assert!(store.data.notes[0].is_pinned);
        assert_eq!(store.data.tasks[0].estimated_minutes, 90);
        assert_eq!(store.data.tasks[0].completed_at.as_deref(), Some("2026-11-18T11:00:00.000Z"));
        assert_eq!(store.data.blocks[0].minutes, Some(90));
        assert_eq!(store.data.messages[0].content, "hej");
        assert!(store.data.extra.contains_key("future_feature"));

        store.save().unwrap();
        let again: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let original: Value = serde_json::from_str(old).unwrap();
        assert_eq!(again, original, "saving must not change the content");
    }

    #[test]
    fn missing_lists_get_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kladde.json");
        fs::write(&path, r#"{"notes": []}"#).unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.data.subjects.len(), 7);
        assert!(store.data.tasks.is_empty());
    }

    #[test]
    fn broken_file_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kladde.json");
        fs::write(&path, "{ this is not json").unwrap();
        assert!(Store::open(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ this is not json");
    }

    #[test]
    fn save_replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kladde.json");
        let mut store = Store::open(&path).unwrap();
        store.data.notes.push(Note { id: "n".into(), title: "Ny".into(), ..Default::default() });
        store.save().unwrap();
        store.data.notes[0].title = "Ændret".into();
        store.save().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("Ændret"));
        assert!(text.contains("\n  \"version\": 1"), "2-space pretty JSON");
        assert!(!dir.path().join("kladde.json.tmp").exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn ensure_subject_finds_or_creates() {
        let mut data = Data::default();
        let (s, created) = data.ensure_subject("  dansk ");
        assert!(!created);
        assert_eq!(s.name, "Dansk");
        let (s, created) = data.ensure_subject("Fysik");
        assert!(created);
        assert_eq!(s.key, "fysik");
        assert_eq!(s.color, "violet");
        let (s, _) = data.ensure_subject("   ");
        assert_eq!(s.name, "Selvstudie");
        assert_eq!(subject_key("Natur & teknik"), "natur_teknik");
    }

    #[test]
    fn small_type_mistakes_do_not_stop_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kladde.json");
        let file = "\u{feff}{\"tasks\": [{\"id\": \"t\", \"title\": null, \"estimated_minutes\": 52.5, \"completed_at\": 5, \"deadline\": \"2026-11-20\"}], \"notes\": [{\"id\": \"n\", \"title\": \"Note\", \"is_pinned\": \"ja\"}], \"blocks\": [{\"id\": \"b\", \"minutes\": \"90\", \"source\": 3}]}";
        fs::write(&path, file).unwrap();
        let store = Store::open(&path).unwrap();
        let t = &store.data.tasks[0];
        assert_eq!((t.title.as_str(), t.estimated_minutes, t.completed_at.as_deref()), ("", 53, Some("5")));
        assert!(!store.data.notes[0].is_pinned);
        assert_eq!(store.data.blocks[0].minutes, Some(90));
        assert_eq!(store.data.blocks[0].source.as_deref(), Some("3"));
        // Nothing is written until something changes.
        assert_eq!(fs::read_to_string(&path).unwrap(), file);
    }

    #[test]
    fn date_helpers() {
        assert_eq!(add_days("2026-11-30", 1), "2026-12-01");
        assert_eq!(add_days("2026-01-01", -1), "2025-12-31");
        assert_eq!(day_name("2026-11-18"), Some("Onsdag"));
        assert_eq!(day_name("2026-11-22"), Some("Søndag"));
        assert_eq!(day_name("2026-02-31"), None);
        let t = Utc.with_ymd_and_hms(2026, 11, 18, 11, 42, 6).unwrap() + Duration::milliseconds(439);
        assert_eq!(iso(t), "2026-11-18T11:42:06.439Z");
    }
}
