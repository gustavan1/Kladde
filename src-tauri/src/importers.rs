//! Turns school exports (Lectio, Ludus, Mit Niels Brock) and calendar feeds (.ics) into
//! tasks (deadlines) and planner blocks (lessons).
//!
//! This is a direct port of the earlier TypeScript version. In particular `external_id` is
//! computed exactly the same way, so rows imported by the old version are still recognised as
//! duplicates.

use std::sync::LazyLock;

use chrono::{Local, TimeZone};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::{day_name, local_date, DAY_NAMES};

pub const MAX_IMPORT_ROWS: usize = 5_000;
pub const MAX_CALENDAR_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENTS: usize = 10_000;
const MAX_EVENT_TEXT: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImportItem {
    /// "deadline" | "lesson" | "subject"
    pub kind: String,
    pub title: String,
    pub subject: String,
    pub subject_key: String,
    pub date: Option<String>,
    pub day: Option<String>,
    pub time: Option<String>,
    pub end_time: Option<String>,
    pub notes: Option<String>,
    pub external_id: Option<String>,
    pub selected: bool,
    pub estimated_minutes: Option<i64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ImportPreview {
    pub source: String,
    pub detected_format: String,
    pub items: Vec<ImportItem>,
    pub warnings: Vec<String>,
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

static DEADLINE_WORDS: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)aflever|opgave|deadline|frist|eksamen|prøve|test|hand.?in|assignment|due"));
static TIME_RE: LazyLock<Regex> = LazyLock::new(|| re(r"([0-9]{1,2})[:.]([0-9]{2})"));
static ISO_DATE: LazyLock<Regex> = LazyLock::new(|| re(r"([0-9]{4})-([0-9]{1,2})-([0-9]{1,2})"));
static DMY_DATE: LazyLock<Regex> = LazyLock::new(|| re(r"([0-9]{1,2})[/.\-]([0-9]{1,2})[/.\-]([0-9]{2,4})"));
static MONTH_DATE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)([0-9]{1,2})\.?\s*([a-zæøå]{3})[a-zæøå]*\.?\s*([0-9]{4})?"));
static LEVEL_SUFFIX: LazyLock<Regex> = LazyLock::new(|| re(r"\s+[ABC]$"));

/// Common Danish subject abbreviations (Lectio team names look like "2a DA" or "1x Ma").
fn abbreviation(word: &str) -> Option<&'static str> {
    Some(match word {
        "da" | "dan" => "Dansk",
        "ma" | "mat" => "Matematik",
        "en" | "eng" => "Engelsk",
        "sa" | "samf" => "Samfundsfag",
        "bi" | "bio" => "Biologi",
        "hi" | "hist" => "Historie",
        "fy" => "Fysik",
        "ke" => "Kemi",
        "ty" => "Tysk",
        "fr" => "Fransk",
        "sp" => "Spansk",
        "re" => "Religion",
        "id" => "Idræt",
        "mu" => "Musik",
        "bk" => "Billedkunst",
        "ng" => "Naturgeografi",
        "nv" => "Naturvidenskab",
        "ap" => "Almen sprogforståelse",
        "ol" => "Oldtidskundskab",
        "ps" => "Psykologi",
        "if" => "Informatik",
        "af" => "Afsætning",
        "vø" => "Virksomhedsøkonomi",
        "iø" => "International økonomi",
        "æs" => "Erhvervsøkonomi",
        _ => return None,
    })
}

fn month_number(name: &str) -> Option<u32> {
    Some(match name {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "maj" | "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "okt" | "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Danish upper-secondary subject names, longest first so "International økonomi" wins over "Økonomi".
const SUBJECT_NAMES: [&str; 38] = [
    "Kommunikation og it", "International økonomi", "Virksomhedsøkonomi", "Erhvervsøkonomi",
    "Samtidshistorie", "Oldtidskundskab", "Naturvidenskab", "Naturgeografi", "Programmering",
    "Erhvervsjura", "Billedkunst", "Samfundsfag", "Informatik", "Innovation", "Psykologi",
    "Matematik", "Teknologi", "Afsætning", "Filosofi", "Geografi", "Religion", "Historie",
    "Mediefag", "Kinesisk", "Biologi", "Engelsk", "Spansk", "Fransk", "Italiensk", "Russisk",
    "Japansk", "Design", "Dansk", "Fysik", "Idræt", "Musik", "Kemi", "Tysk",
];

/// A known subject name inside text like "1.O Tysk fortsætter" or "Engelsk 1o", as a whole word.
fn known_subject_name(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    SUBJECT_NAMES.iter().copied().find(|name| {
        let needle = name.to_lowercase();
        lower.match_indices(&needle).any(|(i, m)| {
            let before = lower[..i].chars().next_back();
            let after = lower[i + m.len()..].chars().next();
            !before.is_some_and(char::is_alphabetic) && !after.is_some_and(char::is_alphabetic)
        })
    })
}

static TEAM_CODE_SUFFIX: LazyLock<Regex> = LazyLock::new(|| re(r"\s+-\s+\S+$"));
static CLASS_PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^[0-9]+\.?[a-zA-ZæøåÆØÅ]{1,2}\s+"));
static CLASS_OR_YEAR_SUFFIX: LazyLock<Regex> = LazyLock::new(|| re(r"\s+([0-9]+\.?[a-zA-ZæøåÆØÅ]{1,2}|[0-9]{4})$"));
static DEPARTMENT_PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-ZÆØÅ]{2,4}\s+(\S)"));

/// "Dansk A" -> "Dansk", "2a DA" -> "Dansk", "1o JTP Informatik C - j26ososh" -> "Informatik",
/// "1.O Tysk fortsætter" -> "Tysk", "Engelsk 1o" -> "Engelsk", "" -> "Selvstudie".
pub fn clean_subject(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Selvstudie".into();
    }
    if let Some(name) = known_subject_name(trimmed) {
        return name.into();
    }
    // School systems add team codes ("- j26ososh"), classes ("1o", "1.O"), years and
    // department codes ("JTP") around the subject; drop those before the usual rules.
    let mut tidy = TEAM_CODE_SUFFIX.replace(trimmed, "").to_string();
    tidy = CLASS_PREFIX.replace(&tidy, "").to_string();
    tidy = CLASS_OR_YEAR_SUFFIX.replace(&tidy, "").to_string();
    if tidy.split_whitespace().count() > 1 {
        tidy = DEPARTMENT_PREFIX.replace(&tidy, "$1").to_string();
    }
    let trimmed = if tidy.trim().is_empty() { trimmed } else { tidy.trim() };
    let s = LEVEL_SUFFIX.replace(trimmed, "").to_string();
    let last = s.split_whitespace().last().unwrap_or("").to_lowercase();
    if let Some(name) = abbreviation(&last) {
        return name.into();
    }
    if let Some(name) = abbreviation(&s.to_lowercase()) {
        return name.into();
    }
    capitalize(&s)
}

/// Finds a known subject name (or abbreviation) inside free text such as a calendar title.
pub fn guess_subject(text: &str, known: &[String]) -> String {
    let lower = text.to_lowercase();
    for name in known {
        if lower.contains(&name.to_lowercase()) {
            return name.clone();
        }
    }
    for word in lower.split(|c: char| !c.is_alphabetic()) {
        if let Some(name) = abbreviation(word) {
            return name.into();
        }
    }
    "Selvstudie".into()
}

fn valid_date(y: i32, m: u32, d: u32) -> Option<String> {
    let y = if y < 100 { y + 2000 } else { y };
    if !(1000..=9999).contains(&y) { return None; }
    chrono::NaiveDate::from_ymd_opt(y, m, d).map(|_| format!("{y}-{m:02}-{d:02}"))
}

pub fn parse_time(raw: &str) -> Option<String> {
    let m = TIME_RE.captures(raw)?;
    let h: u32 = m[1].parse().ok()?;
    let min: u32 = m[2].parse().ok()?;
    (h < 24 && min < 60).then(|| format!("{h:02}:{min:02}"))
}

/// Understands 2026-09-14, 14/9-2026, 14-09-2026, 14.09.2026, 14/09/26 and "14. sep 2026".
/// Returns (date, time).
pub fn parse_date(raw: &str) -> (Option<String>, Option<String>) {
    let s = raw.trim();
    if s.is_empty() {
        return (None, None);
    }
    let num = |x: &str| x.parse::<u32>().unwrap_or(0);
    if let Some(m) = ISO_DATE.captures(s) {
        let whole = m.get(0).unwrap();
        let time = parse_time(&s[whole.end()..]).or_else(|| parse_time(&s[..whole.start()]));
        return (valid_date(num(&m[1]) as i32, num(&m[2]), num(&m[3])), time);
    }
    if let Some(m) = DMY_DATE.captures(s) {
        let rest = &s[m.get(0).unwrap().end()..];
        return (valid_date(num(&m[3]) as i32, num(&m[2]), num(&m[1])), parse_time(rest));
    }
    if let Some(m) = MONTH_DATE.captures(s) {
        if let Some(month) = month_number(&m[2].to_lowercase()) {
            let year = m.get(3).map(|y| num(y.as_str()) as i32).unwrap_or_else(|| {
                use chrono::Datelike;
                Local::now().year()
            });
            let rest = &s[m.get(0).unwrap().end()..];
            return (valid_date(year, month, num(&m[1])), parse_time(rest));
        }
    }
    (None, None)
}

/// CSV parser with quote support; also handles tab-separated text copied from a web table.
fn parse_delimited(text: &str, delimiter: char) -> Result<Vec<Vec<String>>, ()> {
    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<Vec<String>> = vec![];
    let mut row: Vec<String> = vec![];
    let mut cell = String::new();
    let mut quoted = false;
    let mut i = 0;
    let finish_row = |row: &mut Vec<String>, rows: &mut Vec<Vec<String>>| {
        if row.iter().any(|x| !x.is_empty()) {
            rows.push(std::mem::take(row));
        } else {
            row.clear();
        }
    };
    while i < chars.len() {
        let c = chars[i];
        if quoted {
            if c == '"' && chars.get(i + 1) == Some(&'"') {
                cell.push('"');
                i += 1;
            } else if c == '"' {
                quoted = false;
            } else {
                cell.push(c);
            }
        } else if c == '"' && cell.trim().is_empty() {
            quoted = true;
            cell.clear();
        } else if c == delimiter {
            row.push(cell.trim().to_string());
            cell.clear();
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.get(i + 1) == Some(&'\n') {
                i += 1;
            }
            row.push(cell.trim().to_string());
            cell.clear();
            finish_row(&mut row, &mut rows);
            if rows.len() > MAX_IMPORT_ROWS + 1 { return Err(()); }
        } else {
            cell.push(c);
        }
        i += 1;
    }
    row.push(cell.trim().to_string());
    finish_row(&mut row, &mut rows);
    if quoted || rows.len() > MAX_IMPORT_ROWS + 1 { return Err(()); }
    Ok(rows)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Field {
    Date,
    End,
    Estimate,
    Time,
    Day,
    Subject,
    Title,
    Notes,
    Kind,
}

const FIELD_WORDS: [(Field, &[&str]); 9] = [
    (Field::Date, &["afleveringsfrist", "frist", "deadline", "due", "dato", "date"]),
    (Field::End, &["sluttid", "slut", "end_time", "end", "til"]),
    (Field::Estimate, &["elevtid", "estimated_minutes", "minutter", "timer"]),
    (Field::Time, &["starttid", "start", "time", "tid", "kl", "fra"]),
    (Field::Day, &["ugedag", "dag", "day", "weekday"]),
    (Field::Subject, &["fag", "hold", "subject", "course", "team", "klasse"]),
    (
        Field::Title,
        &["opgavetitel", "titel", "title", "aktivitet", "activity", "opgave", "navn", "name", "summary", "emne"],
    ),
    (Field::Notes, &["opgavenote", "note", "noter", "notes", "beskrivelse", "description", "kommentar"]),
    (Field::Kind, &["kind", "type", "kategori", "category"]),
];

type Columns = std::collections::HashMap<Field, usize>;

/// Matches header names to fields: exact names first, then names that contain a keyword.
fn map_columns(header: &[String]) -> Columns {
    let h: Vec<String> = header
        .iter()
        .map(|x| x.to_lowercase().chars().filter(|c| c.is_alphabetic() || c.is_ascii_digit() || *c == '_').collect())
        .collect();
    let mut used = std::collections::HashSet::new();
    let mut out = Columns::new();
    for exact in [true, false] {
        for (field, words) in FIELD_WORDS {
            if out.contains_key(&field) {
                continue;
            }
            for w in words {
                let found = h.iter().enumerate().position(|(i, col)| {
                    !used.contains(&i) && !col.is_empty() && if exact { col == w } else { col.contains(w) }
                });
                if let Some(idx) = found {
                    out.insert(field, idx);
                    used.insert(idx);
                    break;
                }
            }
        }
    }
    out
}

/// sha1 of the parts joined with "|", first 16 hex characters (same as the old version).
pub fn ext_id(parts: &[Option<&str>]) -> String {
    let joined = parts.iter().map(|p| p.unwrap_or("")).collect::<Vec<_>>().join("|");
    sha1_smol::Sha1::from(joined).digest().to_string()[..16].to_string()
}

#[derive(Default)]
struct Record {
    title: Option<String>,
    subject: Option<String>,
    date: Option<String>,
    time: Option<String>,
    end: Option<String>,
    day: Option<String>,
    notes: Option<String>,
    kind: Option<String>,
    estimate: Option<String>,
}

impl Record {
    fn from(get: impl Fn(Field) -> Option<String>) -> Record {
        Record {
            title: get(Field::Title),
            subject: get(Field::Subject),
            date: get(Field::Date),
            time: get(Field::Time),
            end: get(Field::End),
            day: get(Field::Day),
            notes: get(Field::Notes),
            kind: get(Field::Kind),
            estimate: get(Field::Estimate),
        }
    }
}

fn make_item(source: &str, rec: Record, date_is_deadline: bool, estimate_header: &str) -> Option<ImportItem> {
    let title = rec.title.as_deref().unwrap_or("").trim().to_string();
    if title.is_empty() {
        return None;
    }
    let (date, date_time) = parse_date(rec.date.as_deref().unwrap_or(""));
    let time = parse_time(rec.time.as_deref().unwrap_or("")).or_else(|| date_time.clone());
    let end_time = parse_time(rec.end.as_deref().unwrap_or(""));
    let subject = clean_subject(rec.subject.as_deref().unwrap_or(""));
    let day_text = rec.day.as_deref().unwrap_or("");
    let kind_text = rec.kind.as_deref().unwrap_or("").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| kind_text.contains(w));
    let kind = if has(&["lesson", "lektion", "modul", "undervisning"]) {
        "lesson"
    } else if has(&["deadline", "aflevering", "opgave", "assignment"]) {
        "deadline"
    } else if has(&["subject", "fag"]) {
        "subject"
    } else if DEADLINE_WORDS.is_match(&title) || date_is_deadline {
        "deadline"
    } else if time.is_some() || end_time.is_some() || !day_text.is_empty() {
        "lesson"
    } else {
        "deadline"
    };

    let mut day: Option<String> = None;
    if !day_text.is_empty() {
        let d: String = day_text.trim().to_lowercase().chars().take(3).collect();
        day = DAY_NAMES.iter().find(|n| n.to_lowercase().starts_with(&d)).map(|n| n.to_string());
    }
    if day.is_none() {
        day = date.as_deref().and_then(day_name).map(String::from);
    }

    // Explicit minutes/hours use their column unit. Keep the legacy heuristic for Lectio Elevtid.
    let est_text: String = rec
        .estimate
        .as_deref()
        .unwrap_or("")
        .replacen(',', ".", 1)
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let num: f64 = if est_text.is_empty() { 0.0 } else { est_text.parse().unwrap_or(0.0) };
    let estimated_minutes = (num > 0.0).then(|| {
        let unit = estimate_header.to_lowercase();
        if unit.contains("timer") || (unit.contains("elevtid") && num <= 12.0) {
            js_round(num * 60.0)
        } else {
            js_round(num)
        }
    });

    let external_id = ext_id(&[Some(source), Some(kind), Some(&title), date.as_deref(), time.as_deref()]);
    Some(ImportItem {
        kind: kind.into(),
        subject_key: subject.to_lowercase().replace(" & ", "_"),
        subject,
        title,
        time: if kind == "deadline" { date_time.or(time) } else { time },
        date,
        day,
        end_time,
        notes: rec.notes.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
        external_id: Some(external_id),
        selected: true,
        estimated_minutes,
    })
}

/// Math.round from JavaScript (halves round up).
pub fn js_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

/// The text of a JSON value, like String(value) in JavaScript.
fn json_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        Value::Array(a) => a.iter().map(json_text).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

pub fn parse_import(source: &str, content: &str, da: bool) -> Result<ImportPreview, String> {
    let tr = |dk: String, en: String| if da { dk } else { en };
    let mut warnings: Vec<String> = vec![];
    let text = content.trim_start_matches('\u{feff}').trim();
    let mut items: Vec<ImportItem> = vec![];
    let detected_format: String;

    if text.starts_with('[') || text.starts_with('{') {
        detected_format = "JSON".into();
        let data: Value = serde_json::from_str(text).map_err(|_| {
            tr(
                "JSON-teksten kunne ikke læses. Tjek at den er kopieret helt.".into(),
                "The JSON could not be read. Check that it was copied completely.".into(),
            )
        })?;
        let list: Vec<Value> = match &data {
            Value::Array(a) => a.clone(),
            Value::Object(o) => match o.values().find_map(|v| v.as_array()) {
                Some(a) => a.clone(),
                None => vec![data.clone()],
            },
            other => vec![other.clone()],
        };
        if list.len() > MAX_IMPORT_ROWS { return Err(tr("For mange rækker.".into(), "Too many rows.".into())); }
        for raw in &list {
            let Some(obj) = raw.as_object() else { continue };
            let keys: Vec<String> = obj.keys().cloned().collect();
            let cols = map_columns(&keys);
            let date_is_deadline = cols
                .get(&Field::Date)
                .is_some_and(|&i| DEADLINE_HEADER.is_match(&keys[i]));
            let rec = Record::from(|f| cols.get(&f).map(|&i| json_text(&obj[&keys[i]])));
            if let Some(item) = make_item(source, rec, date_is_deadline, cols.get(&Field::Estimate).map_or("", |&i| keys[i].as_str())) {
                items.push(item);
            }
        }
    } else {
        // Prefer a delimiter that actually separates known header names. Commas in
        // quoted descriptions on later rows do not define the file format.
        let delimiter = ['\t', ';', ','].into_iter().max_by_key(|&d| {
            text.lines().take(5).filter_map(|line| {
                let rows = parse_delimited(line, d).ok()?;
                Some(map_columns(rows.first()?).len())
            }).max().unwrap_or(0)
        }).unwrap_or(',');
        detected_format = match delimiter {
            '\t' => tr("Kopieret tabel".into(), "Copied table".into()),
            ';' => tr("CSV (semikolon)".into(), "CSV (semicolon)".into()),
            _ => "CSV".into(),
        };
        let rows = parse_delimited(text, delimiter).map_err(|_| tr(
            "For mange rækker eller uafsluttet citeret felt.".into(), "Too many rows or an unterminated quoted field.".into(),
        ))?;
        // The header is the row (among the first five) that names the most known columns.
        let mut header_idx = 0;
        let mut best: i64 = -1;
        for (i, r) in rows.iter().take(5).enumerate() {
            let n = map_columns(r).len() as i64;
            if n > best {
                best = n;
                header_idx = i;
            }
        }
        let empty = vec![];
        let header = rows.get(header_idx).unwrap_or(&empty);
        let cols = map_columns(header);
        if !cols.contains_key(&Field::Title) {
            return Err(tr(
                "Kunne ikke finde en kolonne med titler. Første linje skal være overskrifter, fx 'Opgave,Fag,Frist'."
                    .into(),
                "Could not find a title column. The first line should be headers, e.g. 'Task,Subject,Deadline'."
                    .into(),
            ));
        }
        let date_is_deadline = cols
            .get(&Field::Date)
            .is_some_and(|&i| DEADLINE_HEADER.is_match(&header[i]));
        let estimate_header = cols.get(&Field::Estimate).map_or("", |&i| header[i].as_str());
        let mut skipped = 0;
        for r in rows.iter().skip(header_idx + 1) {
            let rec = Record::from(|f| cols.get(&f).and_then(|&i| r.get(i).cloned()));
            match make_item(source, rec, date_is_deadline, estimate_header) {
                Some(item) => items.push(item),
                None => skipped += 1,
            }
        }
        if skipped > 0 {
            warnings.push(tr(
                format!("{skipped} rækker uden titel blev sprunget over."),
                format!("{skipped} rows without a title were skipped."),
            ));
        }
    }

    let no_date = items.iter().filter(|i| i.kind == "deadline" && i.date.is_none()).count();
    if no_date > 0 {
        warnings.push(tr(
            format!("{no_date} deadlines mangler en dato og er ikke valgt. Ret datoen i filen, eller vælg dem til, så får de dags dato."),
            format!("{no_date} deadlines have no date and are not selected. Fix the date in the file, or tick them to use today's date."),
        ));
    }
    let no_day = items.iter().filter(|i| i.kind == "lesson" && i.day.is_none()).count();
    if no_day > 0 {
        warnings.push(tr(
            format!("{no_day} lektioner mangler dato/ugedag og springes over."),
            format!("{no_day} lessons have no date/weekday and will be skipped."),
        ));
    }
    for i in items.iter_mut() {
        i.selected = (i.kind != "deadline" || i.date.is_some()) && (i.kind != "lesson" || i.day.is_some());
    }
    if items.is_empty() {
        warnings.push(tr("Der blev ikke fundet noget at importere.".into(), "Nothing to import was found.".into()));
    }
    Ok(ImportPreview { source: source.into(), detected_format, items, warnings })
}

static DEADLINE_HEADER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)frist|deadline|due"));

// ---------- Calendar feeds (.ics) ----------

#[derive(Clone, Debug, PartialEq)]
pub struct IcsEvent {
    pub uid: String,
    pub recurrence_id: Option<String>,
    pub summary: String,
    pub description: String,
    pub date: String,
    pub time: Option<String>,
    pub end_time: Option<String>,
    pub all_day: bool,
}

static ICS_DATE: LazyLock<Regex> =
    LazyLock::new(|| re(r"^([0-9]{4})([0-9]{2})([0-9]{2})(?:T([0-9]{2})([0-9]{2})([0-9]{2})?(Z)?)?"));

fn unescape_ics(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') | Some('N') => {
                    out.push('\n');
                    chars.next();
                }
                Some(&x) if x == ',' || x == ';' || x == '\\' => {
                    out.push(x);
                    chars.next();
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// "VALUE=DATE" but not "VALUE=DATE-TIME".
fn is_date_only_param(params: &str) -> bool {
    params.match_indices("VALUE=DATE").any(|(i, m)| !params[i + m.len()..].starts_with('-'))
}

#[derive(Clone, Copy)]
struct Property<'a> {
    name: &'a str,
    params: &'a str,
    value: &'a str,
}

impl Property<'_> {
    fn all_day(self) -> bool {
        !self.value.contains('T') || is_date_only_param(self.params)
    }

    fn datetime(self) -> Option<chrono::DateTime<rrule::Tz>> {
        let m = ICS_DATE.captures(self.value)?;
        let date = chrono::NaiveDate::from_ymd_opt(m[1].parse().ok()?, m[2].parse().ok()?, m[3].parse().ok()?)?;
        let time = |i| m.get(i).map_or(Some(0), |v| v.as_str().parse::<u32>().ok());
        let (h, min, sec) = if self.all_day() { (0, 0, 0) } else { (time(4)?, time(5)?, time(6)?) };
        let tz = if self.value.ends_with('Z') {
            rrule::Tz::UTC
        } else if let Some(id) = self.params.split(';').find_map(|p| p.strip_prefix("TZID=")) {
            tz_from_id(id.trim_matches('"'))
        } else {
            rrule::Tz::LOCAL
        };
        tz.from_local_datetime(&date.and_hms_opt(h, min, sec)?).earliest()
    }
}

/// IANA names ("Europe/Copenhagen") and the Windows names Outlook/Office 365 calendars use
/// ("Romance Standard Time"). Anything unknown is read as this computer's local time, like the
/// earlier versions did, so no lesson is dropped because of an unusual time zone name.
fn tz_from_id(id: &str) -> rrule::Tz {
    if let Ok(tz) = id.parse() {
        return rrule::Tz::Tz(tz);
    }
    let iana = match id {
        "Romance Standard Time" => "Europe/Copenhagen",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central Europe Standard Time" | "Central European Standard Time" => "Europe/Warsaw",
        "GMT Standard Time" => "Europe/London",
        "Greenwich Standard Time" | "UTC" | "Coordinated Universal Time" => "UTC",
        "FLE Standard Time" => "Europe/Helsinki",
        _ => return rrule::Tz::LOCAL,
    };
    iana.parse().map(rrule::Tz::Tz).unwrap_or(rrule::Tz::LOCAL)
}

fn occurrence_key(start: chrono::DateTime<rrule::Tz>, all_day: bool) -> String {
    if all_day { start.date_naive().to_string() } else { start.timestamp().to_string() }
}

fn event_bodies(text: &str) -> Result<Vec<&str>, String> {
    let mut stack = Vec::new();
    let mut bodies = Vec::new();
    let mut event_start = 0;
    let mut offset = 0;
    let mut finished = false;
    for line in text.split_inclusive('\n') {
        let property = line.trim_end_matches('\n');
        let next = offset + line.len();
        if let Some((key, name)) = property.split_once(':') {
            if key.eq_ignore_ascii_case("BEGIN") {
                if finished || stack.len() >= 32 || (stack.is_empty() && !name.eq_ignore_ascii_case("VCALENDAR")) {
                    return Err("Invalid calendar envelope".into());
                }
                if name.eq_ignore_ascii_case("VEVENT") {
                    if stack.len() != 1 { return Err("Invalid nested event".into()); }
                    event_start = next;
                }
                stack.push(name);
            } else if key.eq_ignore_ascii_case("END") {
                if !stack.pop().is_some_and(|open| name.eq_ignore_ascii_case(open)) {
                    return Err("Incomplete calendar component".into());
                }
                if name.eq_ignore_ascii_case("VEVENT") {
                    if bodies.len() >= MAX_EVENTS { return Err("Too many calendar events".into()); }
                    bodies.push(&text[event_start..offset]);
                }
                if stack.is_empty() { finished = true; }
            }
        }
        if stack.is_empty() && !property.trim().is_empty() && !property.eq_ignore_ascii_case("END:VCALENDAR") {
            return Err("Invalid calendar envelope".into());
        }
        offset = next;
    }
    if !finished || !stack.is_empty() { return Err("Incomplete calendar".into()); }
    Ok(bodies)
}

pub fn parse_ics(text: &str, today: chrono::NaiveDate) -> Result<Vec<IcsEvent>, String> {
    parse_ics_with_limits(text, today, MAX_EVENT_TEXT, 50_000)
}

fn parse_ics_with_limits(text: &str, today: chrono::NaiveDate, mut text_left: usize, mut candidates_left: usize) -> Result<Vec<IcsEvent>, String> {
    use chrono::Duration;
    use std::collections::HashSet;

    if text.len() > MAX_CALENDAR_BYTES { return Err("Calendar exceeds size limit".into()); }
    let unfolded = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n").replace("\n ", "").replace("\n\t", "");
    let mut events = vec![];
    let mut overrides = HashSet::new();
    let mut instances = vec![];
    for body in event_bodies(&unfolded)? {
        let mut fields = Vec::new();
        let mut nested = 0;
        for line in body.lines() {
            let Some((key, value)) = line.split_once(':') else { continue };
            if key.eq_ignore_ascii_case("BEGIN") { nested += 1; continue }
            if key.eq_ignore_ascii_case("END") { nested -= 1; continue }
            if nested == 0 {
                let (name, params) = key.split_once(';').unwrap_or((key, ""));
                if fields.len() >= 2048 { return Err("Too many event properties".into()); }
                fields.push(Property { name, params, value });
            }
        }
        let get = |key: &str| fields.iter().copied().find(|p| p.name.eq_ignore_ascii_case(key));
        let uid = get("UID").map_or("", |p| p.value);
        let recurrence = get("RECURRENCE-ID").and_then(|p| Some(occurrence_key(p.datetime()?, p.all_day())));
        if let Some(key) = &recurrence {
            overrides.insert((uid.to_string(), key.clone()));
        }
        if get("STATUS").is_some_and(|p| p.value.eq_ignore_ascii_case("CANCELLED")) { continue }
        let (Some(start), Some(summary)) = (get("DTSTART"), get("SUMMARY")) else { continue };
        let Some(first) = start.datetime() else { continue };
        let end = get("DTEND").filter(|p| !p.all_day()).and_then(Property::datetime);
        let all_day = start.all_day();
        let description = get("DESCRIPTION").map_or("", |p| p.value);
        let owned_bytes = uid.len() + summary.value.len() + description.len();
        let mut make_event = |at: chrono::DateTime<rrule::Tz>, instance| -> Result<IcsEvent, String> {
            text_left = text_left.checked_sub(owned_bytes).ok_or("Expanded calendar text exceeds limit")?;
            let local = at.with_timezone(&Local);
            let date = if all_day { at.date_naive().to_string() } else { local_date(&local) };
            let time = (!all_day).then(|| local.format("%H:%M").to_string());
            let uid = if uid.is_empty() {
                ext_id(&[Some(summary.value), Some(&date), time.as_deref()])
            } else { uid.to_string() };
            Ok(IcsEvent {
                uid, recurrence_id: instance, date, time, all_day,
                end_time: end.map(|e| (local + (e - first)).format("%H:%M").to_string()),
                summary: unescape_ics(summary.value).trim().into(),
                description: unescape_ics(description).trim().into(),
            })
        };
        if !fields.iter().any(|p| p.name.eq_ignore_ascii_case("RRULE") || p.name.eq_ignore_ascii_case("RDATE")) {
            if events.len() + instances.len() >= MAX_EVENTS { return Err("Too many calendar events".into()); }
            events.push(make_event(first, recurrence)?);
            continue;
        }
        // One odd series (unknown rule, too fine-grained, too many dates) is skipped on its
        // own instead of making the whole calendar fail.
        let expand = || -> Result<_, &'static str> {
            let mut set = rrule::RRuleSet::new(first).rdate(first);
            for p in fields.iter().copied() {
                if p.name.eq_ignore_ascii_case("RRULE") {
                    if p.value.len() > 2048 || !set.get_rrule().is_empty() { return Err("Unsupported recurrence rule"); }
                    let rule = p.value.parse::<rrule::RRule<rrule::Unvalidated>>().and_then(|r| r.validate(first))
                        .map_err(|_| "Invalid RRULE")?;
                    // School timetables have at most hourly occurrences. Very fine frequencies can
                    // require scanning millions of historical instances before the visible window.
                    if matches!(rule.get_freq(), rrule::Frequency::Secondly | rrule::Frequency::Minutely) {
                        return Err("Unsupported sub-hourly calendar series");
                    }
                    if rule.get_by_hour().len().max(1) * rule.get_by_minute().len().max(1) * rule.get_by_second().len().max(1) > 24 {
                        return Err("Unsupported sub-hourly calendar series");
                    }
                    set = set.rrule(rule);
                } else if p.name.eq_ignore_ascii_case("RDATE") || p.name.eq_ignore_ascii_case("EXDATE") {
                    if set.get_rdate().len() + set.get_exdate().len() + p.value.matches(',').count() + 1 > MAX_EVENTS {
                        return Err("Calendar series exceeds expansion limit");
                    }
                    for value in p.value.split(',') {
                        let Some(date) = Property { value, ..p }.datetime() else { continue };
                        set = if p.name.eq_ignore_ascii_case("RDATE") { set.rdate(date) } else { set.exdate(date) };
                    }
                }
            }
            let bound = |days, h| {
                let day = today + Duration::days(days);
                rrule::Tz::LOCAL.from_local_datetime(&day.and_hms_opt(h, 0, 0)?).earliest()
            };
            let start_bound = bound(-2, 0).ok_or("Invalid calendar window")?;
            let end_bound = bound(182, 0).ok_or("Invalid calendar window")?;
            // Count historical candidates as well as visible ones.
            let result = set.before(end_bound).all(candidates_left.min(50_000) as u16);
            if result.limited { return Err("Calendar series exceeds expansion limit") }
            Ok((start_bound, result.dates))
        };
        if candidates_left == 0 { return Err("Calendar exceeds expansion limit".into()); }
        let (start_bound, mut dates) = match expand() {
            Ok(dates) => dates,
            Err("Calendar series exceeds expansion limit") => return Err("Calendar exceeds expansion limit".into()),
            Err(reason) => {
                crate::log(&format!("Skipped calendar series: {reason}"));
                continue;
            }
        };
        candidates_left -= dates.len();
        dates.dedup();
        for at in dates.into_iter().filter(|at| at >= &start_bound) {
            if events.len() + instances.len() >= MAX_EVENTS { return Err("Too many calendar events".into()); }
            let key = occurrence_key(at, all_day);
            instances.push(make_event(at, Some(key))?);
        }
    }
    events.extend(instances.into_iter().filter(|e| {
        !overrides.contains(&(e.uid.clone(), e.recurrence_id.clone().expect("expanded occurrence")))
    }));
    Ok(events)
}

pub fn is_deadline_event(e: &IcsEvent) -> bool {
    e.all_day || DEADLINE_WORDS.is_match(&e.summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn school_system_subject_names_are_tidied() {
        assert_eq!(clean_subject("1o JTP Informatik C - j26ososh"), "Informatik");
        assert_eq!(clean_subject("1o JTP Virksomhedsøkonomi A - j26ososh"), "Virksomhedsøkonomi");
        assert_eq!(clean_subject("1o JTP Aktivitet - j26ososh"), "Aktivitet");
        assert_eq!(clean_subject("1o JTP Læsevejledning - j26ososh"), "Læsevejledning");
        assert_eq!(clean_subject("1.O Tysk fortsætter"), "Tysk");
        assert_eq!(clean_subject("Engelsk 1o"), "Engelsk");
        assert_eq!(clean_subject("Informatik 2026"), "Informatik");
        assert_eq!(clean_subject("International økonomi"), "International økonomi");
        assert_eq!(clean_subject("Dansk A"), "Dansk");
        assert_eq!(clean_subject("2a DA"), "Dansk");
        assert_eq!(clean_subject("1x Ma"), "Matematik");
        assert_eq!(clean_subject("Tyskland-tur"), "Tyskland-tur");
    }

    #[test]
    fn outlook_time_zones_and_odd_series_do_not_drop_lessons() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:o365\r\nSUMMARY:Dansk modul\r\nDTSTART;TZID=Romance Standard Time:20261123T081500\r\nDTEND;TZID=Romance Standard Time:20261123T094500\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:unknown-zone\r\nSUMMARY:Fysik\r\nDTSTART;TZID=Somewhere/Unknown:20261124T100000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:bad\r\nSUMMARY:Mærkelig serie\r\nDTSTART:20261123T081500\r\nRRULE:FREQ=WEEKLY;BYDAY=XX\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:minutely\r\nSUMMARY:Hvert minut\r\nDTSTART:20261123T081500\r\nRRULE:FREQ=MINUTELY\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:ok\r\nSUMMARY:Aflevering\r\nDTSTART;VALUE=DATE:20261125\r\nEND:VEVENT\r\nEND:VCALENDAR";
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        let events = parse_ics(ics, today).unwrap();
        let names: Vec<&str> = events.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(names, ["Dansk modul", "Fysik", "Aflevering"]);
        assert_eq!(events[0].time.as_deref(), Some(copenhagen_time(8, 15).as_str()));
        assert_eq!(events[0].end_time.as_deref(), Some(copenhagen_time(9, 45).as_str()));
        assert_eq!(events[1].time.as_deref(), Some("10:00"), "unknown zone = local time, as before");
    }

    fn copenhagen_time(hour: u32, minute: u32) -> String {
        chrono::Utc.with_ymd_and_hms(2026, 11, 23, hour - 1, minute, 0).unwrap().with_timezone(&Local).format("%H:%M").to_string()
    }

    fn titles(p: &ImportPreview) -> Vec<&str> {
        p.items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn comma_csv_lectio_example() {
        let p = parse_import(
            "lectio",
            "Opgave,Fag,Afleveringsfrist\nDansk aflevering,Dansk,2026-09-14\nMatematik aflevering,Matematik,2026-09-18",
            true,
        )
        .unwrap();
        assert_eq!(p.detected_format, "CSV");
        assert_eq!(titles(&p), ["Dansk aflevering", "Matematik aflevering"]);
        let first = &p.items[0];
        assert_eq!(first.kind, "deadline");
        assert_eq!(first.subject, "Dansk");
        assert_eq!(first.date.as_deref(), Some("2026-09-14"));
        assert_eq!(first.day.as_deref(), Some("Mandag"));
        assert!(first.selected);
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn external_id_matches_the_old_version() {
        // Value computed by the old Node version: sha1("lectio|deadline|Dansk aflevering|2026-09-14|")[0..16].
        let p = parse_import("lectio", "Opgave,Fag,Frist\nDansk aflevering,Dansk,2026-09-14", true).unwrap();
        assert_eq!(p.items[0].external_id.as_deref(), Some("e0bd2fc2466bae4f"));
    }

    #[test]
    fn semicolon_csv_ludus_lessons() {
        let p = parse_import(
            "ludus",
            "\"Aktivitet\";\"Hold\";\"Dato\";\"Start\";\"Slut\"\n\"Dansk modul\";\"Dansk A\";\"2026-09-14\";\"08:15\";\"09:45\"\n\"Matematik modul\";\"Matematik B\";\"2026-09-15\";\"10:00\";\"11:30\"",
            false,
        )
        .unwrap();
        assert_eq!(p.detected_format, "CSV (semicolon)");
        let m = &p.items[0];
        assert_eq!(m.kind, "lesson");
        assert_eq!(m.subject, "Dansk");
        assert_eq!(m.day.as_deref(), Some("Mandag"));
        assert_eq!(m.time.as_deref(), Some("08:15"));
        assert_eq!(m.end_time.as_deref(), Some("09:45"));
        assert_eq!(p.items[1].subject, "Matematik");
        assert_eq!(p.items[1].day.as_deref(), Some("Tirsdag"));
    }

    #[test]
    fn json_mit_niels_brock() {
        let p = parse_import(
            "mitnielsbrock",
            r#"[{"title":"Aflevering: Marketing case","subject":"Afsætning","date":"2026-09-20"},{"title":"Afsætning modul","subject":"Afsætning","date":"2026-09-16","time":"08:15"}]"#,
            true,
        )
        .unwrap();
        assert_eq!(p.detected_format, "JSON");
        assert_eq!(p.items[0].kind, "deadline");
        assert_eq!(p.items[1].kind, "lesson");
        assert_eq!(p.items[1].time.as_deref(), Some("08:15"));
        assert_eq!(p.items[1].day.as_deref(), Some("Onsdag"));
    }

    #[test]
    fn json_object_with_a_list_inside() {
        let p = parse_import("lectio", r#"{"items":[{"titel":"Rapport","fag":"Fysik","frist":"1/12-2026","timer":3}]}"#, true)
            .unwrap();
        let i = &p.items[0];
        assert_eq!((i.title.as_str(), i.subject.as_str(), i.kind.as_str()), ("Rapport", "Fysik", "deadline"));
        assert_eq!(i.date.as_deref(), Some("2026-12-01"));
        assert_eq!(i.estimated_minutes, Some(180));
    }

    #[test]
    fn copied_lectio_table() {
        let p = parse_import(
            "lectio",
            "Uge\tHold\tOpgavetitel\tFrist\tElevtid\tStatus\tOpgavenote\n47\t2a DA\tNovelleanalyse\t25/11-2026 23:30\t3,00\tVenter\tLæs side 4\n48\t2a Ma\tAflevering 4\t1/12-2026 22:00\t2,50\tVenter\t",
            true,
        )
        .unwrap();
        assert_eq!(p.detected_format, "Kopieret tabel");
        let a = &p.items[0];
        assert_eq!(a.title, "Novelleanalyse");
        assert_eq!(a.subject, "Dansk");
        assert_eq!(a.kind, "deadline");
        assert_eq!(a.date.as_deref(), Some("2026-11-25"));
        assert_eq!(a.time.as_deref(), Some("23:30"));
        assert_eq!(a.estimated_minutes, Some(180));
        assert_eq!(a.notes.as_deref(), Some("Læs side 4"));
        let b = &p.items[1];
        assert_eq!(b.subject, "Matematik");
        assert_eq!(b.estimated_minutes, Some(150));
        assert_eq!(b.notes, None);
    }

    #[test]
    fn header_found_below_a_title_line_and_empty_rows_skipped() {
        let p = parse_import("lectio", "Mine opgaver\n\nOpgave,Fag,Frist\nEssay,Engelsk,2026-10-01\n,Dansk,2026-10-02", false)
            .unwrap();
        assert_eq!(titles(&p), ["Essay"]);
        assert_eq!(p.warnings, ["1 rows without a title were skipped."]);
    }

    #[test]
    fn quoted_cells_with_commas_and_newlines() {
        let p = parse_import("lectio", "Opgave,Fag,Frist\n\"Essay, del 1\",Engelsk,2026-10-01\n\"Linje\nto\",Dansk,2026-10-02", true)
            .unwrap();
        assert_eq!(titles(&p), ["Essay, del 1", "Linje\nto"]);
    }

    #[test]
    fn missing_dates_are_not_selected() {
        let p = parse_import("lectio", "Opgave,Fag,Frist\nEssay,Engelsk,\nLektion,Dansk,", true).unwrap();
        assert!(p.items.iter().all(|i| !i.selected));
        assert!(p.warnings[0].starts_with("2 deadlines mangler en dato"));
    }

    #[test]
    fn errors_are_explained() {
        assert!(parse_import("lectio", "a,b,c\n1,2,3", true).unwrap_err().contains("kolonne med titler"));
        assert!(parse_import("lectio", "[{bad json", false).unwrap_err().contains("JSON could not be read"));
        let p = parse_import("lectio", "[]", true).unwrap();
        assert_eq!(p.warnings, ["Der blev ikke fundet noget at importere."]);
    }

    #[test]
    fn date_and_time_formats() {
        assert_eq!(parse_date("2026-09-14"), (Some("2026-09-14".into()), None));
        assert_eq!(parse_date("2026-09-14 08:15"), (Some("2026-09-14".into()), Some("08:15".into())));
        assert_eq!(parse_date("14/9-2026"), (Some("2026-09-14".into()), None));
        assert_eq!(parse_date("14-09-2026"), (Some("2026-09-14".into()), None));
        assert_eq!(parse_date("14.09.2026 kl. 8.15"), (Some("2026-09-14".into()), Some("08:15".into())));
        assert_eq!(parse_date("14/09/26"), (Some("2026-09-14".into()), None));
        assert_eq!(parse_date("14. sep 2026"), (Some("2026-09-14".into()), None));
        assert_eq!(parse_date("3. December 2026"), (Some("2026-12-03".into()), None));
        assert_eq!(parse_date("ingen dato"), (None, None));
        assert_eq!(parse_time("8:05"), Some("08:05".into()));
        assert_eq!(parse_time("25:00"), None);
    }

    #[test]
    fn subjects_are_cleaned() {
        assert_eq!(clean_subject("Dansk A"), "Dansk");
        assert_eq!(clean_subject("2a DA"), "Dansk");
        assert_eq!(clean_subject("1x Ma"), "Matematik");
        assert_eq!(clean_subject("fysik"), "Fysik");
        assert_eq!(clean_subject(""), "Selvstudie");
        let known = vec!["Dansk".to_string(), "Matematik".to_string()];
        assert_eq!(guess_subject("Aflevering i matematik", &known), "Matematik");
        assert_eq!(guess_subject("2a FY modul", &known), "Fysik");
        assert_eq!(guess_subject("Studietur", &known), "Selvstudie");
    }

    #[test]
    fn ics_events() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:abc@lectio\r\nSUMMARY:Aflevering: Dansk\r\n  analyse\r\nDTSTART;VALUE=DATE:20261125\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:les1\r\nSUMMARY:2a MA\\, modul 3\r\nDESCRIPTION:Lokale 12\\nTag lommeregner\r\nDTSTART;TZID=Europe/Copenhagen:20261123T081500\r\nDTEND;TZID=Europe/Copenhagen:20261123T094500\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Uden start\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = parse_ics(ics, Local::now().date_naive()).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].summary, "Aflevering: Dansk analyse");
        assert!(ev[0].all_day && is_deadline_event(&ev[0]));
        assert_eq!(ev[0].date, "2026-11-25");
        assert_eq!(ev[1].summary, "2a MA, modul 3");
        assert_eq!(ev[1].description, "Lokale 12\nTag lommeregner");
        assert_eq!((ev[1].time.as_deref(), ev[1].end_time.as_deref()), (Some(copenhagen_time(8, 15).as_str()), Some(copenhagen_time(9, 45).as_str())));
        assert!(!is_deadline_event(&ev[1]));
        assert!(is_date_only_param("VALUE=DATE"));
        assert!(!is_date_only_param("VALUE=DATE-TIME"));
    }

    // ---------- Compared with the old TypeScript version ----------
    //
    // The expected values below were produced by running the old `parseImport` (Node version,
    // server/importers.ts) on exactly the same input, so whole items (kind, subject, date, day,
    // time, end_time, notes, external_id, selected, estimated_minutes), the detected format and
    // the warnings must be identical.

    fn check(source: &str, content: &str, da: bool, expected: &str) {
        let p = parse_import(source, content, da).unwrap();
        assert_eq!(p.source, source);
        let got = serde_json::json!({ "detected_format": p.detected_format, "items": p.items, "warnings": p.warnings });
        let want: Value = serde_json::from_str(expected).unwrap();
        if got != want {
            // Item by item first, so a failure points at the row that differs.
            for (i, (g, w)) in got["items"].as_array().unwrap().iter().zip(want["items"].as_array().unwrap()).enumerate() {
                assert_eq!(g, w, "item {i} differs");
            }
            assert_eq!(got, want);
        }
    }

    #[test]
    fn same_as_old_version_comma_csv_deadlines() {
        check(
            "lectio",
            "Opgave,Fag,Afleveringsfrist,Elevtid,Note\nDansk aflevering,Dansk A,2026-09-14,\"3,00\",Læs kapitel 2\nMatematik aflevering,2x MA,18-09-2026 12:00,90,\nEngelsk essay,Engelsk,14/9-2026,0.75,\"Husk \"\"kilder\"\"\"\nKemi rapport,ke,,2,\nHistorie opgave,Hi,\"3. december 2026 kl. 10.00\",,\n",
            true,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "deadline", "title": "Dansk aflevering", "subject": "Dansk", "subject_key": "dansk", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": "Læs kapitel 2", "external_id": "e0bd2fc2466bae4f", "selected": true, "estimated_minutes": 180 }, { "kind": "deadline", "title": "Matematik aflevering", "subject": "Matematik", "subject_key": "matematik", "date": "2026-09-18", "day": "Fredag", "time": "12:00", "end_time": null, "notes": null, "external_id": "7b1e9e080ddb8f51", "selected": true, "estimated_minutes": 90 }, { "kind": "deadline", "title": "Engelsk essay", "subject": "Engelsk", "subject_key": "engelsk", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": "Husk \"kilder\"", "external_id": "f5939c5e2911ea98", "selected": true, "estimated_minutes": 45 }, { "kind": "deadline", "title": "Kemi rapport", "subject": "Kemi", "subject_key": "kemi", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "1970baadad265444", "selected": false, "estimated_minutes": 120 }, { "kind": "deadline", "title": "Historie opgave", "subject": "Historie", "subject_key": "historie", "date": "2026-12-03", "day": "Torsdag", "time": "10:00", "end_time": null, "notes": null, "external_id": "69420cc24613897c", "selected": true, "estimated_minutes": null } ], "warnings": [ "1 deadlines mangler en dato og er ikke valgt. Ret datoen i filen, eller vælg dem til, så får de dags dato." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_comma_csv_lessons_english() {
        check(
            "ludus",
            "Titel,Fag,Dato,Start,Slut\nDansk modul,Dansk A,2026-09-14,08:15,09:45\nMatematik,Matematik B,15.09.2026,10.00,11.30\nSpisepause,,2026-09-16,12:00,\nStudiecafé,Selvstudie,,14:00,15:00\nGruppearbejde,Samfundsfag C,17/09/26,,",
            false,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "lesson", "title": "Dansk modul", "subject": "Dansk", "subject_key": "dansk", "date": "2026-09-14", "day": "Mandag", "time": "08:15", "end_time": "09:45", "notes": null, "external_id": "1029cc35aee40f2a", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Matematik", "subject": "Matematik", "subject_key": "matematik", "date": "2026-09-15", "day": "Tirsdag", "time": "10:00", "end_time": "11:30", "notes": null, "external_id": "a3fa8bca2cedf141", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Spisepause", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-16", "day": "Onsdag", "time": "12:00", "end_time": null, "notes": null, "external_id": "10f105ad960d7e9a", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Studiecafé", "subject": "Selvstudie", "subject_key": "selvstudie", "date": null, "day": null, "time": "14:00", "end_time": "15:00", "notes": null, "external_id": "380bd2e6b847742f", "selected": false, "estimated_minutes": null }, { "kind": "deadline", "title": "Gruppearbejde", "subject": "Samfundsfag", "subject_key": "samfundsfag", "date": "2026-09-17", "day": "Torsdag", "time": null, "end_time": null, "notes": null, "external_id": "5612ad909c88255e", "selected": true, "estimated_minutes": null } ], "warnings": [ "1 lessons have no date/weekday and will be skipped." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_comma_csv_kind_column() {
        check(
            "mitnielsbrock",
            "Type,Navn,Fag,Dato,Tid,Beskrivelse\nLektion,Afsætning,AF,2026-11-23,08:15,Lokale 3\nAflevering,Marketing case,Afsætning,2026-11-27,,\nFag,Virksomhedsøkonomi,vø,,,\nAndet,Ekskursion,,2026-11-25,,Husk madpakke\nModul,Studieteknik,,,,\nUndervisning,Matematik,Ma,2026-11-24,10:00,\nAssignment,Case,IØ,2026-11-30,,",
            true,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "lesson", "title": "Afsætning", "subject": "Afsætning", "subject_key": "afsætning", "date": "2026-11-23", "day": "Mandag", "time": "08:15", "end_time": null, "notes": "Lokale 3", "external_id": "529ed31b0e7b8ffc", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Marketing case", "subject": "Afsætning", "subject_key": "afsætning", "date": "2026-11-27", "day": "Fredag", "time": null, "end_time": null, "notes": null, "external_id": "1a19fe0e1783e8e0", "selected": true, "estimated_minutes": null }, { "kind": "subject", "title": "Virksomhedsøkonomi", "subject": "Virksomhedsøkonomi", "subject_key": "virksomhedsøkonomi", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "3fcd62b096ba1eae", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Ekskursion", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-11-25", "day": "Onsdag", "time": null, "end_time": null, "notes": "Husk madpakke", "external_id": "20d7a7b97f98dee5", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Studieteknik", "subject": "Selvstudie", "subject_key": "selvstudie", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "009435ab26ab8a5f", "selected": false, "estimated_minutes": null }, { "kind": "lesson", "title": "Matematik", "subject": "Matematik", "subject_key": "matematik", "date": "2026-11-24", "day": "Tirsdag", "time": "10:00", "end_time": null, "notes": null, "external_id": "318a72788db0124b", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Case", "subject": "International økonomi", "subject_key": "international økonomi", "date": "2026-11-30", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "7a340e953c662ed3", "selected": true, "estimated_minutes": null } ], "warnings": [ "1 lektioner mangler dato/ugedag og springes over." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_comma_csv_weekday_column() {
        check(
            "lectio",
            "Aktivitet,Hold,Ugedag,Fra,Til\nMatematik,1x Ma,mandag,10.00,11.30\nEngelsk,En B,Tue,12:30,13:15\nIdræt,id,FREDAG,14:00,\nMusik,mu,xyz,08:00,09:00\nKor,mu,,08:00,\nPrøve i fysik,fy,onsdag,,",
            false,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "lesson", "title": "Matematik", "subject": "Matematik", "subject_key": "matematik", "date": null, "day": "Mandag", "time": "10:00", "end_time": "11:30", "notes": null, "external_id": "157b6820c6a643f4", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Engelsk", "subject": "Engelsk", "subject_key": "engelsk", "date": null, "day": null, "time": "12:30", "end_time": "13:15", "notes": null, "external_id": "a575e186ac570e04", "selected": false, "estimated_minutes": null }, { "kind": "lesson", "title": "Idræt", "subject": "Idræt", "subject_key": "idræt", "date": null, "day": "Fredag", "time": "14:00", "end_time": null, "notes": null, "external_id": "eb39986bd78b8a8a", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Musik", "subject": "Musik", "subject_key": "musik", "date": null, "day": null, "time": "08:00", "end_time": "09:00", "notes": null, "external_id": "05d8e2925caddc46", "selected": false, "estimated_minutes": null }, { "kind": "lesson", "title": "Kor", "subject": "Musik", "subject_key": "musik", "date": null, "day": null, "time": "08:00", "end_time": null, "notes": null, "external_id": "4462e3f0ccc1c02a", "selected": false, "estimated_minutes": null }, { "kind": "deadline", "title": "Prøve i fysik", "subject": "Fysik", "subject_key": "fysik", "date": null, "day": "Onsdag", "time": null, "end_time": null, "notes": null, "external_id": "e4c862db12bb59ae", "selected": false, "estimated_minutes": null } ], "warnings": [ "1 deadlines have no date and are not selected. Fix the date in the file, or tick them to use today's date.", "3 lessons have no date/weekday and will be skipped." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_comma_csv_header_below_title_lines() {
        check(
            "lectio",
            "Lectio - Opgaver for 2a\nEksporteret 18/11-2026\n\nOpgavetitel,Hold,Frist\nNovelle,2a DA,25/11-2026 23:30\n,2a DA,26/11-2026\n   ,2a MA,\nRapport,2a BI,1/12-2026",
            true,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "deadline", "title": "Novelle", "subject": "Dansk", "subject_key": "dansk", "date": "2026-11-25", "day": "Onsdag", "time": "23:30", "end_time": null, "notes": null, "external_id": "058a0661c697385f", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Rapport", "subject": "Biologi", "subject_key": "biologi", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "aba172dcfb3bdbc6", "selected": true, "estimated_minutes": null } ], "warnings": [ "2 rækker uden titel blev sprunget over." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_comma_csv_crlf_and_quotes() {
        check(
            "lectio",
            "Opgave,Fag,Frist\r\n\"Essay, del 1\",Engelsk,2026-10-01\r\n\r\n\"Linje\r\nto\",Dansk,2026-10-02\r\n  \"Indrykket\"  ,Dansk,2026-10-03\r\nHalv\"citat,Dansk,2026-10-04\r\n\"Med \"\"citat\"\"\",Dansk,2026-10-05\r\n,,\r\n",
            false,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "deadline", "title": "Essay, del 1", "subject": "Engelsk", "subject_key": "engelsk", "date": "2026-10-01", "day": "Torsdag", "time": null, "end_time": null, "notes": null, "external_id": "7cb868d7aac1a777", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Linje\r\nto", "subject": "Dansk", "subject_key": "dansk", "date": "2026-10-02", "day": "Fredag", "time": null, "end_time": null, "notes": null, "external_id": "812690b48d918a80", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Indrykket", "subject": "Dansk", "subject_key": "dansk", "date": "2026-10-03", "day": "Lørdag", "time": null, "end_time": null, "notes": null, "external_id": "72663c5d733b7ca4", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Halv\"citat", "subject": "Dansk", "subject_key": "dansk", "date": "2026-10-04", "day": "Søndag", "time": null, "end_time": null, "notes": null, "external_id": "98f1c247b41740d0", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Med \"citat\"", "subject": "Dansk", "subject_key": "dansk", "date": "2026-10-05", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "5ad1e5e607bf183d", "selected": true, "estimated_minutes": null } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_as_old_version_comma_csv_estimates() {
        check(
            "lectio",
            "Opgave,Frist,Elevtid\nA,2026-12-01,\"3,00\"\nB,2026-12-01,0.75\nC,2026-12-01,12\nD,2026-12-01,13\nE,2026-12-01,abc\nF,2026-12-01,\nG,2026-12-01,1.5 timer\nH,2026-12-01,90 min\nI,2026-12-01,0\nJ,2026-12-01,\"2,5,0\"\nK,2026-12-01,0.0125",
            true,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "deadline", "title": "A", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "1a3f5dce6d636da6", "selected": true, "estimated_minutes": 180 }, { "kind": "deadline", "title": "B", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "62533f61582ace36", "selected": true, "estimated_minutes": 45 }, { "kind": "deadline", "title": "C", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "3abf515d06a14248", "selected": true, "estimated_minutes": 720 }, { "kind": "deadline", "title": "D", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "7262a8e9084b79d0", "selected": true, "estimated_minutes": 13 }, { "kind": "deadline", "title": "E", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "fa67475884e3f4b1", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "F", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "338ad90830441e98", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "G", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "5ab06ace707941a4", "selected": true, "estimated_minutes": 90 }, { "kind": "deadline", "title": "H", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "c1f7c4d4e74fc36c", "selected": true, "estimated_minutes": 90 }, { "kind": "deadline", "title": "I", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "1f84df92a373f598", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "J", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "fd67dec687fd2b4a", "selected": true, "estimated_minutes": 150 }, { "kind": "deadline", "title": "K", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "5be1d63175354e2b", "selected": true, "estimated_minutes": 1 } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_as_old_version_comma_csv_date_formats() {
        check(
            "lectio",
            "Opgave,Deadline\nIso,2026-09-14\nIso med tid,2026-09-14T08:15\nKort iso,2026-9-4\nSkråstreg bindestreg,14/9-2026\nBindestreger,14-09-2026\nPunktummer,14.09.2026 kl. 8.15\nKort år,14/09/26\nMåned kort,14. sep 2026\nMåned lang,3. December 2026 23:59\nMåned engelsk,5 Oct 2026\nMaj,1. maj 2027\nUgyldig måned,14/13/2026\nUgyldig tid,2026-09-14 25:00\nIngen dato,snart",
            true,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "deadline", "title": "Iso", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "4599b66322d65d1b", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Iso med tid", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": "08:15", "end_time": null, "notes": null, "external_id": "7c21d0a057523060", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Kort iso", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-04", "day": "Fredag", "time": null, "end_time": null, "notes": null, "external_id": "64a6c4f1068d94ba", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Skråstreg bindestreg", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "3051ec2d1ac17d9b", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Bindestreger", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "cdf276f6c272338b", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Punktummer", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": "08:15", "end_time": null, "notes": null, "external_id": "fd7184b4511ad15b", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Kort år", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "7dc25d0351b4d254", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Måned kort", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "ce6eb670931a235b", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Måned lang", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-03", "day": "Torsdag", "time": "23:59", "end_time": null, "notes": null, "external_id": "7c847d237ccdd700", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Måned engelsk", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-10-05", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "a322f5accff86f43", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Maj", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2027-05-01", "day": "Lørdag", "time": null, "end_time": null, "notes": null, "external_id": "6293df6651884334", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Ugyldig måned", "subject": "Selvstudie", "subject_key": "selvstudie", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "5f49ab54c46287b9", "selected": false, "estimated_minutes": null }, { "kind": "deadline", "title": "Ugyldig tid", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-14", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "fd57b3efb03d8c67", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Ingen dato", "subject": "Selvstudie", "subject_key": "selvstudie", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "69e2baa6e34c3674", "selected": false, "estimated_minutes": null } ], "warnings": [ "2 deadlines mangler en dato og er ikke valgt. Ret datoen i filen, eller vælg dem til, så får de dags dato." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_semicolon_csv_ludus_with_commas_in_text() {
        check(
            "ludus",
            "Dato;Tid;Sluttid;Hold;Aktivitet;Note\n14-09-2026;08:15;09:45;Dansk A;Dansk, grammatik;Lokale 2, 1. sal\n15-09-2026;10:00;11:30;Mat B;Matematik;\n16-09-2026;;;Hi;Historie, prøve;\n;12:00;13:00;Bio;Biologi;",
            true,
            r#"{ "detected_format": "CSV (semikolon)", "items": [ { "kind": "lesson", "title": "Dansk, grammatik", "subject": "Dansk", "subject_key": "dansk", "date": "2026-09-14", "day": "Mandag", "time": "08:15", "end_time": "09:45", "notes": "Lokale 2, 1. sal", "external_id": "342eee4de248392c", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Matematik", "subject": "Matematik", "subject_key": "matematik", "date": "2026-09-15", "day": "Tirsdag", "time": "10:00", "end_time": "11:30", "notes": null, "external_id": "a3fa8bca2cedf141", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Historie, prøve", "subject": "Historie", "subject_key": "historie", "date": "2026-09-16", "day": "Onsdag", "time": null, "end_time": null, "notes": null, "external_id": "b5c3d5d50ec38812", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Biologi", "subject": "Biologi", "subject_key": "biologi", "date": null, "day": null, "time": "12:00", "end_time": "13:00", "notes": null, "external_id": "0e88f0703bb10705", "selected": false, "estimated_minutes": null } ], "warnings": [ "1 lektioner mangler dato/ugedag og springes over." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_semicolon_csv_with_bom_and_quotes_english() {
        check(
            "ludus",
            "\u{feff}\"Aktivitet\";\"Hold\";\"Dato\";\"Start\";\"Slut\"\r\n\"Dansk modul\";\"Dansk A\";\"2026-09-14\";\"08:15\";\"09:45\"\r\n\"Engelsk; mundtlig\";\"Engelsk B\";\"2026-09-15\";\"12:00\";\"12:45\"\r\n",
            false,
            r#"{ "detected_format": "CSV (semicolon)", "items": [ { "kind": "lesson", "title": "Dansk modul", "subject": "Dansk", "subject_key": "dansk", "date": "2026-09-14", "day": "Mandag", "time": "08:15", "end_time": "09:45", "notes": null, "external_id": "1029cc35aee40f2a", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Engelsk; mundtlig", "subject": "Engelsk", "subject_key": "engelsk", "date": "2026-09-15", "day": "Tirsdag", "time": "12:00", "end_time": "12:45", "notes": null, "external_id": "fb1fe17546686b93", "selected": true, "estimated_minutes": null } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn json_array_with_numbers_bools_and_explicit_minutes() {
        check(
            "mitnielsbrock",
            "[{\"title\":\"Aflevering: Marketing case\",\"subject\":\"Afsætning\",\"due\":\"2026-09-20\",\"estimated_minutes\":90,\"done\":false},{\"title\":\"Afsætning modul\",\"subject\":\"af\",\"date\":\"2026-09-16\",\"time\":\"08:15\",\"end\":\"09:45\"},{\"title\":42,\"subject\":\"Matematik\",\"date\":\"2026-09-17\"},{\"title\":true,\"subject\":null,\"date\":\"2026-09-18\"},{\"title\":\"Halvanden time\",\"estimated_minutes\":1.5,\"date\":\"2026-09-19\"},{\"title\":\"Uden dato\",\"subject\":\"Engelsk\"},{\"title\":\"Lektion uden dag\",\"subject\":\"Engelsk\",\"time\":\"10:00\"},{\"subject\":\"Ingen titel\",\"date\":\"2026-09-21\"},null,7,\"tekst\"]",
            true,
            r#"{ "detected_format": "JSON", "items": [ { "kind": "deadline", "title": "Aflevering: Marketing case", "subject": "Afsætning", "subject_key": "afsætning", "date": "2026-09-20", "day": "Søndag", "time": null, "end_time": null, "notes": null, "external_id": "ddf9a2206fbf42d8", "selected": true, "estimated_minutes": 90 }, { "kind": "lesson", "title": "Afsætning modul", "subject": "Afsætning", "subject_key": "afsætning", "date": "2026-09-16", "day": "Onsdag", "time": "08:15", "end_time": "09:45", "notes": null, "external_id": "513d809513c69b09", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "42", "subject": "Matematik", "subject_key": "matematik", "date": "2026-09-17", "day": "Torsdag", "time": null, "end_time": null, "notes": null, "external_id": "27c9e743f8ff4adf", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "true", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-18", "day": "Fredag", "time": null, "end_time": null, "notes": null, "external_id": "c4b5192901c2753e", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "Halvanden time", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-09-19", "day": "Lørdag", "time": null, "end_time": null, "notes": null, "external_id": "e9beb1f05c7effa2", "selected": true, "estimated_minutes": 2 }, { "kind": "deadline", "title": "Uden dato", "subject": "Engelsk", "subject_key": "engelsk", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "68782d692f276489", "selected": false, "estimated_minutes": null }, { "kind": "lesson", "title": "Lektion uden dag", "subject": "Engelsk", "subject_key": "engelsk", "date": null, "day": null, "time": "10:00", "end_time": null, "notes": null, "external_id": "dbcb196df5b4f231", "selected": false, "estimated_minutes": null } ], "warnings": [ "1 deadlines mangler en dato og er ikke valgt. Ret datoen i filen, eller vælg dem til, så får de dags dato.", "1 lektioner mangler dato/ugedag og springes over." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_json_object_with_list_inside_english() {
        check(
            "lectio",
            "{\"school\":\"Niels Brock\",\"meta\":{\"pages\":[1,2]},\"count\":2,\"opgaver\":[{\"Opgavetitel\":\"Rapport\",\"Fag\":\"Fysik\",\"Frist\":\"1/12-2026 14:00\",\"Elevtid\":\"3,00\"},{\"Opgavetitel\":\"Lektier\",\"Fag\":\"2a DA\",\"Frist\":\"02.12.2026\",\"Opgavenote\":\"  Side 10-20  \"},{\"Opgavetitel\":\"Prøve\",\"Fag\":\"Ke\",\"Dato\":\"2026-12-03\",\"Kategori\":\"Lektion\",\"Tid\":9.3}]}",
            false,
            r#"{ "detected_format": "JSON", "items": [ { "kind": "deadline", "title": "Rapport", "subject": "Fysik", "subject_key": "fysik", "date": "2026-12-01", "day": "Tirsdag", "time": "14:00", "end_time": null, "notes": null, "external_id": "b146bf8fdab1424d", "selected": true, "estimated_minutes": 180 }, { "kind": "deadline", "title": "Lektier", "subject": "Dansk", "subject_key": "dansk", "date": "2026-12-02", "day": "Onsdag", "time": null, "end_time": null, "notes": "Side 10-20", "external_id": "0064b90c567bbe10", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Prøve", "subject": "Kemi", "subject_key": "kemi", "date": "2026-12-03", "day": "Torsdag", "time": null, "end_time": null, "notes": null, "external_id": "39b42b1649a10875", "selected": true, "estimated_minutes": null } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_as_old_version_json_single_object() {
        check(
            "mitnielsbrock",
            "{\"title\":\"Enkelt opgave\",\"date\":\"2026-11-30\",\"priority\":\"high\",\"timer\":2}",
            true,
            r#"{ "detected_format": "JSON", "items": [ { "kind": "deadline", "title": "Enkelt opgave", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-11-30", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "8f723b21c6d06b6c", "selected": true, "estimated_minutes": 120 } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_as_old_version_json_values_of_other_types() {
        check(
            "lectio",
            "[{\"title\":\"Tal som tid\",\"date\":20261130,\"time\":800,\"minutter\":45.5},{\"title\":\"Nul\",\"date\":\"2026-11-30\",\"minutter\":0},{\"title\":\" \",\"date\":\"2026-11-30\"},{\"titel\":\"Dansk titel\",\"fag\":\"en\",\"dato\":\"30/11/2026\",\"ugedag\":\"tirsdag\"}]",
            true,
            r#"{ "detected_format": "JSON", "items": [ { "kind": "deadline", "title": "Tal som tid", "subject": "Selvstudie", "subject_key": "selvstudie", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "52a30f457bea3e5a", "selected": false, "estimated_minutes": 46 }, { "kind": "deadline", "title": "Nul", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-11-30", "day": "Mandag", "time": null, "end_time": null, "notes": null, "external_id": "824db664aa802e39", "selected": true, "estimated_minutes": null }, { "kind": "lesson", "title": "Dansk titel", "subject": "Engelsk", "subject_key": "engelsk", "date": "2026-11-30", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "cdf187a2a078b5e0", "selected": true, "estimated_minutes": null } ], "warnings": [ "1 deadlines mangler en dato og er ikke valgt. Ret datoen i filen, eller vælg dem til, så får de dags dato." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_copied_lectio_table_full() {
        check(
            "lectio",
            "Opgaver\n\nUge\tHold\tOpgavetitel\tFrist\tElevtid\tStatus\tFravær\tAfventer\tOpgavenote\n47\t2a DA\tNovelleanalyse\t25/11-2026 23:30\t3,00\tVenter\t\t\tLæs side 4\n48\t2a Ma\tAflevering 4\t1/12-2026 22:00\t2,50\tAfleveret\t0%\t\t\n49\t2a EN\tEssay: \"Brave New World\"\t7/12-2026 08:00\t0,50\tVenter\t\t\t\n50\t2a BI\tBiologi rapport\t\t1,25\tVenter\t\t\t\n51\t2a SA\t\t9/12-2026 12:00\t1,00\tVenter\t\t\t",
            true,
            r#"{ "detected_format": "Kopieret tabel", "items": [ { "kind": "deadline", "title": "Novelleanalyse", "subject": "Dansk", "subject_key": "dansk", "date": "2026-11-25", "day": "Onsdag", "time": "23:30", "end_time": null, "notes": "Læs side 4", "external_id": "ade57191da92744a", "selected": true, "estimated_minutes": 180 }, { "kind": "deadline", "title": "Aflevering 4", "subject": "Matematik", "subject_key": "matematik", "date": "2026-12-01", "day": "Tirsdag", "time": "22:00", "end_time": null, "notes": null, "external_id": "73cabf8b573aa139", "selected": true, "estimated_minutes": 150 }, { "kind": "deadline", "title": "Essay: \"Brave New World\"", "subject": "Engelsk", "subject_key": "engelsk", "date": "2026-12-07", "day": "Mandag", "time": "08:00", "end_time": null, "notes": null, "external_id": "3c2b01a381d857a9", "selected": true, "estimated_minutes": 30 }, { "kind": "deadline", "title": "Biologi rapport", "subject": "Biologi", "subject_key": "biologi", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "3a363f62345799b7", "selected": false, "estimated_minutes": 75 } ], "warnings": [ "1 rækker uden titel blev sprunget over.", "1 deadlines mangler en dato og er ikke valgt. Ret datoen i filen, eller vælg dem til, så får de dags dato." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_copied_lectio_table_english() {
        check(
            "lectio",
            "Uge\tHold\tOpgavetitel\tFrist\tElevtid\tStatus\n47\t2a DA\tNovelleanalyse\t25/11-2026 23:30\t3,00\tVenter\n48\t2a Ma\tAflevering 4\t\t2,50\tVenter",
            false,
            r#"{ "detected_format": "Copied table", "items": [ { "kind": "deadline", "title": "Novelleanalyse", "subject": "Dansk", "subject_key": "dansk", "date": "2026-11-25", "day": "Onsdag", "time": "23:30", "end_time": null, "notes": null, "external_id": "ade57191da92744a", "selected": true, "estimated_minutes": 180 }, { "kind": "deadline", "title": "Aflevering 4", "subject": "Matematik", "subject_key": "matematik", "date": null, "day": null, "time": null, "end_time": null, "notes": null, "external_id": "34cf24f675a0b77e", "selected": false, "estimated_minutes": 150 } ], "warnings": [ "1 deadlines have no date and are not selected. Fix the date in the file, or tick them to use today's date." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_only_a_header() {
        check(
            "lectio",
            "Opgave,Fag,Frist",
            true,
            r#"{ "detected_format": "CSV", "items": [], "warnings": [ "Der blev ikke fundet noget at importere." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_only_a_header_english() {
        check(
            "ludus",
            "Opgave;Fag;Frist\n",
            false,
            r#"{ "detected_format": "CSV (semicolon)", "items": [], "warnings": [ "Nothing to import was found." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_empty_json() {
        check(
            "lectio",
            "{}",
            false,
            r#"{ "detected_format": "JSON", "items": [], "warnings": [ "Nothing to import was found." ] }"#,
        );
    }

    #[test]
    fn times_before_and_after_iso_dates_are_read_at_their_full_offsets() {
        assert_eq!(parse_date("æøå kl. 10.30 2026-09-14").1.as_deref(), Some("10:30"));
        assert_eq!(parse_date("Afl. kl. 12.00 2026-09-14").1.as_deref(), Some("12:00"));
        assert_eq!(parse_date("Lørdag 2026-09-19 kl. 23:59").1.as_deref(), Some("23:59"));
    }

    #[test]
    fn same_as_old_version_json_arrays_and_objects_as_values() {
        check(
            "mitnielsbrock",
            "[{\"title\":[\"Del 1\",\"Del 2\"],\"date\":\"2026-11-30\",\"notes\":[\"a\",null,[\"b\",\"c\"]],\"subject\":{\"name\":\"Dansk\"}}]",
            true,
            r#"{ "detected_format": "JSON", "items": [ { "kind": "deadline", "title": "Del 1,Del 2", "subject": "[object Object]", "subject_key": "[object object]", "date": "2026-11-30", "day": "Mandag", "time": null, "end_time": null, "notes": "a,,b,c", "external_id": "e81844c0cc757541", "selected": true, "estimated_minutes": null } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_as_old_version_subjects_are_cleaned_like_before() {
        check(
            "lectio",
            "Opgave,Hold,Frist\nA,Natur & teknik B,2026-12-01\nB,1.g Dansk A,2026-12-01\nC,2a vø,2026-12-01\nD,3b IØ,2026-12-01\nE,matematik,2026-12-01\nF,Idræt C,2026-12-01\nG,Fysik A ,2026-12-01\nH,  ,2026-12-01\nI,æstetik,2026-12-01\nJ,SAMF,2026-12-01\nK,Ma A,2026-12-01\nL,A,2026-12-01",
            true,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "deadline", "title": "A", "subject": "Natur & teknik", "subject_key": "natur_teknik", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "1a3f5dce6d636da6", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "B", "subject": "Dansk", "subject_key": "dansk", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "62533f61582ace36", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "C", "subject": "Virksomhedsøkonomi", "subject_key": "virksomhedsøkonomi", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "3abf515d06a14248", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "D", "subject": "International økonomi", "subject_key": "international økonomi", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "7262a8e9084b79d0", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "E", "subject": "Matematik", "subject_key": "matematik", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "fa67475884e3f4b1", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "F", "subject": "Idræt", "subject_key": "idræt", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "338ad90830441e98", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "G", "subject": "Fysik", "subject_key": "fysik", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "5ab06ace707941a4", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "H", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "c1f7c4d4e74fc36c", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "I", "subject": "Æstetik", "subject_key": "æstetik", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "1f84df92a373f598", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "J", "subject": "Samfundsfag", "subject_key": "samfundsfag", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "fd67dec687fd2b4a", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "K", "subject": "Matematik", "subject_key": "matematik", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "5be1d63175354e2b", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "L", "subject": "A", "subject_key": "a", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "a94b0fd58b846989", "selected": true, "estimated_minutes": null } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_as_old_version_rows_without_title_english() {
        check(
            "ludus",
            "Aktivitet;Dato\nA;2026-12-01\n;2026-12-02\n;2026-12-03\nB;2026-12-04",
            false,
            r#"{ "detected_format": "CSV (semicolon)", "items": [ { "kind": "deadline", "title": "A", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "7d5c5ab4fc781bf9", "selected": true, "estimated_minutes": null }, { "kind": "deadline", "title": "B", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-04", "day": "Fredag", "time": null, "end_time": null, "notes": null, "external_id": "a318f599dffaac8f", "selected": true, "estimated_minutes": null } ], "warnings": [ "2 rows without a title were skipped." ] }"#,
        );
    }

    #[test]
    fn same_as_old_version_delimiter_tie_uses_comma() {
        check(
            "lectio",
            "Opgave,Fag;Frist\nA,Dansk;2026-12-01",
            true,
            r#"{ "detected_format": "CSV", "items": [ { "kind": "deadline", "title": "A", "subject": "Selvstudie", "subject_key": "selvstudie", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "1a3f5dce6d636da6", "selected": true, "estimated_minutes": null } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_as_old_version_tab_wins_over_commas() {
        check(
            "lectio",
            "Opgavetitel\tHold\tFrist\nA, B, C\t2a DA\t1/12-2026",
            false,
            r#"{ "detected_format": "Copied table", "items": [ { "kind": "deadline", "title": "A, B, C", "subject": "Dansk", "subject_key": "dansk", "date": "2026-12-01", "day": "Tirsdag", "time": null, "end_time": null, "notes": null, "external_id": "9ce11a9ae0d26649", "selected": true, "estimated_minutes": null } ], "warnings": [] }"#,
        );
    }

    #[test]
    fn same_error_as_old_version_no_title_column() {
        assert_eq!(
            parse_import("lectio", "a,b,c\n1,2,3", true).unwrap_err(),
            "Kunne ikke finde en kolonne med titler. Første linje skal være overskrifter, fx 'Opgave,Fag,Frist'."
        );
    }

    #[test]
    fn same_error_as_old_version_no_title_column_english() {
        assert_eq!(
            parse_import("ludus", "Dato;Fag\n2026-09-14;Dansk", false).unwrap_err(),
            "Could not find a title column. The first line should be headers, e.g. 'Task,Subject,Deadline'."
        );
    }

    #[test]
    fn same_error_as_old_version_header_after_row_five() {
        assert_eq!(
            parse_import("lectio", "a\nb\nc\nd\ne\nOpgave,Fag\nX,Y", true).unwrap_err(),
            "Kunne ikke finde en kolonne med titler. Første linje skal være overskrifter, fx 'Opgave,Fag,Frist'."
        );
    }

    #[test]
    fn same_error_as_old_version_bad_json() {
        assert_eq!(
            parse_import("lectio", "[{\"title\": \"x\"", true).unwrap_err(),
            "JSON-teksten kunne ikke læses. Tjek at den er kopieret helt."
        );
    }

    #[test]
    fn same_error_as_old_version_bad_json_english() {
        assert_eq!(
            parse_import("mitnielsbrock", "{bad json}", false).unwrap_err(),
            "The JSON could not be read. Check that it was copied completely."
        );
    }

    #[test]
    fn ics_events_same_as_old_version() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:abc@lectio\r\nSUMMARY:Aflevering: Dansk\r\n  analyse\r\nDTSTART;VALUE=DATE:20261125\r\nDTEND;VALUE=DATE:20261126\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:les1\r\nSUMMARY:2a MA\\, modul 3; lokale 12\r\nDESCRIPTION:Lokale 12\\nTag lommeregner\\N\\\\slut\r\nDTSTART;TZID=Europe/Copenhagen:20261123T081500\r\nDTEND;TZID=Europe/Copenhagen:20261123T094500\r\nEND:VEVENT\r\nBEGIN:VEVENT\nSUMMARY:Uden uid\nDTSTART:20261124T1000\nDTEND;VALUE=DATE-TIME:20261124T113000\nEND:VEVENT\nBEGIN:VEVENT\nUID:\nSUMMARY:Tom uid\ndtstart:20261127T120000\nEND:VEVENT\nBEGIN:VEVENT\nUID:dato-param\nSUMMARY:Heldagsprøve\nDTSTART;VALUE=DATE:20261128T090000\nEND:VEVENT\nBEGIN:VEVENT\nUID:ingen-start\nSUMMARY:Uden start\nEND:VEVENT\nBEGIN:VEVENT\nUID:ingen-titel\nDTSTART:20261129T080000\nEND:VEVENT\nBEGIN:VEVENT\nUID:folded\nSUMMARY:Lang\n\ttitel med tab\nDTSTART:20261130T081500\nDTEND:20261130\nEND:VEVENT\nEND:VCALENDAR\r\n";
        let expected = vec![
            IcsEvent { recurrence_id: None, uid: "abc@lectio".into(), summary: "Aflevering: Dansk analyse".into(), description: "".into(), date: "2026-11-25".into(), time: None, end_time: None, all_day: true },
            IcsEvent { recurrence_id: None, uid: "les1".into(), summary: "2a MA, modul 3; lokale 12".into(), description: "Lokale 12\nTag lommeregner\n\\slut".into(), date: "2026-11-23".into(), time: Some(copenhagen_time(8, 15)), end_time: Some(copenhagen_time(9, 45)), all_day: false },
            IcsEvent { recurrence_id: None, uid: "659ef6710c8d544d".into(), summary: "Uden uid".into(), description: "".into(), date: "2026-11-24".into(), time: Some("10:00".into()), end_time: Some("11:30".into()), all_day: false },
            IcsEvent { recurrence_id: None, uid: "2db290d55b327546".into(), summary: "Tom uid".into(), description: "".into(), date: "2026-11-27".into(), time: Some("12:00".into()), end_time: None, all_day: false },
            IcsEvent { recurrence_id: None, uid: "dato-param".into(), summary: "Heldagsprøve".into(), description: "".into(), date: "2026-11-28".into(), time: None, end_time: None, all_day: true },
            IcsEvent { recurrence_id: None, uid: "folded".into(), summary: "Langtitel med tab".into(), description: "".into(), date: "2026-11-30".into(), time: Some("08:15".into()), end_time: None, all_day: false },
        ];
        assert_eq!(parse_ics(ics, Local::now().date_naive()).unwrap(), expected);
    }
}
