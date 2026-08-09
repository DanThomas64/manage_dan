//! Background reminder monitor.
//!
//! Polls DB-backed reminders (`db::reminders`) on the same interval as
//! `todo::monitor`/`notes::monitor`. A reminder with `fire_time` set fires
//! (prints a dedicated ticket, writes a daily-log entry tagged
//! `reminder-fired`, and records a `reminder_occurrences` row) once per
//! calendar day it's due, at that local time — or again after a snooze set
//! on it has elapsed. Reminders with no `fire_time` are summary-only and
//! untouched here (see `todo::reminders::config_due_today`/`print_weekly_summary`).
//!
//! This is the intended future call site for a push/local-notification
//! system — `fire_reminder` is the one place a reminder's firing actually
//! happens, so a later notification dispatch attaches here rather than
//! requiring changes to the scheduling/occurrence logic above.

use chrono::{Local, NaiveTime};
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::reminders::Reminder;
use printer::PrintJob;

/// Runs forever, checking every `interval_secs` whether any reminder is due
/// to fire right now.
pub async fn run(interval_secs: u64) {
    info!("Reminder monitor started (interval: {}s)", interval_secs);

    loop {
        if let Err(e) = poll().await {
            warn!("Reminder monitor poll error: {}", e);
        }
        sleep(Duration::from_secs(interval_secs)).await;
    }
}

async fn poll() -> Result<(), String> {
    let reminders = crate::reminders::load_reminders().await.map_err(|e| e.to_string())?;
    let today = Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();
    let now = Local::now();

    for reminder in reminders {
        let Some(fire_time_str) = reminder.fire_time.as_deref() else { continue };
        if !reminder.is_due_on(today) {
            continue;
        }
        let Ok(fire_time) = NaiveTime::parse_from_str(fire_time_str, "%H:%M") else {
            warn!("Reminder '{}' has an unparseable fire_time '{}' — skipping", reminder.title, fire_time_str);
            continue;
        };

        let existing = db::reminder_occurrence_get_for(reminder.id, today_str.clone())
            .await
            .map_err(|e| e.to_string())?;

        let should_fire = match &existing {
            None => now.time() >= fire_time,
            Some(row) if row.acknowledged => false,
            Some(row) => match row.snoozed_until.as_deref() {
                Some(snoozed) => chrono::DateTime::parse_from_rfc3339(snoozed)
                    .map(|s| s.with_timezone(&Local) <= now)
                    .unwrap_or(false),
                None => false,
            },
        };

        if should_fire {
            fire_reminder(&reminder, &today_str, now).await;
        }
    }

    Ok(())
}

/// Prints a dedicated ticket for `reminder`, logs it, and records it as
/// fired for `occurrence_date`. The one hook point a future notification-
/// delivery system should call alongside (or instead of) the print.
async fn fire_reminder(reminder: &Reminder, occurrence_date: &str, now: chrono::DateTime<Local>) {
    print_reminder_ticket(reminder).await;
    log_reminder_fired(reminder).await;

    if let Err(e) = db::reminder_occurrence_upsert(
        reminder.id,
        occurrence_date.to_string(),
        Some(now.to_rfc3339()),
        false,
        None,
        None,
    )
    .await
    {
        warn!("Failed to record reminder occurrence for '{}': {}", reminder.title, e);
    }
}

/// Writes a daily-log entry for a fired reminder — same best-effort
/// convention as `todo::log_completion` (a logging failure only warns, it
/// never fails the firing itself, since the `log` notebook is a `notes`-
/// crate concept independent of this one).
async fn log_reminder_fired(reminder: &Reminder) {
    let req = notes::CreateLogRequest {
        title: format!("Reminder fired: {}", reminder.title),
        content: if reminder.description.trim().is_empty() {
            "(no description)".to_string()
        } else {
            reminder.description.clone()
        },
        tags: Some(vec!["reminder-fired".to_string()]),
    };

    if let Err(e) = notes::create_log(req).await {
        warn!("Failed to log firing of reminder '{}': {}", reminder.title, e);
    }
}

/// Prints a distinctly-formatted reminder ticket — same `~`-separator
/// convention as `recurring::print_ticket`, with a `[ REMINDER ]` badge.
async fn print_reminder_ticket(reminder: &Reminder) {
    let width = printer::line_width();
    let sep = "~".repeat(width);

    let badge = "[ REMINDER ]";
    let label = "REMINDER";
    let gap = width.saturating_sub(label.len() + badge.len());
    let header = format!("{}{}{}", label, " ".repeat(gap), badge);

    let origin = reminder.title.clone();

    let today = Local::now();
    let date_schedule = format!(
        "{}  |  {}",
        today.format("%a %d %b %Y"),
        reminder.schedule_display()
    );

    let mut lines = vec![date_schedule, sep.clone(), String::new()];

    if !reminder.description.is_empty() {
        lines.extend(reminder.description.lines().map(str::to_string));
        lines.push(String::new());
    } else {
        lines.push(String::new());
        lines.push(String::new());
        lines.push(String::new());
    }

    lines.push(sep);

    let job = PrintJob::new(origin, header, lines);
    if let Err(e) = job.execute(0, 0).await {
        warn!("Failed to print reminder '{}': {}", reminder.title, e);
    } else {
        info!("Reminder ticket printed: '{}'", reminder.title);
    }
}
