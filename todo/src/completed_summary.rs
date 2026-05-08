//! End-of-day completed-task summary printer.
//!
//! Runs once per day at a configured hour (default 20:00 local time).
//! Lists every task whose `completed_at` falls on today's date.
//! If nothing was completed the ticket still prints as a confirmation.

use chrono::{Duration, Local, NaiveDate, TimeZone};
use tracing::{info, warn};
use tokio::time::sleep;

use printer::PrintJob;

// ---------------------------------------------------------------------------
// Summary printer
// ---------------------------------------------------------------------------

/// Builds and sends the completed-task summary print job.
pub async fn print_summary() {
    let items = match crate::read_items().await {
        Ok(v) => v,
        Err(e) => {
            warn!("Completed summary: failed to fetch tasks: {}", e);
            return;
        }
    };

    let now = Local::now();
    let today = now.date_naive();

    let mut completed_today: Vec<_> = items
        .iter()
        .filter(|i| {
            i.completed
                && i.completed_at
                    .map(|d| d.date_naive() == today)
                    .unwrap_or(false)
        })
        .collect();

    // Sort chronologically by completion time.
    completed_today.sort_by_key(|i| i.completed_at);

    let title = "COMPLETED TODAY".to_string();
    let date_line = today.format("%a %d %b %Y").to_string();
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("COMPLETED ({}):", completed_today.len()));
    if completed_today.is_empty() {
        lines.push("  None".to_string());
    } else {
        for item in &completed_today {
            let time_str = item
                .completed_at
                .map(|d| d.format("%H:%M").to_string())
                .unwrap_or_default();
            let proj = item
                .project_title
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|p| format!(" [{}]", p))
                .unwrap_or_default();
            lines.push(format!("  [x] {}{} ({})", item.title, proj, time_str));
        }
    }

    lines.push(String::new());
    lines.push(format!("Generated: {}", now.format("%H:%M")));

    let job = PrintJob::new(date_line, title, lines);
    if let Err(e) = job.execute(0, 0).await {
        warn!("Completed summary: print failed: {}", e);
    } else {
        info!("Completed summary printed successfully");
    }
}

// ---------------------------------------------------------------------------
// Startup guard
// ---------------------------------------------------------------------------

const LAST_COMPLETED_SUMMARY_KEY: &str = "last_completed_summary_date";

async fn check_and_mark(today: NaiveDate) -> bool {
    let today_str = today.format("%Y-%m-%d").to_string();
    match db::setting_get(LAST_COMPLETED_SUMMARY_KEY).await {
        Ok(Some(ref stored)) if stored == &today_str => {
            info!(
                "Completed summary already printed today ({}), skipping",
                today_str
            );
            return false;
        }
        Err(e) => {
            warn!("Completed summary: could not read last_completed_summary_date: {}", e);
        }
        _ => {}
    }
    if let Err(e) = db::setting_set(LAST_COMPLETED_SUMMARY_KEY, today_str).await {
        warn!("Completed summary: failed to record key: {}", e);
    }
    true
}

/// Prints the completed summary only if it hasn't been printed yet today.
pub async fn print_summary_if_not_today() {
    if check_and_mark(Local::now().date_naive()).await {
        print_summary().await;
    }
}

// ---------------------------------------------------------------------------
// Background scheduler
// ---------------------------------------------------------------------------

fn secs_until_next_hour(hour: u32) -> u64 {
    let now = Local::now();
    let today_naive = now
        .date_naive()
        .and_hms_opt(hour, 0, 0)
        .expect("invalid hour");
    let today_at = Local
        .from_local_datetime(&today_naive)
        .single()
        .expect("ambiguous local time");

    let target = if today_at > now {
        today_at
    } else {
        let tomorrow = (now + Duration::days(1)).date_naive();
        let tomorrow_naive = tomorrow.and_hms_opt(hour, 0, 0).expect("invalid hour");
        Local
            .from_local_datetime(&tomorrow_naive)
            .single()
            .expect("ambiguous local time")
    };

    (target - now).num_seconds().max(0) as u64
}

/// Starts the completed-task summary background task.
///
/// Prints an immediate summary on startup (skipped if already printed today),
/// then fires again every day at `hour:00` local time.
pub async fn run(hour: u32) {
    info!(
        "Completed summary scheduler started (fires at {:02}:00 local)",
        hour
    );

    let wait = secs_until_next_hour(hour);
    info!(
        "Completed summary: first scheduled run in {}h {}m",
        wait / 3600,
        (wait % 3600) / 60
    );
    sleep(tokio::time::Duration::from_secs(wait)).await;

    loop {
        info!("Completed summary: running");
        print_summary_if_not_today().await;

        let wait = secs_until_next_hour(hour);
        sleep(tokio::time::Duration::from_secs(wait)).await;
    }
}
