//! Calendar links (.ics). This is the only part of Kladde that uses the internet, and only for
//! links the user adds themselves.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::{LazyLock, Mutex, mpsc, atomic::{AtomicUsize, Ordering}};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::commands::{minutes_between, AppState};
use crate::importers::{ext_id, guess_subject, is_deadline_event, parse_ics, IcsEvent, MAX_CALENDAR_BYTES};
use crate::store::{add_days, day_name, local_date, new_id, now_iso, Block, CalendarFeed, Data, Task};

/// Calendar links are credentials. Keep them out of diagnostics and require encrypted transport.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 4096 { return Err("Invalid calendar URL".into()); }
    let candidate = match raw.split_once("://") {
        Some((scheme, rest)) if ["webcal", "webcals"].iter().any(|s| scheme.eq_ignore_ascii_case(s)) => format!("https://{rest}"),
        Some(_) => raw.to_string(),
        None => format!("https://{raw}"),
    };
    let mut url = url::Url::parse(&candidate).map_err(|_| "Invalid calendar URL")?;
    if url.scheme() != "https" || url.host().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("Use an HTTPS calendar subscription without embedded credentials.".into());
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

fn public_address(ip: IpAddr) -> bool {
    // Allow global unicast only. Exclude IANA special-purpose and transition ranges, including 6to4.
    static SPECIAL: LazyLock<Vec<ipnet::IpNet>> = LazyLock::new(|| [
        "0.0.0.0/8", "10.0.0.0/8", "100.64.0.0/10", "127.0.0.0/8", "169.254.0.0/16",
        "172.16.0.0/12", "192.0.0.0/24", "192.0.2.0/24", "192.88.99.0/24", "192.168.0.0/16",
        "198.18.0.0/15", "198.51.100.0/24", "203.0.113.0/24", "224.0.0.0/4", "240.0.0.0/4",
        "2001::/23", "2001:db8::/32", "2002::/16", "3fff::/20",
    ].iter().map(|s| s.parse().expect("constant network")).collect());
    let ip = ip.to_canonical();
    if let IpAddr::V6(v6) = ip {
        if v6.segments()[0] >> 13 != 1 { return false; } // 2000::/3
    }
    !SPECIAL.iter().any(|net| net.contains(&ip))
}

// Resolve once per connection, and pass the vetted addresses directly to the socket connector.
// The OS DNS call can outlive a timeout. Bound those workers across all concurrent feeds.
fn resolve_public(name: &str, deadline: Instant) -> std::io::Result<Vec<SocketAddr>> {
    use std::io::{Error, ErrorKind};
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    if ACTIVE.fetch_add(1, Ordering::Relaxed) >= 8 {
        ACTIVE.fetch_sub(1, Ordering::Relaxed);
        return Err(Error::other("Calendar resolver busy"));
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let name = name.to_owned();
    if let Err(e) = std::thread::Builder::new().spawn(move || {
        let addrs = name.to_socket_addrs().map(|a| a.collect::<Vec<_>>());
        ACTIVE.fetch_sub(1, Ordering::Relaxed);
        let _ = tx.send(addrs);
    }) {
        ACTIVE.fetch_sub(1, Ordering::Relaxed);
        return Err(e);
    }
    let addrs = rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| Error::new(ErrorKind::TimedOut, "Calendar DNS timeout"))??;
    if addrs.is_empty() || !addrs.iter().all(|a| public_address(a.ip())) {
        return Err(Error::new(ErrorKind::PermissionDenied, "Restricted calendar destination"));
    }
    Ok(addrs)
}

/// The feed as the frontend sees it: the full link (which may contain a private token) is
/// replaced by just its host name.
pub fn feed_out(feed: &CalendarFeed) -> Value {
    let preview = url::Url::parse(&feed.url)
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{h}/…")))
        .unwrap_or_default();
    let mut v = serde_json::to_value(feed).unwrap_or_else(|_| json!({}));
    if let Some(o) = v.as_object_mut() {
        o.remove("url");
        o.insert("needs_url".into(), json!(feed.url.is_empty()));
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
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut current = normalize_url(url)?;
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .try_proxy_from_env(false)
        .resolver(move |name: &str| resolve_public(name, deadline))
        .timeout(Duration::from_secs(20))
        .redirects(0)
        .user_agent("Kladde calendar sync")
        .build();
    let mut hops = 0;
    let resp = loop {
        let timeout = deadline.checked_duration_since(Instant::now()).ok_or_else(timeout_msg)?;
        match agent.get(&current).timeout(timeout).call() {
        Ok(resp) if (300..400).contains(&resp.status()) => {
            hops += 1;
            if hops > 5 { return Err("Too many calendar redirects".into()); }
            let next = resp.header("Location").and_then(|location| url::Url::parse(&current).ok()?.join(location).ok())
                .ok_or("Invalid calendar redirect")?;
            current = normalize_url(next.as_str())?;
        }
        Ok(resp) => break resp,
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
        }
    };
    let mut text = String::new();
    match resp.into_reader().take(MAX_CALENDAR_BYTES as u64 + 1).read_to_string(&mut text) {
        Ok(n) if n > MAX_CALENDAR_BYTES => Err("Calendar exceeds the size limit".into()),
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
pub(crate) fn calendar_events(fetched: Result<String, String>, da: bool, now: &DateTime<Local>) -> Result<Vec<IcsEvent>, String> {
    let text = fetched?;
    if !text.trim_start_matches('\u{feff}').trim_start().get(..15).is_some_and(|s| s.eq_ignore_ascii_case("BEGIN:VCALENDAR")) {
        return Err(if da { "Linket gav ikke en kalender (.ics)." } else { "The link did not return a calendar (.ics)." }.into());
    }
    parse_ics(&text, now.date_naive()).map_err(|why| {
        if da { format!("Kalenderen kunne ikke læses: {why}") } else { format!("Could not read calendar: {why}") }
    })
}

pub fn apply_events(data: &mut Data, feed_id: &str, events: Result<Vec<IcsEvent>, String>, now: &DateTime<Local>) -> Option<Value> {
    data.feeds.iter().position(|f| f.id == feed_id)?;
    let today = local_date(now);
    let mut new_tasks = 0;
    let mut new_blocks = 0;
    let outcome = events.map(|events| {
        let known: Vec<String> = data.subjects.iter().map(|s| s.name.clone()).collect();
        let source = format!("feed:{feed_id}");
        // Deadlines: everything from yesterday and half a year ahead.
        let (from, to) = (add_days(&today, -1), add_days(&today, 180));
        let prefix = format!("{source}:");
        let mut current = HashMap::<String, Vec<usize>>::new();
        let mut legacy = HashMap::<String, Vec<usize>>::new();
        let mut truncated = HashMap::<String, Vec<usize>>::new();
        for (index, task) in data.tasks.iter().enumerate() {
            let Some(id) = task.external_id.as_ref().filter(|id| id.starts_with(&prefix)) else { continue };
            current.entry(id.clone()).or_default().push(index);
            if let Some((uid, _)) = id.rsplit_once(':').filter(|(_, date)| crate::store::is_valid_date(date)) {
                legacy.entry(uid.to_owned()).or_default().push(index);
            }
            if id.chars().count() == 200 {
                truncated.entry(id.chars().take(190).collect()).or_default().push(index);
            }
        }
        // Keep indices stable until every event has been matched. Old versions keyed tasks
        // by UID and date, sometimes truncating the ID; migrate the most advanced copy.
        let mut removed = HashMap::new();
        for e in events.iter().filter(|e| is_deadline_event(e)) {
            let ext = format!("{source}:event:{}", ext_id(&[Some(&e.uid), e.recurrence_id.as_deref()]));
            let uid = format!("{prefix}{}", e.uid);
            let mut matches = current.get(&ext).cloned().unwrap_or_default();
            if let Some(candidates) = legacy.get(&uid) {
                matches.extend(candidates.iter().copied().filter(|&i| {
                    let id = data.tasks[i].external_id.as_deref().unwrap_or_default();
                    id.rsplit_once(':').is_some_and(|(key, date)| key == uid && (e.recurrence_id.is_none() || date == e.date))
                }));
            }
            if e.recurrence_id.is_none() && !truncated.is_empty() && uid.chars().count() >= 189 {
                let legacy_prefix = format!("{uid}:");
                if let Some(candidates) = truncated.get(&legacy_prefix.chars().take(190).collect::<String>()) {
                    matches.extend(candidates.iter().copied().filter(|&i| {
                        let task = &data.tasks[i];
                        task.external_id.as_deref().is_some_and(|id| format!("{legacy_prefix}{}", task.deadline).chars().take(200).eq(id.chars()))
                    }));
                }
            }
            matches.retain(|&i| !removed.contains_key(&data.tasks[i].id));
            if let Some(&best) = matches.iter().min_by_key(|&&i| match data.tasks[i].status.as_str() {
                "done" => 0, "doing" => 1, _ => 2,
            }) {
                for i in matches.into_iter().filter(|&i| i != best) {
                    removed.insert(data.tasks[i].id.clone(), data.tasks[best].id.clone());
                }
                let task = &mut data.tasks[best];
                current.insert(ext.clone(), vec![best]);
                task.external_id = Some(ext);
                task.title.clone_from(&e.summary);
                task.deadline.clone_from(&e.date);
                task.deadline_time.clone_from(&e.time);
                continue;
            }
            if e.date < from || e.date > to { continue }
            let (subject, _) = data.ensure_subject(&guess_subject(&format!("{} {}", e.summary, e.description), &known));
            current.insert(ext.clone(), vec![data.tasks.len()]);
            data.tasks.push(Task {
                id: new_id(),
                title: e.summary.clone(),
                subject: subject.name,
                subject_key: subject.key,
                deadline: e.date.clone(),
                deadline_time: e.time.clone(),
                status: "todo".into(),
                priority: if e.date <= add_days(&today, 3) { "high" } else { "medium" }.into(),
                estimated_minutes: 60,
                created_at: now_iso(),
                completed_at: None,
                external_id: Some(ext),
                ..Default::default()
            });
            new_tasks += 1;
        }
        if !removed.is_empty() {
            data.tasks.retain(|t| !removed.contains_key(&t.id));
            for block in data.blocks.iter_mut().filter(|b| b.source.as_deref() == Some("plan")) {
                if let Some(replacement) = block.external_id.as_ref().and_then(|id| removed.get(id)) {
                    block.external_id = Some(replacement.clone());
                }
            }
        }
        // Lessons: the planner shows one week, so it is rebuilt from the next 7 days.
        let key = |b: &Block| (b.day.clone(), b.time.clone(), b.label.clone());
        let old_keys: HashSet<_> =
            data.blocks.iter().filter(|b| b.source.as_deref() == Some(source.as_str())).map(key).collect();
        let mut fresh_keys = HashSet::new();
        let mut fresh: Vec<Block> = vec![];
        let week_end = add_days(&today, 6);
        for e in events.iter().filter(|e| !is_deadline_event(e) && e.date >= today && e.date <= week_end) {
            let Some(day) = day_name(&e.date) else { continue };
            let time = e.time.clone().unwrap_or_else(|| "08:00".into());
            let k = (day.to_string(), time.clone(), e.summary.clone());
            if !fresh_keys.insert(k.clone()) {
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
                ..Default::default()
            });
            if !old_keys.contains(&k) {
                new_blocks += 1;
            }
        }
        let count = fresh.len() as i64;
        data.blocks.retain(|b| b.source.as_deref() != Some(source.as_str()));
        data.blocks.extend(fresh);
        count
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
pub fn sync_feed_blocking(state: &AppState, feed_id: &str, da: bool) -> Result<Option<Value>, String> {
    let mut refreshing = state.refreshing.lock().unwrap_or_else(|p| p.into_inner());
    if !refreshing.insert(feed_id.to_string()) {
        return Err(if da { "Kalenderen opdateres allerede." } else { "Calendar is already refreshing." }.into());
    }
    drop(refreshing);
    struct Refresh<'a>(&'a Mutex<std::collections::HashSet<String>>, &'a str);
    impl Drop for Refresh<'_> {
        fn drop(&mut self) { self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(self.1); }
    }
    let _active = Refresh(&state.refreshing, feed_id);
    let store = &state.store;
    let url = {
        let s = store.lock().unwrap_or_else(|p| p.into_inner());
        let Some(feed) = s.data.feeds.iter().find(|f| f.id == feed_id) else { return Ok(None) };
        feed.url.clone()
    };
    if url.is_empty() { return Err("Reconnect this subscription first.".into()); }
    let now = Local::now();
    let events = calendar_events(fetch_calendar(&url, da), da, &now);
    let mut s = store.lock().unwrap_or_else(|p| p.into_inner());
    let mut transaction = s.transaction();
    let result = apply_events(&mut transaction.data, feed_id, events, &now);
    if result.is_some() {
        transaction.commit()?;
    }
    Ok(result)
}

pub async fn sync_feed(app: AppHandle, feed_id: String, da: bool) -> Result<Option<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        sync_feed_blocking(&state, &feed_id, da)
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
                    .filter(|f| !f.url.is_empty())
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
                if let Err(e) = sync_feed_blocking(&state, &id, true) {
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

    fn apply_sync(data: &mut Data, feed: &str, fetched: Result<String,String>, da: bool, now: &DateTime<Local>) -> Option<Value> {
        apply_events(data, feed, calendar_events(fetched, da, now), now)
    }

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
        assert_eq!(normalize_url("webcal://lectio.dk/cal.ics").unwrap(), "https://lectio.dk/cal.ics");
        assert_eq!(normalize_url("WEBCALS://x.dk/a").unwrap(), "https://x.dk/a");
        assert_eq!(normalize_url("x.dk/a.ics").unwrap(), "https://x.dk/a.ics");
        assert!(normalize_url("http://x.dk/a.ics").is_err());
    }
    #[test]
    fn invalid_or_truncated_calendars_retain_cached_lessons() {
        let (mut d, now) = setup();
        apply_sync(&mut d, "f1", Ok(ICS.into()), false, &now).unwrap();
        let saved = d.blocks.clone();
        for invalid in ["BEGIN:VCALENDAR\nBEGIN:VEVENT\n", "BEGIN:VCALENDAR\nEND:VEVENT\nEND:VCALENDAR"] {
            apply_sync(&mut d, "f1", Ok(invalid.into()), false, &now).unwrap();
            assert_eq!(d.blocks, saved);
            assert_eq!(d.feeds[0].last_status, "error");
        }
        apply_sync(&mut d, "f1", Ok("begin:vcalendar\nend:vcalendar\n".into()), false, &now).unwrap();
        assert_eq!(d.feeds[0].last_status, "ok");
        assert!(d.blocks.iter().all(|b| b.source.as_deref() != Some("feed:f1")));
    }

    #[test]
    fn resolver_accepts_only_public_network_destinations() {
        for ip in ["127.0.0.1", "192.168.4.5", "100.64.1.3", "169.254.1.3", "::1", "fe80::1", "::ffff:10.0.0.1", "2002::1"] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "2606:4700:4700::1111"] { assert!(public_address(ip.parse().unwrap())); }
        for url in ["file:///calendar", "http://example.org/cal", "https://person:secret@example.org/calendar"] {
            assert!(normalize_url(url).is_err());
        }
    }

}
