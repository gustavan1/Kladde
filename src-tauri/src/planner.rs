//! Study plan and weekly summary. The plan is calculated (no AI): it spreads the work for
//! upcoming deadlines over the coming weekdays in focus blocks of up to 45 minutes.

use chrono::{DateTime, Datelike, Local};
use serde::Serialize;

use crate::store::{add_days, day_name, local_date, local_midnight_iso, new_id, parse_date, Block, Data, Task};

const SLOTS: [&str; 4] = ["15:30", "16:30", "17:30", "19:00"];
const MAX_BLOCK: i64 = 45;
/// The planner shows one week (by weekday name), so it plans one week ahead.
const WINDOW_DAYS: i64 = 7;

#[derive(Serialize, Debug, Clone)]
pub struct StudyPlan {
    pub blocks: Vec<Block>,
    pub summary: String,
}

fn priority_rank(p: &str) -> u8 {
    match p {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    }
}

struct PlanDay {
    date: String,
    slots: Vec<&'static str>,
    minutes: i64,
    busy: Vec<(i64, i64)>,
}

fn minute_of_day(time: &str) -> Option<i64> {
    let (h, m) = time.split_once(':')?;
    Some(h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?)
}

impl PlanDay {
    fn free_slot(&self, duration: i64) -> Option<usize> {
        self.slots.iter().position(|time| {
            let start = minute_of_day(time).expect("fixed planner slot");
            self.busy.iter().all(|&(a, b)| start + duration <= a || start >= b)
        })
    }
}

/// Returns None when there are no open deadlines in the next 14 days.
pub fn make_study_plan(data: &Data, da: bool, now: &DateTime<Local>) -> Option<StudyPlan> {
    let today = local_date(now);
    let horizon = add_days(&today, 14);
    let mut open: Vec<&Task> = data.tasks.iter().filter(|t| t.status != "done" && t.deadline <= horizon).collect();
    open.sort_by(|a, b| {
        a.deadline.cmp(&b.deadline).then(priority_rank(&a.priority).cmp(&priority_rank(&b.priority)))
    });
    if open.is_empty() {
        return None;
    }

    // Days in the window, weekdays only (the planner shows Monday to Friday).
    let now_time = now.format("%H:%M").to_string();
    let mut days: Vec<PlanDay> = vec![];
    for i in 0..WINDOW_DAYS {
        let date = add_days(&today, i);
        let dow = parse_date(&date).map(|d| d.weekday().num_days_from_sunday()).unwrap_or(0);
        if dow == 0 || dow == 6 {
            continue;
        }
        let slots: Vec<&str> = SLOTS.iter().copied().filter(|s| i > 0 || *s > now_time.as_str()).collect();
        if !slots.is_empty() {
            let weekday = day_name(&date).expect("generated date");
            let busy = data.blocks.iter()
                .filter(|block| block.day == weekday && block.source.as_deref() != Some("plan"))
                .filter_map(|block| {
                    let start = minute_of_day(&block.time)?;
                    Some((start, start.saturating_add(block.minutes.unwrap_or(MAX_BLOCK).max(1))))
                }).collect();
            days.push(PlanDay { date, slots, minutes: 0, busy });
        }
    }

    let mut blocks: Vec<Block> = vec![];
    let mut unplanned = 0;
    let mut planned: Vec<&str> = vec![];
    for task in open {
        // Work happens before the deadline day, or on it if that is today. Overdue work is
        // already late, so it is caught up on any free day in the coming week.
        let last_day = if task.deadline < today {
            add_days(&today, WINDOW_DAYS)
        } else if task.deadline == today {
            today.clone()
        } else {
            add_days(&task.deadline, -1)
        };
        let usable: Vec<usize> = (0..days.len()).filter(|&i| days[i].date <= last_day).collect();
        // Far-off deadlines: plan half the work this week, the rest when the plan is remade.
        let beyond_window = task.deadline > add_days(&today, WINDOW_DAYS);
        let estimate = if beyond_window { (task.estimated_minutes + 1) / 2 } else { task.estimated_minutes };
        let mut remaining = estimate.max(15);
        while remaining > 0 {
            let minutes = MAX_BLOCK.min(remaining);
            let best = usable
                .iter()
                .copied()
                .filter_map(|i| days[i].free_slot(minutes).map(|slot| (i, slot)))
                .min_by(|&(a, _), &(b, _)| days[a].minutes.cmp(&days[b].minutes).then(days[a].date.cmp(&days[b].date)));
            let Some((i, slot)) = best else {
                unplanned += remaining;
                break;
            };
            let day = &mut days[i];
            let time = day.slots.remove(slot);
            day.minutes += minutes;
            remaining -= minutes;
            if !planned.contains(&task.id.as_str()) {
                planned.push(&task.id);
            }
            blocks.push(Block {
                id: new_id(),
                day: day_name(&day.date).unwrap_or("Mandag").to_string(),
                time: time.to_string(),
                label: task.title.clone(),
                subject: task.subject.clone(),
                color: "sky".into(),
                source: Some("plan".into()),
                minutes: Some(minutes),
                external_id: Some(task.id.clone()),
            });
        }
    }

    let total: i64 = blocks.iter().map(|b| b.minutes.unwrap_or(0)).sum();
    let (h, m) = (total / 60, total % 60);
    let hours = if h > 0 { format!("{h} {} ", if da { "t" } else { "h" }) } else { String::new() };
    let mins = if m > 0 || h == 0 { format!("{m} min") } else { String::new() };
    let time = format!("{hours}{mins}").trim().to_string();
    let mut summary = if da {
        format!("{} fokusblokke · {time} til {} opgaver", blocks.len(), planned.len())
    } else {
        format!("{} focus blocks · {time} for {} tasks", blocks.len(), planned.len())
    };
    if unplanned > 0 {
        summary += &if da {
            format!(" ({unplanned} min kunne ikke nås før fristerne)")
        } else {
            format!(" ({unplanned} min did not fit before the deadlines)")
        };
    }
    Some(StudyPlan { blocks, summary })
}

#[derive(Serialize, Debug, Clone)]
pub struct WeeklySummary {
    pub week_start: String,
    pub is_sunday: bool,
    pub completed_tasks: usize,
    pub study_minutes: i64,
    pub notes_touched: usize,
    pub planned_focus_minutes: i64,
    pub overdue_tasks: usize,
    pub upcoming: Vec<Task>,
}

pub fn weekly_summary(data: &Data, now: &DateTime<Local>) -> WeeklySummary {
    let today = local_date(now);
    let dow = now.weekday().num_days_from_sunday() as i64;
    let week_start = add_days(&today, if dow == 0 { -6 } else { 1 - dow });
    let week_start_iso = local_midnight_iso(&week_start);
    let completed: Vec<&Task> = data
        .tasks
        .iter()
        .filter(|t| t.status == "done" && t.completed_at.as_deref().is_some_and(|c| !c.is_empty() && c >= week_start_iso.as_str()))
        .collect();
    let open: Vec<&Task> = data.tasks.iter().filter(|t| t.status != "done").collect();
    let in_a_week = add_days(&today, 7);
    let mut upcoming: Vec<Task> =
        open.iter().filter(|t| t.deadline >= today && t.deadline <= in_a_week).map(|t| (*t).clone()).collect();
    upcoming.sort_by(|a, b| a.deadline.cmp(&b.deadline));
    WeeklySummary {
        week_start,
        is_sunday: dow == 0,
        completed_tasks: completed.len(),
        study_minutes: completed.iter().map(|t| t.estimated_minutes).sum(),
        notes_touched: data.notes.iter().filter(|n| n.updated_at >= week_start_iso).count(),
        planned_focus_minutes: data
            .blocks
            .iter()
            .filter(|b| b.source.as_deref() == Some("plan"))
            .map(|b| b.minutes.unwrap_or(0))
            .sum(),
        overdue_tasks: open.iter().filter(|t| t.deadline < today).count(),
        upcoming,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{iso, Note};
    use chrono::{TimeZone, Utc};

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }
    fn task(title: &str, deadline: &str, minutes: i64, priority: &str, status: &str) -> Task {
        Task {
            id: format!("id-{title}"),
            title: title.into(),
            deadline: deadline.into(),
            estimated_minutes: minutes,
            priority: priority.into(),
            status: status.into(),
            ..Task::default()
        }
    }
    fn data(tasks: Vec<Task>) -> Data {
        Data { tasks, ..Data::default() }
    }
    fn summary(plan: &StudyPlan) -> Vec<(String, String, String, i64)> {
        plan.blocks.iter().map(|b| (b.label.clone(), b.day.clone(), b.time.clone(), b.minutes.unwrap())).collect()
    }
    fn row(l: &str, d: &str, t: &str, m: i64) -> (String, String, String, i64) {
        (l.into(), d.into(), t.into(), m)
    }

    // 2026-11-18 is a Wednesday.
    #[test]
    fn spreads_work_before_deadlines_in_45_minute_blocks() {
        let d = data(vec![task("Analyse", "2026-11-20", 90, "medium", "todo"), task("Lektie", "2026-11-19", 45, "high", "todo")]);
        let plan = make_study_plan(&d, true, &at(2026, 11, 18, 14, 0)).unwrap();
        assert_eq!(
            summary(&plan),
            [row("Lektie", "Onsdag", "15:30", 45), row("Analyse", "Torsdag", "15:30", 45), row("Analyse", "Onsdag", "16:30", 45)]
        );
        assert!(plan.blocks.iter().all(|b| b.source.as_deref() == Some("plan") && b.color == "sky"));
        assert_eq!(plan.blocks[0].external_id.as_deref(), Some("id-Lektie"));
        assert_eq!(plan.summary, "3 fokusblokke · 2 t 15 min til 2 opgaver");
        let en = make_study_plan(&d, false, &at(2026, 11, 18, 14, 0)).unwrap();
        assert_eq!(en.summary, "3 focus blocks · 2 h 15 min for 2 tasks");
    }

    #[test]
    fn weekends_are_skipped_and_far_deadlines_get_half_the_time() {
        // Saturday: the week ahead is Monday 23 to Friday 27; the deadline is after that.
        let d = data(vec![task("Projekt", "2026-11-30", 120, "medium", "todo")]);
        let plan = make_study_plan(&d, true, &at(2026, 11, 21, 10, 0)).unwrap();
        assert_eq!(summary(&plan), [row("Projekt", "Mandag", "15:30", 45), row("Projekt", "Tirsdag", "15:30", 15)]);
    }

    #[test]
    fn overdue_work_is_caught_up_on_the_next_free_day() {
        // 19:30 on Wednesday: no slots left today, so the overdue task goes to Thursday.
        let d = data(vec![task("Gammel", "2026-11-16", 10, "high", "todo")]);
        let plan = make_study_plan(&d, true, &at(2026, 11, 18, 19, 30)).unwrap();
        assert_eq!(summary(&plan), [row("Gammel", "Torsdag", "15:30", 15)]);
        let earlier = make_study_plan(&d, true, &at(2026, 11, 18, 14, 0)).unwrap();
        assert_eq!(summary(&earlier), [row("Gammel", "Onsdag", "15:30", 15)]);
    }

    #[test]
    fn weekend_deadline_without_remaining_slots_is_reported() {
        let d = data(vec![task("Essay", "2026-11-22", 45, "high", "todo")]);
        let plan = make_study_plan(&d, false, &at(2026, 11, 20, 19, 30)).unwrap();
        assert!(plan.blocks.is_empty());
        assert!(plan.summary.contains("45 min did not fit"));
    }

    #[test]
    fn avoids_partial_overlaps_and_reuses_previous_plan_slots() {
        let mut d = data(vec![task("Essay", "2026-11-19", 45, "high", "todo")]);
        for (source, time) in [("manual", "16:00"), ("feed:f", "17:15"), ("plan", "19:00")] {
            d.blocks.push(Block { day: "Onsdag".into(), time: time.into(), minutes: Some(60), source: Some(source.into()), ..Default::default() });
        }
        let plan = make_study_plan(&d, false, &at(2026, 11, 18, 14, 0)).unwrap();
        assert_eq!(plan.blocks[0].time, "19:00");
    }

    #[test]
    fn work_that_does_not_fit_is_reported() {
        let d = data(vec![task("Stor", "2026-11-19", 200, "high", "todo")]);
        let plan = make_study_plan(&d, true, &at(2026, 11, 18, 18, 0)).unwrap();
        assert_eq!(summary(&plan), [row("Stor", "Onsdag", "19:00", 45)]);
        assert_eq!(plan.summary, "1 fokusblokke · 45 min til 1 opgaver (155 min kunne ikke nås før fristerne)");
        let en = make_study_plan(&d, false, &at(2026, 11, 18, 18, 0)).unwrap();
        assert!(en.summary.ends_with("(155 min did not fit before the deadlines)"));
    }

    #[test]
    fn higher_priority_first_on_the_same_deadline() {
        let d = data(vec![task("Lav", "2026-11-20", 45, "low", "todo"), task("Høj", "2026-11-20", 45, "high", "todo")]);
        let plan = make_study_plan(&d, true, &at(2026, 11, 18, 14, 0)).unwrap();
        assert_eq!(plan.blocks[0].label, "Høj");
    }

    #[test]
    fn nothing_to_plan() {
        let d = data(vec![task("Senere", "2026-12-10", 60, "high", "todo"), task("Færdig", "2026-11-19", 60, "high", "done")]);
        assert!(make_study_plan(&d, true, &at(2026, 11, 18, 14, 0)).is_none());
        assert!(make_study_plan(&data(vec![]), true, &at(2026, 11, 18, 14, 0)).is_none());
    }

    fn local_iso(y: i32, m: u32, d: u32, h: u32) -> String {
        iso(Local.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap().with_timezone(&Utc))
    }

    #[test]
    fn weekly_summary_counts_this_week() {
        let mut done_now = task("Færdig", "2026-11-17", 50, "medium", "done");
        done_now.completed_at = Some(local_iso(2026, 11, 16, 9));
        let mut done_before = task("Sidste uge", "2026-11-10", 70, "medium", "done");
        done_before.completed_at = Some(local_iso(2026, 11, 15, 22));
        let mut d = data(vec![
            done_now,
            done_before,
            task("Overskredet", "2026-11-17", 30, "high", "todo"),
            task("Senere", "2026-11-25", 30, "medium", "todo"),
            task("I dag", "2026-11-18", 30, "medium", "doing"),
            task("For langt væk", "2026-11-26", 30, "medium", "todo"),
        ]);
        d.notes = vec![
            Note { title: "Ny".into(), updated_at: local_iso(2026, 11, 17, 8), ..Note::default() },
            Note { title: "Gammel".into(), updated_at: local_iso(2026, 11, 14, 8), ..Note::default() },
        ];
        d.blocks = vec![
            Block { source: Some("plan".into()), minutes: Some(45), ..Block::default() },
            Block { source: Some("plan".into()), minutes: Some(30), ..Block::default() },
            Block { source: Some("import".into()), minutes: Some(90), ..Block::default() },
        ];
        let w = weekly_summary(&d, &at(2026, 11, 18, 14, 0));
        assert_eq!(w.week_start, "2026-11-16");
        assert!(!w.is_sunday);
        assert_eq!((w.completed_tasks, w.study_minutes), (1, 50));
        assert_eq!(w.notes_touched, 1);
        assert_eq!(w.planned_focus_minutes, 75);
        assert_eq!(w.overdue_tasks, 1);
        let upcoming: Vec<&str> = w.upcoming.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(upcoming, ["I dag", "Senere"]);
    }

    #[test]
    fn sunday_belongs_to_the_week_that_started_monday() {
        let w = weekly_summary(&data(vec![]), &at(2026, 11, 22, 20, 0));
        assert!(w.is_sunday);
        assert_eq!(w.week_start, "2026-11-16");
        let monday = weekly_summary(&data(vec![]), &at(2026, 11, 23, 0, 5));
        assert_eq!((monday.week_start.as_str(), monday.is_sunday), ("2026-11-23", false));
    }
}
