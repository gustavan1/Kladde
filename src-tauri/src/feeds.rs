//! Calendar links (.ics). This is the only part of Kladde that uses the internet, and only for
//! links the user adds themselves.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::commands::{minutes_between, AppState};
use crate::importers::{ext_id, guess_subject, is_deadline_event, parse_ics};
use crate::store::{add_days, day_name, local_date, new_id, now_iso, Block, CalendarFeed, Data, Store, Task};

/// webcal:// becomes https://, and a missing scheme gets https://.
pub fn normalize_url(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let rest = if lower.starts_with("webcals://") {
        Some(&raw[10..])
    } else if lower.starts_with("webcal://") {
        Some(&raw[9..])
    } else {
        None
    };
    let url = match rest {
        Some(r) => format!("https://{r}"),
        None => raw.to_string(),
    };
    let l = url.to_lowercase();
    if l.starts_with("http://") || l.starts_with("https://") {
        url
    } else {
        format!("https://{url}")
    }
}

/// The feed as the frontend sees it: the full link (which may contain a private token) is
/// replaced by just its host name.
pub fn feed_out(feed: &CalendarFeed) -> Value {
    let preview = url::Url::parse(&feed.url)
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{h}/…")))
        .unwrap_or_else(|| feed.url.clone());
    let mut v = serde_json::to_value(feed).unwrap_or_else(|_| json!({}));
    if let Some(o) = v.as_object_mut() {
        o.remove("url");
        o.insert("url_preview".into(), json!(preview));
    }
    v
}

fn timed_out(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = cur {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            if matches!(io.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
                return true;
            }
        }
        cur = e.source();
    }
    false
}

/// Downloads a calendar. Error messages never contain the link itself, because calendar links
/// often include a private token.
pub(crate) fn fetch_calendar(url: &str, da: bool) -> Result<String, String> {
    use std::io::Read;
    let timeout_msg = || if da { "Kalenderen svarede ikke i tide." } else { "The calendar did not answer in time." }.to_string();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .redirects(20)
        .user_agent("Kladde calendar sync")
        .build();
    let resp = match agent.get(url).call() {
        Ok(resp) => resp,
        Err(ureq::Error::Status(code, _)) => {
            return Err(if da { format!("Serveren svarede {code}.") } else { format!("The server answered {code}.") })
        }
        Err(ureq::Error::Transport(t)) => {
            if timed_out(&t) {
                return Err(timeout_msg());
            }
            use ureq::ErrorKind::*;
            let reason = match (t.kind(), da) {
                (Dns, true) => "serveren blev ikke fundet",
                (Dns, false) => "the server was not found",
                (ConnectionFailed, true) => "ingen forbindelse til serveren",
                (ConnectionFailed, false) => "could not connect to the server",
                (TooManyRedirects, true) => "for mange omdirigeringer",
                (TooManyRedirects, false) => "too many redirects",
                (InvalidUrl | UnknownScheme, true) => "ugyldigt link",
                (InvalidUrl | UnknownScheme, false) => "invalid link",
                (_, true) => "netværksfejl",
                (_, false) => "network error",
            };
            return Err(if da {
                format!("Kalenderen kunne ikke hentes ({reason}).")
            } else {
                format!("The calendar could not be fetched ({reason}).")
            });
        }
    };
    let mut text = String::new();
    // Up to 50 MB; school calendars are far smaller.
    match resp.into_reader().take(50 * 1024 * 1024).read_to_string(&mut text) {
        Ok(_) => Ok(text),
        Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => Err(timeout_msg()),
        Err(_) => Err(if da {
            "Kalenderen kunne ikke læses (afbrudt eller ikke tekst).".into()
        } else {
            "The calendar could not be read (interrupted or not text).".into()
        }),
    }
}

/// Puts the result of a download into the data: new deadlines become tasks, and the feed's
/// lessons for the next 7 days replace its earlier lessons in the planner.
/// Returns { feed, new_tasks, new_blocks } or None when the feed no longer exists.
pub fn apply_sync(data: &mut Data, feed_id: &str, fetched: Result<String, String>, da: bool, now: &DateTime<Local>) -> Option<Value> {
    data.feeds.iter().position(|f| f.id == feed_id)?;
    let today = local_date(now);
    let mut new_tasks = 0;
    let mut new_blocks = 0;
    let outcome: Result<i64, String> = fetched.and_then(|text| {
        if !text.contains("BEGIN:VCALENDAR") {
            return Err(if da { "Linket gav ikke en kalender (.ics)." } else { "The link did not return a calendar (.ics)." }.into());
        }
        let events = parse_ics(&text, now.date_naive()).map_err(|reason| {
            if da { format!("Kalenderen kunne ikke læses: {reason}") } else { format!("Could not read calendar: {reason}") }
        })?;
        let known: Vec<String> = data.subjects.iter().map(|s| s.name.clone()).collect();
        let source = format!("feed:{feed_id}");
        // Deadlines: everything from yesterday and half a year ahead.
        let (from, to) = (add_days(&today, -1), add_days(&today, 180));
        for e in events.iter().filter(|e| is_deadline_event(e)) {
            let ext = format!("{source}:event:{}", ext_id(&[Some(&e.uid), e.recurrence_id.as_deref()]));
            let legacy_prefix = format!("{source}:{}:", e.uid);
            // A previous release stored UID + date, even for a single movable event.
            // A recurring series uses a distinct identity for each original occurrence.
            let matches = |task: &Task| {
                let Some(id) = &task.external_id else { return false };
                if id == &ext { return true }
                if let Some(date) = id.strip_prefix(&legacy_prefix) {
                    return crate::store::is_valid_date(date) && (e.recurrence_id.is_none() || date == e.date);
                }
                // Old releases truncated long feed identifiers at 200 characters.
                id.chars().count() == 200 && e.recurrence_id.is_none()
                    && format!("{legacy_prefix}{}", task.deadline).chars().take(200).eq(id.chars())
            };
            // If older syncs already made copies, keep the most advanced task and its edits.
            let survivor = data.tasks.iter().filter(|t| matches(t)).min_by_key(|t| match t.status.as_str() {
                "done" => 0, "doing" => 1, _ => 2,
            }).map(|t| t.id.clone());
            if let Some(id) = survivor {
                let mut removed = std::collections::HashSet::new();
                data.tasks.retain(|t| {
                    if t.id != id && matches(t) { removed.insert(t.id.clone()); false } else { true }
                });
                for block in data.blocks.iter_mut().filter(|b| b.source.as_deref() == Some("plan")) {
                    if block.external_id.as_ref().is_some_and(|old| removed.contains(old)) {
                        block.external_id = Some(id.clone());
                    }
                }
                let task = data.tasks.iter_mut().find(|t| t.id == id).expect("retained task");
                task.external_id = Some(ext);
                task.title = e.summary.clone();
                task.deadline = e.date.clone();
                continue;
            }
            if e.date < from || e.date > to { continue }
            let (subject, _) = data.ensure_subject(&guess_subject(&format!("{} {}", e.summary, e.description), &known));
            data.tasks.push(Task {
                id: new_id(),
                title: e.summary.clone(),
                subject: subject.name,
                subject_key: subject.key,
                deadline: e.date.clone(),
                status: "todo".into(),
                priority: if e.date <= add_days(&today, 3) { "high" } else { "medium" }.into(),
                estimated_minutes: 60,
                created_at: now_iso(),
                completed_at: None,
                external_id: Some(ext),
            });
            new_tasks += 1;
        }
        // Lessons: the planner shows one week, so it is rebuilt from the next 7 days.
        let key = |b: &Block| format!("{}|{}|{}", b.day, b.time, b.label);
        let old_keys: Vec<String> =
            data.blocks.iter().filter(|b| b.source.as_deref() == Some(source.as_str())).map(key).collect();
        let mut fresh: Vec<Block> = vec![];
        let week_end = add_days(&today, 6);
        for e in events.iter().filter(|e| !is_deadline_event(e) && e.date >= today && e.date <= week_end) {
            let Some(day) = day_name(&e.date) else { continue };
            let time = e.time.clone().unwrap_or_else(|| "08:00".into());
            let k = format!("{day}|{time}|{}", e.summary);
            if fresh.iter().any(|b| key(b) == k) {
                continue;
            }
            let (subject, _) = data.ensure_subject(&guess_subject(&e.summary, &known));
            fresh.push(Block {
                id: new_id(),
                day: day.into(),
                minutes: minutes_between(&time, e.end_time.as_deref().unwrap_or("")),
                time,
                label: e.summary.clone(),
                subject: subject.name,
                color: subject.color,
                source: Some(source.clone()),
                external_id: Some(format!("{source}:{}", e.uid).chars().take(200).collect()),
            });
            if !old_keys.contains(&k) {
                new_blocks += 1;
            }
        }
        let count = fresh.len() as i64;
        data.blocks.retain(|b| b.source.as_deref() != Some(source.as_str()));
        data.blocks.extend(fresh);
        Ok(count)
    });
    let prefix = format!("feed:{feed_id}:");
    let imported_tasks =
        data.tasks.iter().filter(|t| t.external_id.as_deref().is_some_and(|e| e.starts_with(&prefix))).count() as i64;
    let feed = data.feeds.iter_mut().find(|f| f.id == feed_id)?;
    match outcome {
        Ok(blocks) => {
            feed.last_status = "ok".into();
            feed.last_error = None;
            feed.imported_tasks = imported_tasks;
            feed.imported_blocks = blocks;
        }
        Err(message) => {
            feed.last_status = "error".into();
            feed.last_error = Some(message);
        }
    }
    feed.last_synced_at = Some(now_iso());
    Some(json!({ "feed": feed_out(feed), "new_tasks": new_tasks, "new_blocks": new_blocks }))
}

/// Downloads one feed and saves the result. The data is not locked while downloading.
pub fn sync_feed_blocking(store: &Mutex<Store>, feed_id: &str, da: bool) -> Result<Option<Value>, String> {
    let url = {
        let s = store.lock().unwrap_or_else(|p| p.into_inner());
        let Some(feed) = s.data.feeds.iter().find(|f| f.id == feed_id) else { return Ok(None) };
        feed.url.clone()
    };
    let fetched = fetch_calendar(&url, da);
    let mut s = store.lock().unwrap_or_else(|p| p.into_inner());
    let mut transaction = s.transaction();
    let result = apply_sync(&mut transaction.data, feed_id, fetched, da, &Local::now());
    if result.is_some() {
        transaction.commit()?;
    }
    Ok(result)
}

pub async fn sync_feed(app: AppHandle, feed_id: String, da: bool) -> Result<Option<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        sync_feed_blocking(&state.store, &feed_id, da)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Re-checks calendar links every 3 hours while Kladde is running.
pub fn start_auto_sync(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        loop {
            let due: Vec<String> = {
                let state = app.state::<AppState>();
                let store = state.store.lock().unwrap_or_else(|p| p.into_inner());
                store
                    .data
                    .feeds
                    .iter()
                    .filter(|f| {
                        let last = f
                            .last_synced_at
                            .as_deref()
                            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                            .map(|t| t.with_timezone(&Utc));
                        last.is_none_or(|t| Utc::now() - t >= chrono::Duration::minutes(3 * 60 - 1))
                    })
                    .map(|f| f.id.clone())
                    .collect()
            };
            for id in due {
                let state = app.state::<AppState>();
                if let Err(e) = sync_feed_blocking(&state.store, &id, true) {
                    crate::log(&format!("Calendar sync failed: {e}"));
                }
            }
            std::thread::sleep(Duration::from_secs(15 * 60));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn setup() -> (Data, DateTime<Local>) {
        let mut d = Data::default();
        d.feeds.push(CalendarFeed {
            id: "f1".into(),
            label: "Skole".into(),
            url: "https://example.com/private-token/cal.ics".into(),
            last_status: "pending".into(),
            ..CalendarFeed::default()
        });
        // An old lesson from this feed, which should be replaced.
        d.blocks.push(Block { id: "old".into(), day: "Mandag".into(), time: "08:00".into(), label: "Gammel lektion".into(), source: Some("feed:f1".into()), ..Block::default() });
        d.blocks.push(Block { id: "mine".into(), label: "Fodbold".into(), source: Some("manual".into()), ..Block::default() });
        (d, Local.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap())
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:essay@x\r\nSUMMARY:Aflevering: Engelsk essay\r\nDTSTART;VALUE=DATE:20261125\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:ma@x\r\nSUMMARY:2a MA modul\r\nDTSTART:20261119T081500\r\nDTEND:20261119T094500\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:later@x\r\nSUMMARY:Senere modul\r\nDTSTART:20261130T081500\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:old@x\r\nSUMMARY:Gammel aflevering\r\nDTSTART;VALUE=DATE:20260101\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn deadlines_become_tasks_and_this_weeks_lessons_replace_old_ones() {
        let (mut d, now) = setup();
        let r = apply_sync(&mut d, "f1", Ok(ICS.into()), true, &now).unwrap();
        assert_eq!((r["new_tasks"].as_i64(), r["new_blocks"].as_i64()), (Some(1), Some(1)));
        let t = &d.tasks[0];
        assert_eq!(t.title, "Aflevering: Engelsk essay");
        assert_eq!(t.subject, "Engelsk");
        assert_eq!(t.deadline, "2026-11-25");
        assert_eq!(t.priority, "medium");
        assert!(t.external_id.as_ref().unwrap().starts_with("feed:f1:event:"));
        let lessons: Vec<&Block> = d.blocks.iter().filter(|b| b.source.as_deref() == Some("feed:f1")).collect();
        assert_eq!(lessons.len(), 1);
        assert_eq!((lessons[0].day.as_str(), lessons[0].time.as_str(), lessons[0].minutes), ("Torsdag", "08:15", Some(90)));
        assert_eq!(lessons[0].subject, "Matematik");
        assert!(d.blocks.iter().any(|b| b.id == "mine"), "other blocks are kept");
        let feed = &r["feed"];
        assert_eq!(feed["last_status"], "ok");
        assert_eq!(feed["imported_tasks"], 1);
        assert_eq!(feed["imported_blocks"], 1);
        assert_eq!(feed["url_preview"], "example.com/…");
        assert!(feed.get("url").is_none(), "the private link is never sent to the window");
        assert!(feed["last_synced_at"].is_string());

        // Syncing again adds nothing new.
        let again = apply_sync(&mut d, "f1", Ok(ICS.into()), true, &now).unwrap();
        assert_eq!((again["new_tasks"].as_i64(), again["new_blocks"].as_i64()), (Some(0), Some(0)));
        assert_eq!(d.tasks.len(), 1);
    }

    #[test]
    fn rescheduled_legacy_deadline_keeps_progress_and_updates_in_place() {
        let (mut d, now) = setup();
        d.tasks.push(Task { id: "original".into(), status: "doing".into(), deadline: "2026-11-23".into(),
            external_id: Some("feed:f1:essay@x:2026-11-23".into()), ..Default::default() });
        d.tasks.push(Task { id: "duplicate".into(), deadline: "2026-11-24".into(),
            external_id: Some("feed:f1:essay@x:2026-11-24".into()), ..Default::default() });
        d.blocks.push(Block { source: Some("plan".into()), external_id: Some("duplicate".into()), ..Default::default() });
        for date in ["20261125", "20261127"] {
            let text = ICS.replace("20261125", date);
            let result = apply_sync(&mut d, "f1", Ok(text), false, &now).unwrap();
            assert_eq!(result["new_tasks"], 0);
        }
        assert_eq!(d.tasks.len(), 1);
        assert!(d.blocks.iter().any(|b| b.source.as_deref() == Some("plan") && b.external_id.as_deref() == Some("original")));
        let task = &d.tasks[0];
        assert_eq!((&*task.id, &*task.status, &*task.deadline, &*task.title),
                   ("original", "doing", "2026-11-27", "Aflevering: Engelsk essay"));
    }

    #[test]
    fn weekly_series_honors_exclusions_and_moved_occurrences() {
        let (mut d, now) = setup();
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:class\nSUMMARY:Biologi\nDTSTART:20260902T090000\nDTEND:20260902T100000\nRRULE:FREQ=WEEKLY;BYDAY=WE,TH,FR\nEXDATE:20261119T090000\nEND:VEVENT\nBEGIN:VEVENT\nUID:class\nRECURRENCE-ID:20261120T090000\nDTSTART:20261120T120000\nSUMMARY:Biologi ekstra\nEND:VEVENT\nEND:VCALENDAR";
        let r = apply_sync(&mut d, "f1", Ok(ics.into()), false, &now).unwrap();
        assert_eq!(r["feed"]["last_status"], "ok");
        let blocks: Vec<_> = d.blocks.iter().filter(|b| b.source.as_deref() == Some("feed:f1")).collect();
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().any(|b| b.day == "Onsdag" && b.minutes == Some(60)));
        assert!(blocks.iter().any(|b| b.day == "Fredag" && b.time == "12:00"));
    }

    #[test]
    fn errors_are_stored_on_the_feed() {
        let (mut d, now) = setup();
        let r = apply_sync(&mut d, "f1", Ok("<html>Login</html>".into()), true, &now).unwrap();
        assert_eq!(r["feed"]["last_status"], "error");
        assert_eq!(r["feed"]["last_error"], "Linket gav ikke en kalender (.ics).");
        let r = apply_sync(&mut d, "f1", Ok("nope".into()), false, &now).unwrap();
        assert_eq!(r["feed"]["last_error"], "The link did not return a calendar (.ics).");
        let r = apply_sync(&mut d, "f1", Err("Serveren svarede 404.".into()), true, &now).unwrap();
        assert_eq!(r["feed"]["last_error"], "Serveren svarede 404.");
        assert!(d.blocks.iter().any(|b| b.id == "old"), "a failed sync keeps the old lessons");
        assert!(apply_sync(&mut d, "missing", Ok(ICS.into()), true, &now).is_none());
    }

    #[test]
    fn links_are_normalised() {
        assert_eq!(normalize_url("webcal://lectio.dk/cal.ics"), "https://lectio.dk/cal.ics");
        assert_eq!(normalize_url("WEBCALS://x.dk/a"), "https://x.dk/a");
        assert_eq!(normalize_url("x.dk/a.ics"), "https://x.dk/a.ics");
        assert_eq!(normalize_url("http://x.dk/a.ics"), "http://x.dk/a.ics");
    }
}
