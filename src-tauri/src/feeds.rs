//! Calendar links (.ics). This is the only part of Kladde that uses the internet, and only for
//! links the user adds themselves.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::commands::{minutes_between, AppState};
use crate::importers::{guess_subject, is_deadline_event, parse_ics};
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
fn fetch_calendar(url: &str, da: bool) -> Result<String, String> {
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
        let events = parse_ics(&text);
        let known: Vec<String> = data.subjects.iter().map(|s| s.name.clone()).collect();
        let source = format!("feed:{feed_id}");
        // Deadlines: everything from yesterday and half a year ahead.
        let (from, to) = (add_days(&today, -1), add_days(&today, 180));
        for e in events.iter().filter(|e| is_deadline_event(e) && e.date >= from && e.date <= to) {
            let ext: String = format!("{source}:{}:{}", e.uid, e.date).chars().take(200).collect();
            if data.tasks.iter().any(|t| t.external_id.as_deref() == Some(ext.as_str())) {
                continue;
            }
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
pub fn sync_feed_blocking(store: &Mutex<Store>, feed_id: &str, da: bool) -> Option<Value> {
    let url = {
        let s = store.lock().unwrap_or_else(|p| p.into_inner());
        s.data.feeds.iter().find(|f| f.id == feed_id)?.url.clone()
    };
    let fetched = fetch_calendar(&url, da);
    let mut s = store.lock().unwrap_or_else(|p| p.into_inner());
    let result = apply_sync(&mut s.data, feed_id, fetched, da, &Local::now());
    if let Err(e) = s.save() {
        crate::log(&format!("Saving after calendar sync failed: {e}"));
    }
    result
}

pub async fn sync_feed(app: AppHandle, feed_id: String, da: bool) -> Option<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        sync_feed_blocking(&state.store, &feed_id, da)
    })
    .await
    .ok()
    .flatten()
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
                sync_feed_blocking(&state.store, &id, true);
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
        assert_eq!(t.external_id.as_deref(), Some("feed:f1:essay@x:2026-11-25"));
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
