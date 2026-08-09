//! Reminder support — todo item reminders and DB-backed reminders.
//!
//! DB reminders (`reminders` table) use the same schedule syntax/struct as
//! recurring tasks (`crate::recurring::RecurringTask`/`Schedule`), plus an
//! optional `fire_time` (`"HH:MM"` local) that, when set, makes
//! `todo::reminder_monitor` print a dedicated ticket and record a fired
//! occurrence at that exact time each day the reminder is due — rather than
//! only ever appearing bundled into summary output. Previously loaded from
//! `config/reminders.toml`; see [`migrate_from_toml_if_empty`] for the
//! one-time import of any existing file into the DB, run once at startup.
//!
//! Three outputs are produced:
//!
//! - **Exact-time ticket** (`fire_time` set only) — see `todo::reminder_monitor`.
//!
//! - **Daily summary section** — "REMINDERS TODAY": all DB reminders due
//!   today + all todo items whose `reminder_dates` include today.
//!
//! - **Weekly overview ticket** — printed once every Monday: every reminder
//!   (DB + todo item) falling within the Mon–Sun of the current week,
//!   grouped by day.

use chrono::{DateTime, Datelike, Duration, IsoWeek, Local, NaiveDate, Weekday};
use std::collections::HashMap;
use serde::Deserialize;
use tracing::{info, warn};

use printer::PrintJob;
use crate::models::TodoItem;
use crate::recurring::RecurringTask;
use crate::todo_error::{TodoLibError, TodoLibResult};

// ---------------------------------------------------------------------------
// A DB reminder — RecurringTask (schedule) + an optional exact fire time.
// ---------------------------------------------------------------------------

/// A single reminder, DB-backed (`reminders` table). Wraps a
/// [`RecurringTask`] (same `id`/title/description/schedule/reference_date
/// shape and scheduling logic) plus the one field reminders have that
/// recurring tasks don't: an optional exact daily fire time.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Reminder {
    #[serde(flatten)]
    pub task: RecurringTask,
    /// `"HH:MM"` local time. `None` = summary-only (today's/weekly bundled
    /// behavior only, never an individual ticket).
    pub fire_time: Option<String>,
}

impl std::ops::Deref for Reminder {
    type Target = RecurringTask;
    fn deref(&self) -> &RecurringTask { &self.task }
}

impl Reminder {
    fn from_row(row: db::models::ReminderRow) -> TodoLibResult<Self> {
        let reference_date = row
            .reference_date
            .as_deref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()
            .map_err(|e| TodoLibError::Db(format!("bad reference_date in reminders: {}", e)))?;
        Ok(Reminder {
            task: RecurringTask {
                id: row.id,
                title: row.title,
                description: row.description,
                schedule: row.schedule,
                reference_date,
                // Reminders have no priority concept of their own (only
                // recurring tasks do, for the frontend's merged List-tab
                // sort) — this field is simply unused for a Reminder.
                priority: 0,
            },
            fire_time: row.fire_time,
        })
    }
}

// ---------------------------------------------------------------------------
// DB-backed CRUD
// ---------------------------------------------------------------------------

/// Lists every reminder from the `reminders` DB table.
pub async fn load_reminders() -> TodoLibResult<Vec<Reminder>> {
    let rows = db::reminder_list().await.map_err(|e| TodoLibError::Db(e.to_string()))?;
    rows.into_iter().map(Reminder::from_row).collect()
}

/// Same list, as plain [`RecurringTask`]s (schedule fields only) — what the
/// summary/weekly-ticket helpers below actually need.
pub async fn load_config() -> TodoLibResult<Vec<RecurringTask>> {
    Ok(load_reminders().await?.into_iter().map(|r| r.task).collect())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_reminder(
    title: String,
    description: String,
    schedule: String,
    reference_date: Option<NaiveDate>,
    fire_time: Option<String>,
) -> TodoLibResult<i64> {
    crate::recurring::validate_schedule(&schedule)?;
    validate_fire_time(fire_time.as_deref())?;
    let reference_date_str = reference_date.map(|d| d.format("%Y-%m-%d").to_string());
    let created_at = Local::now().to_rfc3339();
    db::reminder_create(title, description, schedule, reference_date_str, fire_time, created_at)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))
}

#[allow(clippy::too_many_arguments)]
pub async fn update_reminder(
    id: i64,
    title: String,
    description: String,
    schedule: String,
    reference_date: Option<NaiveDate>,
    fire_time: Option<String>,
) -> TodoLibResult {
    crate::recurring::validate_schedule(&schedule)?;
    validate_fire_time(fire_time.as_deref())?;
    let reference_date_str = reference_date.map(|d| d.format("%Y-%m-%d").to_string());
    db::reminder_update(id, title, description, schedule, reference_date_str, fire_time)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))
}

pub async fn delete_reminder(id: i64) -> TodoLibResult {
    db::reminder_delete(id).await.map_err(|e| TodoLibError::Db(e.to_string()))
}

fn validate_fire_time(fire_time: Option<&str>) -> TodoLibResult {
    let Some(t) = fire_time else { return Ok(()) };
    chrono::NaiveTime::parse_from_str(t, "%H:%M")
        .map(|_| ())
        .map_err(|_| TodoLibError::InvalidSchedule(format!("invalid fire_time: '{}' (expected HH:MM)", t)))
}

// ---------------------------------------------------------------------------
// Occurrence tracking (fired/acknowledged/snoozed) — powers the frontend's
// "Today's Reminders" panel. No cross-crate bridging needed (unlike
// `finances`, which is journal-file-backed and lives in a separate crate
// from its occurrence tracking) — reminders and their occurrences are both
// already `db`-backed within this crate.
// ---------------------------------------------------------------------------

/// One reminder-occurrence row, joined with its reminder's title for display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReminderOccurrenceSummary {
    pub reminder_id: i64,
    pub title: String,
    pub occurrence_date: NaiveDate,
    pub fired_at: Option<String>,
    pub acknowledged: bool,
    pub snoozed_until: Option<String>,
}

/// Every reminder-occurrence fired on `date` — the bulk read backing the
/// frontend's "Today's Reminders" panel in one call.
pub async fn occurrences_on(date: NaiveDate) -> TodoLibResult<Vec<ReminderOccurrenceSummary>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let rows = db::reminder_occurrence_get_all().await.map_err(|e| TodoLibError::Db(e.to_string()))?;
    let titles: HashMap<i64, String> =
        load_reminders().await?.into_iter().map(|r| (r.id, r.title.clone())).collect();

    Ok(rows
        .into_iter()
        .filter(|r| r.occurrence_date == date_str)
        .filter_map(|r| {
            titles.get(&r.reminder_id).map(|title| ReminderOccurrenceSummary {
                reminder_id: r.reminder_id,
                title: title.clone(),
                occurrence_date: date,
                fired_at: r.fired_at,
                acknowledged: r.acknowledged,
                snoozed_until: r.snoozed_until,
            })
        })
        .collect())
}

/// Acknowledges (dismisses) or snoozes a fired reminder occurrence.
/// Preserves the occurrence's existing `fired_at` (and `acknowledged_at`
/// unless newly acknowledging) rather than requiring the caller to know it.
pub async fn set_occurrence_state(
    reminder_id: i64,
    date: NaiveDate,
    acknowledged: bool,
    snoozed_until: Option<DateTime<Local>>,
) -> TodoLibResult {
    let date_str = date.format("%Y-%m-%d").to_string();
    let existing = db::reminder_occurrence_get_for(reminder_id, date_str.clone())
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?;

    let fired_at = existing.as_ref().and_then(|r| r.fired_at.clone());
    let acknowledged_at = if acknowledged {
        Some(Local::now().to_rfc3339())
    } else {
        existing.as_ref().and_then(|r| r.acknowledged_at.clone())
    };

    db::reminder_occurrence_upsert(
        reminder_id,
        date_str,
        fired_at,
        acknowledged,
        acknowledged_at,
        snoozed_until.map(|d| d.to_rfc3339()),
    )
    .await
    .map_err(|e| TodoLibError::Db(e.to_string()))
}

// ---------------------------------------------------------------------------
// One-time TOML migration
// ---------------------------------------------------------------------------

/// Config-file shape `config/reminders.toml` used before this became
/// DB-backed — kept only for [`migrate_from_toml_if_empty`] to parse. No
/// `fire_time` existed in the file format; imported reminders are
/// summary-only until edited in the app.
#[derive(Debug, Deserialize)]
struct TomlReminder {
    title: String,
    #[serde(default)]
    description: String,
    schedule: String,
    #[serde(default)]
    reference_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct RemindersFile {
    #[serde(default)]
    reminders: Vec<TomlReminder>,
}

/// One-time import of `$APP_CONFIG_DIR/reminders.toml` into the `reminders`
/// DB table — only runs if that table is currently empty, so it's safe to
/// call unconditionally on every startup. Never writes to or deletes the
/// TOML file itself; it's simply no longer read afterward.
pub async fn migrate_from_toml_if_empty() -> TodoLibResult {
    let existing = db::reminder_list().await.map_err(|e| TodoLibError::Db(e.to_string()))?;
    if !existing.is_empty() {
        return Ok(());
    }

    let cfg_dir = std::env::var("APP_CONFIG_DIR").unwrap_or_else(|_| "config".to_string());
    let path = format!("{}/reminders.toml", cfg_dir);

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to read reminders.toml at {}: {}", path, e);
            }
            return Ok(());
        }
    };

    let reminders = match toml::from_str::<RemindersFile>(&content) {
        Ok(f) => f.reminders,
        Err(e) => {
            warn!("Failed to parse reminders.toml during migration: {}", e);
            return Ok(());
        }
    };

    let count = reminders.len();
    for r in reminders {
        create_reminder(r.title, r.description, r.schedule, r.reference_date, None).await?;
    }
    if count > 0 {
        info!("Imported {} reminder(s) from reminders.toml — safe to delete the file now", count);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Daily helpers
// ---------------------------------------------------------------------------

/// Config reminders whose schedule fires on today's local date.
pub async fn config_due_today() -> TodoLibResult<Vec<RecurringTask>> {
    let today = Local::now().date_naive();
    Ok(load_config().await?.into_iter().filter(|t| t.is_due_on(today)).collect())
}

/// Todo items (non-completed) that have at least one reminder falling on
/// today's local date.
pub fn todo_due_today(items: &[TodoItem]) -> Vec<&TodoItem> {
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
pub async fn config_due_this_week() -> TodoLibResult<Vec<(NaiveDate, RecurringTask)>> {
    let (mon, sun) = week_bounds();
    let tasks = load_config().await?;
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
    Ok(result)
}

/// (date, &TodoItem) for every todo item reminder falling within the Mon–Sun
/// of the current week.  A task with two reminders in the week appears twice.
pub fn todo_due_this_week(items: &[TodoItem]) -> Vec<(NaiveDate, &TodoItem)> {
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

    let cfg_week = match config_due_this_week().await {
        Ok(w) => w,
        Err(e) => {
            warn!("Failed to load reminders for weekly summary: {}", e);
            Vec::new()
        }
    };
    let todo_week = todo_due_this_week(items);

    let mut lines = vec![sep.clone(), String::new()];

    let mut any = false;
    let mut day = mon;
    while day <= sun {
        let cfg_day: Vec<_> = cfg_week.iter()
            .filter(|(d, _)| *d == day)
            .map(|(_, t)| t)
            .collect();
        let todo_day: Vec<_> = todo_week.iter()
            .filter(|(d, _)| *d == day)
            .map(|(_, i)| *i)
            .collect();

        if !cfg_day.is_empty() || !todo_day.is_empty() {
            any = true;
            lines.push(day.format("%a %d %b").to_string().to_uppercase());
            for item in &todo_day {
                let id_tag = item.id.map(|id| format!(" [#{}]", id)).unwrap_or_default();
                let proj = item.project_title.as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|p| format!(" [{}]", p))
                    .unwrap_or_default();
                lines.push(format!("  ~ {}{}{}", item.title, id_tag, proj));
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
