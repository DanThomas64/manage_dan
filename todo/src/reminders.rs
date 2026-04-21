//! Reminder support — Vikunja task reminders and config-based reminders.
//!
//! Config reminders are loaded from `$APP_CONFIG_DIR/reminders.toml` using the
//! same schedule syntax and struct as `recurring.toml`.  They appear only in
//! summary output; no individual ticket is printed for them.
//!
//! Two outputs are produced:
//!
//! - **Daily summary section** — "REMINDERS TODAY": all config reminders due
//!   today + all Vikunja tasks whose `reminder_dates` include today.
//!
//! - **Weekly overview ticket** — printed once every Monday: every reminder
//!   (config + Vikunja) falling within the Mon–Sun of the current week,
//!   grouped by day.

use chrono::{Datelike, Duration, IsoWeek, Local, NaiveDate, Weekday};
use tracing::{info, warn};

use printer::PrintJob;
use crate::models::TodoItem;
use crate::recurring::RecurringTask;

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Loads reminders from `$APP_CONFIG_DIR/reminders.toml`.
/// Returns an empty list if the file does not exist.
pub fn load_config() -> Vec<RecurringTask> {
    let cfg_dir = std::env::var("APP_CONFIG_DIR").unwrap_or_else(|_| "config".to_string());
    let path = format!("{}/reminders.toml", cfg_dir);

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to read reminders.toml at {}: {}", path, e);
            }
            return Vec::new();
        }
    };

    #[derive(serde::Deserialize)]
    struct RemindersFile {
        #[serde(default)]
        reminders: Vec<RecurringTask>,
    }

    match toml::from_str::<RemindersFile>(&content) {
        Ok(f) => f.reminders,
        Err(e) => {
            warn!("Failed to parse reminders.toml: {}", e);
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Daily helpers
// ---------------------------------------------------------------------------

/// Config reminders whose schedule fires on today's local date.
pub fn config_due_today() -> Vec<RecurringTask> {
    let today = Local::now().date_naive();
    load_config().into_iter().filter(|t| t.is_due_on(today)).collect()
}

/// Vikunja tasks (non-completed) that have at least one reminder falling on
/// today's local date.
pub fn vikunja_due_today<'a>(items: &'a [TodoItem]) -> Vec<&'a TodoItem> {
    let today = Local::now().date_naive();
    items
        .iter()
        .filter(|i| !i.completed && i.reminders.iter().any(|r| r.date_naive() == today))
        .collect()
}

// ---------------------------------------------------------------------------
// Week helpers
// ---------------------------------------------------------------------------

fn week_bounds() -> (NaiveDate, NaiveDate) {
    let today = Local::now().date_naive();
    let mon = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    (mon, mon + Duration::days(6))
}

/// (date, RecurringTask) for every config reminder that fires during the
/// Mon–Sun of the current week.
pub fn config_due_this_week() -> Vec<(NaiveDate, RecurringTask)> {
    let (mon, sun) = week_bounds();
    let tasks = load_config();
    let mut result = Vec::new();
    let mut day = mon;
    while day <= sun {
        for task in &tasks {
            if task.is_due_on(day) {
                result.push((day, task.clone()));
            }
        }
        day += Duration::days(1);
    }
    result
}

/// (date, &TodoItem) for every Vikunja reminder falling within the Mon–Sun of
/// the current week.  A task with two reminders in the week appears twice.
pub fn vikunja_due_this_week<'a>(items: &'a [TodoItem]) -> Vec<(NaiveDate, &'a TodoItem)> {
    let (mon, sun) = week_bounds();
    let mut result: Vec<(NaiveDate, &TodoItem)> = items
        .iter()
        .filter(|i| !i.completed)
        .flat_map(|i| {
            i.reminders
                .iter()
                .map(|r| r.date_naive())
                .filter(|&d| d >= mon && d <= sun)
                .map(move |d| (d, i))
        })
        .collect();
    result.sort_by_key(|(d, _)| *d);
    result
}

// ---------------------------------------------------------------------------
// Weekly summary ticket
// ---------------------------------------------------------------------------

const LAST_WEEKLY_KEY: &str = "last_weekly_reminder_week";

/// Returns true (and marks the week as printed) if the weekly overview should
/// be printed — i.e. today is Monday and it hasn't been printed yet this week.
async fn should_print_weekly() -> bool {
    let today = Local::now().date_naive();
    if today.weekday() != Weekday::Mon {
        return false;
    }
    let iso: IsoWeek = today.iso_week();
    let week_key = format!("{}-W{:02}", iso.year(), iso.week());

    match db::setting_get(LAST_WEEKLY_KEY).await {
        Ok(Some(ref stored)) if stored == &week_key => {
            info!("Weekly reminder already printed this week ({}), skipping", week_key);
            return false;
        }
        Err(e) => warn!("Weekly reminder: could not read key: {}", e),
        _ => {}
    }
    if let Err(e) = db::setting_set(LAST_WEEKLY_KEY, week_key).await {
        warn!("Weekly reminder: failed to record week key: {}", e);
    }
    true
}

/// Prints the Monday weekly overview if it hasn't been printed yet this week.
pub async fn print_weekly_if_not_printed(items: &[TodoItem]) {
    if should_print_weekly().await {
        print_weekly_summary(items).await;
    }
}

/// Builds and prints the weekly reminder overview ticket.
pub async fn print_weekly_summary(items: &[TodoItem]) {
    let today = Local::now().date_naive();
    let (mon, sun) = week_bounds();
    let width = printer::line_width();
    let sep = "-".repeat(width);

    let iso: IsoWeek = today.iso_week();
    let week_badge = format!("[ WK {:02} {} ]", iso.week(), iso.year());
    let head = "WEEKLY REMINDERS";
    let gap = width.saturating_sub(head.len() + week_badge.len());
    let title = format!("{}{}{}", head, " ".repeat(gap), week_badge);

    let origin = format!("{} \u{2013} {}", mon.format("%d %b"), sun.format("%d %b %Y"));

    let cfg_week = config_due_this_week();
    let vjk_week = vikunja_due_this_week(items);

    let mut lines = vec![sep.clone(), String::new()];

    let mut any = false;
    let mut day = mon;
    while day <= sun {
        let cfg_day: Vec<_> = cfg_week.iter()
            .filter(|(d, _)| *d == day)
            .map(|(_, t)| t)
            .collect();
        let vjk_day: Vec<_> = vjk_week.iter()
            .filter(|(d, _)| *d == day)
            .map(|(_, i)| *i)
            .collect();

        if !cfg_day.is_empty() || !vjk_day.is_empty() {
            any = true;
            lines.push(day.format("%a %d %b").to_string().to_uppercase());
            for item in &vjk_day {
                let id_tag = item.id.map(|id| format!(" [#{}]", id)).unwrap_or_default();
                lines.push(format!("  ~ {}{}", item.title, id_tag));
            }
            for task in &cfg_day {
                lines.push(format!("  ~ {}", task.title));
            }
            lines.push(String::new());
        }

        day += Duration::days(1);
    }

    if !any {
        lines.push("  No reminders this week.".to_string());
        lines.push(String::new());
    }

    lines.push(sep);

    let job = PrintJob::new(origin, title, lines);
    if let Err(e) = job.execute(0, 0).await {
        warn!("Weekly reminder summary: print failed: {}", e);
    } else {
        info!("Weekly reminder summary printed");
    }
}
