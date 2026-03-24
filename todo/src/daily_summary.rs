//! Daily summary printer.
//!
//! Runs once per day at a configured hour (default 08:00 local time).
//! The sections included depend on the configured `SummaryLevel`:
//!
//! | Level      | Overdue | High-priority | Upcoming (7 days) |
//! |------------|---------|---------------|-------------------|
//! | `minimal`  | ✓       |               |                   |
//! | `standard` | ✓       | ✓             |                   |
//! | `full`     | ✓       | ✓             | ✓                 |
//!
//! If no category has items the ticket is still printed so you always get a
//! morning confirmation that nothing is outstanding.

use chrono::{Duration, Local, TimeZone};
use tracing::{info, warn};
use tokio::time::sleep;

use printer::PrintJob;

// ---------------------------------------------------------------------------
// SummaryLevel
// ---------------------------------------------------------------------------

/// Controls which sections are included in the printed daily summary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SummaryLevel {
    /// Overdue tasks only.
    Minimal,
    /// Overdue + high-priority (priority ≥ 4) tasks.
    Standard,
    /// Overdue + high-priority + tasks due within the next 7 days.
    Full,
}

impl SummaryLevel {
    /// Parses a config string.  Unrecognised values fall back to `Full`.
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "minimal"  => Self::Minimal,
            "standard" => Self::Standard,
            _          => Self::Full,
        }
    }
}

// ---------------------------------------------------------------------------
// Scheduling helper
// ---------------------------------------------------------------------------

/// Seconds until the next occurrence of `hour:00` local time.
fn secs_until_next_hour(hour: u32) -> u64 {
    let now = Local::now();
    let today_naive = now.date_naive().and_hms_opt(hour, 0, 0)
        .expect("invalid hour");
    let today_at = Local.from_local_datetime(&today_naive)
        .single()
        .expect("ambiguous local time");

    let target = if today_at > now {
        today_at
    } else {
        let tomorrow = (now + Duration::days(1)).date_naive();
        let tomorrow_naive = tomorrow.and_hms_opt(hour, 0, 0)
            .expect("invalid hour");
        Local.from_local_datetime(&tomorrow_naive)
            .single()
            .expect("ambiguous local time")
    };

    (target - now).num_seconds().max(0) as u64
}

// ---------------------------------------------------------------------------
// Summary printer
// ---------------------------------------------------------------------------

/// Builds and sends the daily summary print job.
pub async fn print_summary(level: SummaryLevel) {
    let items = match crate::read_items().await {
        Ok(v) => v,
        Err(e) => {
            warn!("Daily summary: failed to fetch tasks: {}", e);
            return;
        }
    };

    let now = Local::now();
    let week_out = now + Duration::days(7);
    let today = now.date_naive();

    // Always included.
    let overdue: Vec<_> = items
        .iter()
        .filter(|i| !i.completed && i.due_date.map(|d| d < now).unwrap_or(false))
        .collect();

    // Included for Standard and Full.
    let high_priority: Vec<_> = items
        .iter()
        .filter(|i| !i.completed && i.priority >= 4)
        .collect();

    // Included for Full only — due > now and due <= now + 7 days (excludes overdue).
    let upcoming: Vec<_> = items
        .iter()
        .filter(|i| {
            !i.completed
                && i.due_date
                    .map(|d| d >= now && d <= week_out)
                    .unwrap_or(false)
        })
        .collect();

    let title = "DAILY SUMMARY".to_string();
    let date_line = today.format("%a %d %b %Y").to_string();
    let mut lines: Vec<String> = Vec::new();

    // --- Overdue (always) ---
    lines.push(format!("OVERDUE ({}):", overdue.len()));
    if overdue.is_empty() {
        lines.push("  None".to_string());
    } else {
        for item in &overdue {
            let due_str = item.due_date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            lines.push(format!("  [{}] {} (due {})", item.priority, item.title, due_str));
        }
    }

    // --- High-priority (Standard+) ---
    if level != SummaryLevel::Minimal {
        lines.push(String::new());
        lines.push(format!("HIGH PRIORITY ({}):", high_priority.len()));
        if high_priority.is_empty() {
            lines.push("  None".to_string());
        } else {
            for item in &high_priority {
                let due_str = item.due_date
                    .map(|d| format!("due {}", d.format("%Y-%m-%d")))
                    .unwrap_or_else(|| "no due date".to_string());
                lines.push(format!("  [{}] {} ({})", item.priority, item.title, due_str));
            }
        }
    }

    // --- Upcoming next 7 days (Full only) ---
    if level == SummaryLevel::Full {
        lines.push(String::new());
        lines.push(format!("UPCOMING 7 DAYS ({}):", upcoming.len()));
        if upcoming.is_empty() {
            lines.push("  None".to_string());
        } else {
            for item in &upcoming {
                let due_str = item.due_date
                    .map(|d| d.format("%a %d %b").to_string())
                    .unwrap_or_default();
                lines.push(format!("  [{}] {} ({})", item.priority, item.title, due_str));
            }
        }
    }

    lines.push(String::new());

    // --- Footer ---
    let pending = items.iter().filter(|i| !i.completed).count();
    lines.push(format!("Total pending: {}", pending));
    lines.push(format!("Generated: {}", now.format("%H:%M")));

    let job = PrintJob::new(date_line, title, lines);
    if let Err(e) = job.execute(0, 0).await {
        warn!("Daily summary: print failed: {}", e);
    } else {
        info!("Daily summary printed successfully");
    }
}

// ---------------------------------------------------------------------------
// Background scheduler
// ---------------------------------------------------------------------------

/// Starts the daily summary background task.
///
/// Prints an immediate summary on first call to `run`, then fires again every
/// day at `hour:00` local time.
pub async fn run(hour: u32, level: SummaryLevel) {
    info!(
        "Daily summary scheduler started (fires at {:02}:00 local, level: {:?})",
        hour, level
    );

    let wait = secs_until_next_hour(hour);
    info!(
        "Daily summary: first scheduled run in {}h {}m",
        wait / 3600,
        (wait % 3600) / 60
    );
    sleep(tokio::time::Duration::from_secs(wait)).await;

    loop {
        info!("Daily summary: running");
        print_summary(level).await;

        let wait = secs_until_next_hour(hour);
        sleep(tokio::time::Duration::from_secs(wait)).await;
    }
}
