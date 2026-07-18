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

use chrono::{Duration, Local, NaiveDate, TimeZone};
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
    pub fn from_config_str(s: &str) -> Self {
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
            let proj = item.project_title.as_deref()
                .filter(|s| !s.is_empty())
                .map(|p| format!(" [{}]", p))
                .unwrap_or_default();
            lines.push(format!("  [{}] {}{} (due {})", item.priority, item.title, proj, due_str));
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
                let proj = item.project_title.as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|p| format!(" [{}]", p))
                    .unwrap_or_default();
                lines.push(format!("  [{}] {}{} ({})", item.priority, item.title, proj, due_str));
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
                let proj = item.project_title.as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|p| format!(" [{}]", p))
                    .unwrap_or_default();
                lines.push(format!("  [{}] {}{} ({})", item.priority, item.title, proj, due_str));
            }
        }
    }

    // --- Recurring tasks due today ---
    let recurring = crate::recurring::due_today();
    lines.push(String::new());
    lines.push(format!("RECURRING TODAY ({}):", recurring.len()));
    if recurring.is_empty() {
        lines.push("  None".to_string());
    } else {
        for task in &recurring {
            lines.push(format!("  ~ {}", task.title));
        }
    }

    // --- Reminders today ---
    let cfg_reminders = crate::reminders::config_due_today();
    let vjk_reminders = crate::reminders::vikunja_due_today(&items);
    let total_reminders = cfg_reminders.len() + vjk_reminders.len();
    lines.push(String::new());
    lines.push(format!("REMINDERS TODAY ({}):", total_reminders));
    if total_reminders == 0 {
        lines.push("  None".to_string());
    } else {
        for item in &vjk_reminders {
            let id_tag = item.id.map(|id| format!(" [#{}]", id)).unwrap_or_default();
            let proj = item.project_title.as_deref()
                .filter(|s| !s.is_empty())
                .map(|p| format!(" [{}]", p))
                .unwrap_or_default();
            lines.push(format!("  ~ {}{}{}", item.title, id_tag, proj));
        }
        for task in &cfg_reminders {
            lines.push(format!("  ~ {}", task.title));
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

    crate::reminders::print_weekly_if_not_printed(&items).await;
}

// ---------------------------------------------------------------------------
// Startup guard
// ---------------------------------------------------------------------------

const LAST_SUMMARY_DATE_KEY: &str = "last_summary_date";

/// Checks whether the summary has already been printed on `today`.
/// If not, atomically marks it as printed and returns `true` (caller should print).
/// If already marked, returns `false` (caller should skip).
async fn check_and_mark(today: NaiveDate) -> bool {
    let today_str = today.format("%Y-%m-%d").to_string();
    match db::setting_get(LAST_SUMMARY_DATE_KEY).await {
        Ok(Some(ref stored)) if stored == &today_str => {
            info!("Daily summary already printed today ({}), skipping", today_str);
            return false;
        }
        Err(e) => {
            warn!("Daily summary: could not read last_summary_date: {}", e);
        }
        _ => {}
    }
    if let Err(e) = db::setting_set(LAST_SUMMARY_DATE_KEY, today_str).await {
        warn!("Daily summary: failed to record last_summary_date: {}", e);
    }
    true
}

/// Prints the daily summary only if it hasn't been printed yet today.
pub async fn print_summary_if_not_today(level: SummaryLevel) {
    if check_and_mark(Local::now().date_naive()).await {
        print_summary(level).await;
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
        print_summary_if_not_today(level).await;
        crate::recurring::print_due_today_if_not_printed().await;

        let wait = secs_until_next_hour(hour);
        sleep(tokio::time::Duration::from_secs(wait)).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use tokio::sync::Mutex;

    /// Serializes all DB-touching tests so `set_current_dir` doesn't race.
    /// An async-aware `Mutex` since the guard is held across `.await` points.
    static TEST_DB_LOCK: Mutex<()> = Mutex::const_new(());

    fn setup_test_db() {
        let dir = std::env::temp_dir().join("manage_dan_summary_test");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(db::DB_FILE));
        std::env::set_current_dir(&dir).unwrap();
        db::init().unwrap();
    }

    /// Simulates the fixed behaviour: startup fires before summary hour, then the
    /// scheduler fires at the summary hour — both on the same calendar day.
    /// The summary should only be printed once.
    #[tokio::test]
    async fn same_day_startup_and_scheduler_prints_once() {
        let _guard = TEST_DB_LOCK.lock().await;
        setup_test_db();

        let day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        // Startup (before summary hour): not yet printed → should print.
        assert!(check_and_mark(day).await, "startup: should want to print");

        // Scheduler fires at summary hour same day → should skip.
        assert!(!check_and_mark(day).await, "scheduler: should skip, already printed today");

        // Any further spurious calls same day → still skip.
        assert!(!check_and_mark(day).await, "extra call: should still skip");
    }

    /// Simulates three consecutive days: each day triggers exactly one print
    /// regardless of how many times the guard is called.
    #[tokio::test]
    async fn each_new_day_triggers_exactly_one_print() {
        let _guard = TEST_DB_LOCK.lock().await;
        setup_test_db();

        for day_offset in 0..3i64 {
            let day = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
                + chrono::Duration::days(day_offset);

            // First call of the day: should print.
            assert!(
                check_and_mark(day).await,
                "day {}: first call should print",
                day_offset + 1
            );

            // Subsequent calls same day (startup + scheduler overlap): should skip.
            assert!(
                !check_and_mark(day).await,
                "day {}: second call should skip",
                day_offset + 1
            );
            assert!(
                !check_and_mark(day).await,
                "day {}: third call should skip",
                day_offset + 1
            );
        }
    }
}
