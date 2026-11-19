//! The school window: Ludus and Mit Niels Brock open in a separate, private Kladde window.
//! The user logs in there themselves. A small "Hent afleveringer" button is added to those
//! sites; pressing it hands the visible page (its tables and text) to Kladde, which turns it
//! into the normal import preview. Nothing is saved until the user confirms in Kladde.
//!
//! Safety:
//! - The school window has no Tauri capabilities, so school pages cannot call Kladde's commands.
//!   The page is handed over by a navigation to a reserved, never-resolving address
//!   (https://kladde-extract.invalid/), which Kladde intercepts and cancels.
//! - Pages are only read on the allowed school sites, checked against the window's real address.
//! - The window is private (incognito): logins and cookies are forgotten when it closes.
//! - Kladde never sees the password; it only reads the page the user chose, when they press the button.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use chrono::Local;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::ai::{generate, AiState};
use crate::commands::{err, is_da, mark_known, str_of, tr, AppState, Res};
use crate::importers::{clean_subject, ext_id, parse_import, parse_time, ImportItem, ImportPreview};
use crate::store::{day_name, is_valid_date, local_date};

const LABEL: &str = "school";
const EXTRACT_HOST: &str = "kladde-extract.invalid";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Site {
    Ludus,
    MitNielsBrock,
}

impl Site {
    fn from_name(name: &str) -> Option<Site> {
        match name {
            "ludus" => Some(Site::Ludus),
            "mitnielsbrock" => Some(Site::MitNielsBrock),
            _ => None,
        }
    }
    fn source(self) -> &'static str {
        match self {
            Site::Ludus => "ludus",
            Site::MitNielsBrock => "mitnielsbrock",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Site::Ludus => "Ludus",
            Site::MitNielsBrock => "Mit Niels Brock",
        }
    }
    fn start_url(self) -> String {
        if let Some(origin) = test_origin() {
            return format!("{origin}/{}.html", self.source());
        }
        match self {
            Site::Ludus => "https://nielsbrock.luduseg.dk/ui/main".into(),
            Site::MitNielsBrock => "https://www.mitnielsbrock.dk/login/index.php".into(),
        }
    }
}

/// Only the automated tests point the school window at local stand-in pages.
#[cfg(debug_assertions)]
fn test_origin() -> Option<String> {
    let origin = Url::parse(&std::env::var("KLADDE_SCHOOL_TEST_ORIGIN").ok()?).ok()?;
    let local = match origin.host()? {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(s) => s == "localhost",
    };
    (local && matches!(origin.scheme(), "http" | "https")).then(|| origin.origin().ascii_serialization())
}

#[cfg(not(debug_assertions))]
fn test_origin() -> Option<String> { None }

/// Which school site an address belongs to, if it is one Kladde may read.
pub fn site_of(url: &Url) -> Option<Site> {
    if let Some(origin) = test_origin().and_then(|o| Url::parse(&o).ok()) {
        if url.origin() == origin.origin() {
            return Some(if url.path().contains("ludus") { Site::Ludus } else { Site::MitNielsBrock });
        }
    }
    if url.scheme() != "https" || url.port_or_known_default() != Some(443) || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    match url.host_str()? {
        "nielsbrock.luduseg.dk" => Some(Site::Ludus),
        "www.mitnielsbrock.dk" | "mitnielsbrock.dk" => Some(Site::MitNielsBrock),
        _ => None,
    }
}

fn allowed_origins() -> Vec<String> {
    let mut hosts: Vec<String> = ["https://nielsbrock.luduseg.dk", "https://www.mitnielsbrock.dk", "https://mitnielsbrock.dk"].map(String::from).to_vec();
    if let Some(host) = test_origin().and_then(|o| Url::parse(&o).ok()).map(|u| u.origin().ascii_serialization()) {
        hosts.push(host);
    }
    hosts
}

/// The floating "Hent afleveringer" button, added only on the school sites.
fn page_script(da: bool, handoff_path: &str) -> String {
    let hosts = serde_json::to_string(&allowed_origins()).unwrap_or_else(|_| "[]".into());
    let (button, reading, hint) = if da {
        ("Hent afleveringer fra denne side", "Læser siden…", "Kladde læser kun denne side, når du trykker.")
    } else {
        ("Get assignments from this page", "Reading the page…", "Kladde only reads this page when you press.")
    };
    format!(
        r#"(() => {{
  if (window.top !== window || !{hosts}.includes(location.origin)) return;
  const tableText = (table) => [...table.rows].map((row) =>
    [...row.cells].map((c) => (c.innerText || "").replace(/[\t\r\n]+/g, " ").trim()).join("\t")).join("\n");
  const collect = (doc, out) => {{
    for (const t of doc.querySelectorAll("table")) out.tables.push(tableText(t));
    out.text.push((doc.body && doc.body.innerText) || "");
    for (const f of doc.querySelectorAll("iframe, frame")) {{
      try {{ if (f.contentDocument) collect(f.contentDocument, out); }} catch (e) {{ /* other site: skip */ }}
    }}
  }};
  // A table whose header names both a task and a date is an assignment list: read it by itself.
  const looksLikeDeadlines = (t) => {{
    const head = (t.rows[0] ? t.rows[0].innerText : "").toLowerCase();
    return /opgave|titel|title|aktivitet|aflevering/.test(head) && /frist|deadline|dato|date|due|aflevering/.test(head) && t.rows.length > 1;
  }};
  let lastAuto = "";
  const send = (auto) => {{
    const out = {{ tables: [], text: [] }};
    collect(document, out);
    const payload = {{ auto, title: document.title.slice(0, 300), tables: out.tables.join("\u0001").slice(0, 150000).split("\u0001"),
      text: auto ? "" : out.text.join("\n\n").slice(0, 60000) }};
    location.href = "https://{EXTRACT_HOST}{handoff_path}?d=" + encodeURIComponent(JSON.stringify(payload));
  }};
  const autoCheck = () => {{
    const out = {{ tables: [], text: [] }};
    const found = [];
    const scan = (doc) => {{
      for (const t of doc.querySelectorAll("table")) if (looksLikeDeadlines(t)) found.push(t.innerText.length + ":" + t.innerText.slice(0, 200));
      for (const f of doc.querySelectorAll("iframe, frame")) {{ try {{ if (f.contentDocument) scan(f.contentDocument); }} catch (e) {{}} }}
    }};
    scan(document);
    const key = location.href + "|" + found.join("|");
    if (found.length && key !== lastAuto) {{ lastAuto = key; send(true); }}
    void out;
  }};
  const start = () => {{
    if (document.getElementById("kladde-school-bar")) return;
    const host = document.createElement("div");
    host.id = "kladde-school-bar";
    host.style.cssText = "position:fixed;right:16px;bottom:16px;z-index:2147483647";
    const root = host.attachShadow({{ mode: "open" }});
    root.innerHTML = `<style>
      .box{{font:13px system-ui,sans-serif;background:#0b1d3a;color:#fff;border-radius:14px;padding:10px 12px;box-shadow:0 8px 24px rgba(0,0,0,.25);max-width:320px}}
      button{{font:600 13px system-ui,sans-serif;background:#2563eb;color:#fff;border:0;border-radius:10px;padding:8px 12px;cursor:pointer;width:100%}}
      button:disabled{{opacity:.6;cursor:default}} p{{margin:6px 0 0;color:#cbd5e1;font-size:12px}}</style>
      <div class="box"><button id="go">{button}</button><p id="msg">{hint}</p></div>`;
    const go = root.getElementById("go"), msg = root.getElementById("msg");
    window.__kladdeStatus = (text, done) => {{ msg.textContent = text; if (done) go.disabled = false; }};
    go.addEventListener("click", () => {{ go.disabled = true; msg.textContent = "{reading}"; send(false); }});
    document.documentElement.appendChild(host);
    // Pages that load their lists later (or change without reloading) are checked every 2 seconds.
    autoCheck();
    setInterval(autoCheck, 2000);
  }};
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();
}})();"#
    )
}

#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct PagePayload {
    /// True when the page was read by itself because it shows an assignment list.
    pub auto: bool,
    pub title: String,
    pub tables: Vec<String>,
    pub text: String,
}

pub fn decode_payload(url: &Url) -> Option<PagePayload> {
    if url.as_str().len() > 2 * 1024 * 1024 { return None }
    let data = url.query_pairs().find(|(k, _)| k == "d")?.1.into_owned();
    let page: PagePayload = serde_json::from_str(&data).ok()?;
    (page.title.chars().count() <= 300 && page.tables.len() <= 512
        && page.text.chars().count() <= 60_000
        && page.tables.iter().map(|s| s.chars().count()).sum::<usize>() <= 150_000).then_some(page)
}

/// Deadlines found in clear tables (columns like Opgave / Fag / Frist). No AI involved.
pub fn items_from_tables(site: Site, tables: &[String], today: &str, da: bool) -> Vec<ImportItem> {
    let mut items: Vec<ImportItem> = vec![];
    for table in tables.iter().filter(|t| t.lines().count() >= 2) {
        let Ok(preview) = parse_import(site.source(), table, da) else { continue };
        for item in preview.items {
            let upcoming = item.kind == "deadline" && item.date.as_deref().is_some_and(|d| d >= today);
            if upcoming && !items.iter().any(|i| i.external_id == item.external_id && i.subject == item.subject) {
                items.push(item);
            }
        }
    }
    items
}

/// Builds import rows from the AI's answer, keeping only real, upcoming dates.
pub fn items_from_ai(site: Site, raw: &Value, today: &str) -> Vec<ImportItem> {
    let mut items: Vec<ImportItem> = vec![];
    for a in raw["items"].as_array().into_iter().flatten() {
        let title: String = a["title"].as_str().unwrap_or("").trim().chars().take(200).collect();
        let date = a["deadline"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() || !is_valid_date(&date) || date.as_str() < today {
            continue;
        }
        let time = a["time"].as_str().and_then(parse_time);
        let subject = clean_subject(a["subject"].as_str().unwrap_or(""));
        let notes: String = a["notes"].as_str().unwrap_or("").trim().chars().take(300).collect();
        let external_id = ext_id(&[Some(site.source()), Some("deadline"), Some(&title), Some(&date), time.as_deref()]);
        if items.iter().any(|i| i.external_id.as_deref() == Some(external_id.as_str()) && i.subject == subject) {
            continue;
        }
        items.push(ImportItem {
            kind: "deadline".into(),
            subject_key: subject.to_lowercase().replace(" & ", "_"),
            subject,
            day: day_name(&date).map(String::from),
            date: Some(date),
            time,
            end_time: None,
            notes: (!notes.is_empty()).then_some(notes),
            external_id: Some(external_id),
            selected: true,
            estimated_minutes: None,
            title,
        });
    }
    items.sort_by(|a, b| a.date.cmp(&b.date));
    items
}

fn ai_extract(app: &AppHandle, site: Site, page: &PagePayload, today: &str, lang: &Option<String>) -> Res<Vec<ImportItem>> {
    let settings = app.state::<AiState>().get();
    let system = "You read one page from a Danish school system (Ludus or Moodle/Mit Niels Brock) that a student opened. \
List the student's upcoming hand-ins (afleveringer, opgaver, deadlines) and concrete homework they must do by a date. \
Skip general information, events and anything without a date. Dates on the page may look like 30.11.26, 30/11-2026 or 30. nov. 2026; \
write them as YYYY-MM-DD. Only include dates from today onwards. Keep titles short (max 80 characters) and in the page's language. \
subject is the course name as written on the page. notes is at most one short sentence with what to bring or do, or empty.";
    let user = format!(
        "Today is {today}.\nPage title: {}\n\nTables on the page (tab separated):\n{}\n\nPage text:\n{}",
        page.title,
        page.tables.join("\n\n").chars().take(40_000).collect::<String>(),
        page.text.chars().take(30_000).collect::<String>()
    );
    let schema = json!({
        "type": "object", "additionalProperties": false,
        "properties": { "items": { "type": "array", "items": {
            "type": "object", "additionalProperties": false,
            "properties": {
                "title": { "type": "string" }, "subject": { "type": "string" },
                "deadline": { "type": "string" }, "time": { "type": "string" }, "notes": { "type": "string" }
            },
            "required": ["title", "subject", "deadline", "time", "notes"]
        }}},
        "required": ["items"]
    });
    let text = generate(app, &settings, system, vec![json!({ "role": "user", "content": user })], Some(schema), lang)?;
    let raw: Value = serde_json::from_str(text.trim_start_matches("```json").trim_matches('`').trim()).map_err(|_| {
        err(502, tr(lang, "AI svarede i et uventet format. Prøv igen.", "AI answered in an unexpected format. Try again."))
    })?;
    let _ = site;
    Ok(items_from_ai(site, &raw, today))
}

fn ai_configured(app: &AppHandle) -> bool {
    app.state::<AiState>().get().is_configured()
}

/// Reads one handed-over page and sends the preview (or a clear error) to Kladde's main window.
fn handle_page(app: AppHandle, page_url: Option<Url>, payload: Option<PagePayload>, lang: Option<String>) {
    let da = is_da(&lang);
    let status = |text: &str| {
        if let Some(w) = app.get_webview_window(LABEL) {
            let _ = w.eval(format!("window.__kladdeStatus && window.__kladdeStatus({}, true)", json!(text)));
        }
    };
    let fail = |message: String| {
        status(&message);
        let _ = app.emit_to("main", "school-import-error", json!({ "detail": message }));
    };
    let Some(site) = page_url.as_ref().and_then(site_of) else {
        return fail(tr(&lang, "Kladde læser kun sider fra Ludus og Mit Niels Brock.", "Kladde only reads pages from Ludus and Mit Niels Brock."));
    };
    let Some(page) = payload else {
        return fail(tr(&lang, "Siden kunne ikke læses. Prøv igen.", "The page could not be read. Try again."));
    };
    let today = local_date(&Local::now());
    let mut items = items_from_tables(site, &page.tables, &today, da);
    let mut format = tr(&lang, "tabel på siden", "table on the page");
    if items.is_empty() && page.auto {
        // Read by itself but nothing upcoming: stay quiet. The AI is only used after a click.
        return;
    }
    if items.is_empty() {
        if !ai_configured(&app) {
            return fail(tr(
                &lang,
                "Der er ingen tydelig tabel med afleveringer på denne side. Slå AI til under AI-hjælper, så kan Kladde også læse sider som denne – eller gå til siden med afleveringsfrister.",
                "There is no clear table of assignments on this page. Switch on AI under AI helper so Kladde can read pages like this – or open the page with deadlines.",
            ));
        }
        let confirmed = app.dialog().message(tr(&lang,
            "Send denne sides titel, tabeller og tekst til din valgte AI-udbyder?",
            "Send this page's title, tables and text to your selected AI provider?"))
            .title(site.label()).buttons(MessageDialogButtons::OkCancel).blocking_show();
        if !confirmed {
            status(&tr(&lang, "Annulleret", "Cancelled"));
            return;
        }
        status(&tr(&lang, "AI læser siden…", "AI is reading the page…"));
        match ai_extract(&app, site, &page, &today, &lang) {
            Ok(found) => items = found,
            Err(e) => return fail(e.detail),
        }
        format = tr(&lang, "læst af AI", "read by AI");
    }
    let count = items.len();
    let mut preview = ImportPreview {
        source: site.source().into(),
        detected_format: format!("{} · {format}", site.label()),
        items,
        warnings: vec![],
    };
    if count == 0 {
        preview.warnings.push(tr(&lang, "Der blev ikke fundet kommende afleveringer på denne side.", "No upcoming assignments were found on this page."));
    }
    {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap_or_else(|p| p.into_inner());
        mark_known(&store.data, &mut preview, &lang);
    }
    let mut out = serde_json::to_value(&preview).unwrap_or_else(|_| json!({}));
    out["source_label"] = json!(site.label());
    let _ = app.emit_to("main", "school-import", out);
    status(&if count == 0 {
        tr(&lang, "Ingen kommende afleveringer fundet her. Prøv siden med frister.", "No upcoming assignments found here. Try the page with deadlines.")
    } else if da {
        format!("{count} fundet – tjek og gem dem i Kladde.")
    } else {
        format!("{count} found – review and save them in Kladde.")
    });
}

/// WebView2 needs the UI thread to process messages while a new window is created.
/// Keep this command asynchronous, including the first click before the user signs in.
#[tauri::command]
pub async fn open_school(app: AppHandle, lang: Option<String>, body: Value) -> Res<Value> {
    let Some(site) = Site::from_name(&str_of(body.get("site"), 20)) else {
        return Err(err(400, "Unknown school site"));
    };
    let url = Url::parse(&site.start_url()).map_err(|e| err(500, e.to_string()))?;
    if let Some(w) = app.get_webview_window(LABEL) {
        w.navigate(url).map_err(|e| err(500, e.to_string()))?;
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(json!({ "opened": site.source() }));
    }
    // Older WebView2 falls back to a non-private controller. The frontend targets
    // Chromium 110, which also includes the controller options needed for InPrivate.
    #[cfg(windows)]
    if tauri::webview_version().ok().and_then(|v| v.split('.').next()?.parse::<u32>().ok()).unwrap_or(0) < 110 {
        return Err(err(400, tr(&lang, "Opdatér Microsoft Edge WebView2 for at åbne det private skolevindue.",
            "Update Microsoft Edge WebView2 to open the private school window.")));
    }
    let nav_app = app.clone();
    let popup_app = app.clone();
    let nav_lang = lang.clone();
    let processing = Arc::new(AtomicBool::new(false));
    // The nonce lives in the trusted top frame's collector. A cross-origin
    // subframe cannot hand off its own content under the parent page's address.
    let handoff_path = format!("/{}", crate::store::new_id());
    WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::External(url))
        .title(format!("Kladde · {}", site.label()))
        .inner_size(1180.0, 820.0)
        .min_inner_size(640.0, 480.0)
        .incognito(true)
        .initialization_script(page_script(is_da(&lang), &handoff_path))
        .on_navigation(move |url| {
            if url.host_str() == Some(EXTRACT_HOST) {
                if url.scheme() != "https" || url.path() != handoff_path || url.as_str().len() > 2 * 1024 * 1024 { return false }
                let page_url = nav_app.get_webview_window(LABEL).and_then(|w| w.url().ok());
                if page_url.as_ref().and_then(site_of).is_none() || processing.swap(true, Ordering::AcqRel) {
                    return false;
                }
                let app = nav_app.clone();
                let lang = nav_lang.clone();
                let busy = processing.clone();
                let handoff = url.clone();
                if std::thread::Builder::new().spawn(move || {
                    handle_page(app, page_url, decode_payload(&handoff), lang);
                    busy.store(false, Ordering::Release);
                }).is_err() {
                    processing.store(false, Ordering::Release);
                }
                return false;
            }
            // Logging in may pass through other sign-in sites; anything but web pages is refused.
            matches!(url.scheme(), "https" | "http") && (site_of(url).is_some() || !crate::is_internal(url))
        })
        .on_new_window(move |url, _features| {
            if site_of(&url).is_some() {
                if let Some(w) = popup_app.get_webview_window(LABEL) {
                    let _ = w.navigate(url);
                }
            } else {
                let _ = crate::open_url(popup_app.clone(), url.to_string());
            }
            NewWindowResponse::Deny
        })
        .build()
        .map_err(|e| err(500, e.to_string()))?;
    Ok(json!({ "opened": site.source() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn only_the_school_sites_may_be_read() {
        assert_eq!(site_of(&url("https://nielsbrock.luduseg.dk/ui/main")), Some(Site::Ludus));
        assert_eq!(site_of(&url("https://www.mitnielsbrock.dk/my/")), Some(Site::MitNielsBrock));
        assert_eq!(site_of(&url("http://www.mitnielsbrock.dk/my/")), None, "plain http is not trusted");
        assert_eq!(site_of(&url("https://login.microsoftonline.com/")), None);
        assert_eq!(site_of(&url("https://mitnielsbrock.dk.evil.example/")), None);
        assert_eq!(site_of(&url("https://evil.example/nielsbrock.luduseg.dk")), None);
    }

    #[test]
    fn page_payload_round_trip() {
        let payload = json!({"title": "Deadlines", "tables": ["Opgave\tFag\tFrist"], "text": "Æblegrød & 100%"});
        let mut u = url("https://kladde-extract.invalid/");
        u.query_pairs_mut().append_pair("d", &payload.to_string());
        let p = decode_payload(&u).unwrap();
        assert_eq!((p.title.as_str(), p.tables.len(), p.text.as_str()), ("Deadlines", 1, "Æblegrød & 100%"));
        assert!(decode_payload(&url("https://kladde-extract.invalid/?d=not-json")).is_none());
    }

    #[test]
    fn moodle_deadline_table_is_read_without_ai() {
        // Like Mit Niels Brock's "Deadlines" page: Opgave, Fag and a date column.
        let table = "Opgave\tFag\tAfleveringsfrist\n\
            Writing exercise 1 - pictures\tEngelsk 1o\t25. november 2026, 23:59\n\
            Aflevering af Projekt 1\tInformatik 2026\t28. november 2026, 12:00\n\
            Anden aflevering 1.G\t1.O Tysk fortsætter\t2. december 2026, 23:59\n\
            Gammel opgave\tDansk\t1. november 2026, 23:59"
            .to_string();
        let items = items_from_tables(Site::MitNielsBrock, &[table, "kun én linje".into()], "2026-11-18", true);
        let got: Vec<(&str, &str, &str, Option<&str>)> =
            items.iter().map(|i| (i.title.as_str(), i.subject.as_str(), i.date.as_deref().unwrap(), i.time.as_deref())).collect();
        assert_eq!(
            got,
            [
                ("Writing exercise 1 - pictures", "Engelsk", "2026-11-25", Some("23:59")),
                ("Aflevering af Projekt 1", "Informatik", "2026-11-28", Some("12:00")),
                ("Anden aflevering 1.G", "Tysk", "2026-12-02", Some("23:59")),
            ],
            "past deadlines are left out"
        );
    }

    #[test]
    fn ludus_lesson_notes_have_no_title_column_so_they_need_ai() {
        let table = "Dato\tUgedag\tFag\tLektier\n30.11.26\tMandag\t1o JTP Informatik C - j26ososh\tLektier: Modul 18 Innovation";
        assert!(items_from_tables(Site::Ludus, &[table.to_string()], "2026-11-18", true).is_empty());
    }

    #[test]
    fn ai_answer_is_checked_and_tidied() {
        let raw = json!({ "items": [
            { "title": "Medbring høretelefoner til screening", "subject": "1o JTP Læsevejledning - j26ososh", "deadline": "2026-11-24", "time": "", "notes": "Tag computer med." },
            { "title": "Kapitel 6, afsnit 6.3 om budgetter", "subject": "1o JTP Virksomhedsøkonomi A - j26ososh", "deadline": "2026-11-23", "time": "08:15", "notes": "" },
            { "title": "For sent", "subject": "Dansk", "deadline": "2026-11-01", "time": "", "notes": "" },
            { "title": "Ugyldig dato", "subject": "Dansk", "deadline": "30.11.26", "time": "", "notes": "" },
            { "title": "", "subject": "Dansk", "deadline": "2026-11-30", "time": "", "notes": "" },
            { "title": "Kapitel 6, afsnit 6.3 om budgetter", "subject": "1o JTP Virksomhedsøkonomi A - j26ososh", "deadline": "2026-11-23", "time": "08:15", "notes": "" }
        ]});
        let items = items_from_ai(Site::Ludus, &raw, "2026-11-18");
        assert_eq!(items.len(), 2, "past, invalid, empty and duplicate rows are dropped");
        assert_eq!((items[0].subject.as_str(), items[0].date.as_deref(), items[0].time.as_deref()), ("Virksomhedsøkonomi", Some("2026-11-23"), Some("08:15")));
        assert_eq!(items[0].day.as_deref(), Some("Mandag"));
        assert_eq!(items[1].subject, "Læsevejledning");
        assert_eq!(items[1].notes.as_deref(), Some("Tag computer med."));
        assert!(items.iter().all(|i| i.kind == "deadline" && i.selected));
    }

    #[test]
    fn button_script_only_runs_on_school_sites() {
        let script = page_script(true, "/test-handoff");
        assert!(script.contains("nielsbrock.luduseg.dk") && script.contains("www.mitnielsbrock.dk"));
        assert!(script.contains("window.top !== window"));
        assert!(script.contains("https://kladde-extract.invalid/test-handoff?d="));
        assert!(script.contains("Hent afleveringer fra denne side"));
    }
    #[test]
    fn collection_uses_the_exact_https_origin() {
        for other in ["https://mitnielsbrock.dk:444/", "https://name@mitnielsbrock.dk/",
                      "https://mitnielsbrock.dk@elsewhere.example/", "about:blank"] {
            assert_eq!(site_of(&url(other)), None);
        }
        assert_eq!(site_of(&url("https://mitnielsbrock.dk:443/login")), Some(Site::MitNielsBrock));
    }

}
