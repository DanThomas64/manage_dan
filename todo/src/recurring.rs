//! Recurring task support — loaded from `config/recurring.toml`.
//!
//! Recurring tasks are never stored as todo items or shown in the GUI/TUI.
//! Each day that a task is due, a dedicated physical ticket is printed once
//! (idempotent via the `recurring_printed` DB table) and the task appears in
//! the daily summary.
//!
//! ## Schedule syntax
//!
//! An optional `N:` prefix makes a schedule fire every N periods instead of
//! every period.  N defaults to 1 when omitted.
//!
//! | Value                 | Meaning                              |
//! |-----------------------|--------------------------------------|
//! | `"daily"`             | Every calendar day                   |
//! | `"2:daily"`           | Every other day                      |
//! | `"weekly:monday"`     | Every Monday                         |
//! | `"2:weekly:monday"`   | Every second Monday                  |
//! | `"monthly:15"`        | The 15th of each month               |
//! | `"3:monthly:1"`       | The 1st of every third month         |
//!
//! Multi-period schedules are anchored to 1970-01-01 for deterministic
//! counting.  Use the examples in `config/recurring.toml` to verify which
//! dates a given schedule lands on.

use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};
use serde::Deserialize;
use tracing::{info, warn};

use printer::PrintJob;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A single recurring task entry as read from `recurring.toml`.
#[derive(Debug, Deserialize, Clone)]
pub struct RecurringTask {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Schedule string: `"[N:]daily"`, `"[N:]weekly:<day>"`, or `"[N:]monthly:<d>"`.
    pub schedule: String,
    /// Optional anchor date for multi-period schedules (`"YYYY-MM-DD"`).
    ///
    /// When set, period counting starts from this date instead of 1970-01-01.
    /// Ignored for N=1 schedules.  The date does not need to fall on the
    /// correct weekday — the first matching weekday on or after it is used.
    #[serde(default)]
    pub reference_date: Option<NaiveDate>,
}

/// Parsed representation of a task's schedule.
///
/// The `u32` field is the period multiplier (1 = every occurrence, 2 = every
/// other, etc.).
#[derive(Debug, Clone, PartialEq)]
pub enum Schedule {
    /// Every `n` days.
    Daily(u32),
    /// Every `n` weeks on the given weekday.
    Weekly(u32, Weekday),
    /// Every `n` months on the given day-of-month (1–31).
    Monthly(u32, u32),
}

impl RecurringTask {
    /// Parses `self.schedule` into a typed [`Schedule`], or `None` if invalid.
    pub fn parsed_schedule(&self) -> Option<Schedule> {
        let s = self.schedule.to_ascii_lowercase();

        // Extract an optional "N:" multiplier prefix.
        // Strategy: if the string starts with digits followed by ':', and the
        // remainder still contains a recognised keyword, treat the digits as N.
        let (n, rest): (u32, &str) = {
            if let Some(colon) = s.find(':') {
                if let Ok(num) = s[..colon].parse::<u32>() {
                    if num >= 1 {
                        (num, &s[colon + 1..])
                    } else {
                        (1, s.as_str())
                    }
                } else {
                    // First token is not a number (e.g. "weekly:monday") — no prefix.
                    (1, s.as_str())
                }
            } else {
                (1, s.as_str())
            }
        };

        if rest == "daily" {
            return Some(Schedule::Daily(n));
        }

        if let Some(day_str) = rest.strip_prefix("weekly:") {
            let day = match day_str.trim() {
                "monday"    | "mon" => Some(Weekday::Mon),
                "tuesday"   | "tue" => Some(Weekday::Tue),
                "wednesday" | "wed" => Some(Weekday::Wed),
                "thursday"  | "thu" => Some(Weekday::Thu),
                "friday"    | "fri" => Some(Weekday::Fri),
                "saturday"  | "sat" => Some(Weekday::Sat),
                "sunday"    | "sun" => Some(Weekday::Sun),
                _ => None,
            };
            return day.map(|d| Schedule::Weekly(n, d));
        }

        if let Some(day_str) = rest.strip_prefix("monthly:") {
            if let Ok(d) = day_str.trim().parse::<u32>() {
                if (1..=31).contains(&d) {
                    return Some(Schedule::Monthly(n, d));
                }
            }
        }

        None
    }

    /// Returns `true` if this task is due on the given date.
    pub fn is_due_on(&self, date: NaiveDate) -> bool {
        let reference = self.reference_date.unwrap_or_else(EPOCH);
        match self.parsed_schedule() {
            Some(Schedule::Daily(1))       => true,
            Some(Schedule::Daily(n))       => days_since_ref(date, reference) % n as i64 == 0,
            Some(Schedule::Weekly(1, day)) => date.weekday() == day,
            Some(Schedule::Weekly(n, day)) => {
                date.weekday() == day && weeks_since_ref(date, day, reference) % n as i64 == 0
            }
            Some(Schedule::Monthly(1, d))  => date.day() == d,
            Some(Schedule::Monthly(n, d))  => {
                date.day() == d && months_since_ref(date, reference) % n as i64 == 0
            }
            None => false,
        }
    }

    /// Returns `true` if this task is due on today's local date.
    pub fn is_due_today(&self) -> bool {
        self.is_due_on(Local::now().date_naive())
    }

    /// Human-readable schedule description, e.g. "Every 2 Mondays".
    pub fn schedule_display(&self) -> String {
        match self.parsed_schedule() {
            Some(Schedule::Daily(1))    => "Every day".to_string(),
            Some(Schedule::Daily(n))    => format!("Every {} days", n),
            Some(Schedule::Weekly(1, day)) => format!("Every {}", weekday_name(day)),
            Some(Schedule::Weekly(n, day)) => format!("Every {} {}s", n, weekday_name(day)),
            Some(Schedule::Monthly(1, d))  => {
                format!("Monthly on the {}{}", d, ordinal_suffix(d))
            }
            Some(Schedule::Monthly(n, d))  => {
                format!("Every {} months on the {}{}", n, d, ordinal_suffix(d))
            }
            None => format!("Unknown ({})", self.schedule),
        }
    }
}

// ---------------------------------------------------------------------------
// Period counting (reference-date anchored)
// ---------------------------------------------------------------------------

const EPOCH: fn() -> NaiveDate = || NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();

/// Number of days between `reference` and `date` (clamped to 0 if negative).
fn days_since_ref(date: NaiveDate, reference: NaiveDate) -> i64 {
    (date - reference).num_days().max(0)
}

/// Number of complete weeks of `weekday` that have elapsed since the first
/// occurrence of `weekday` on or after `reference`.
fn weeks_since_ref(date: NaiveDate, weekday: Weekday, reference: NaiveDate) -> i64 {
    let offset = (weekday.num_days_from_monday() as i64
        - reference.weekday().num_days_from_monday() as i64)
        .rem_euclid(7);
    let anchor = reference + Duration::days(offset);
    let days = (date - anchor).num_days();
    if days < 0 { 0 } else { days / 7 }
}

/// Number of complete months since `reference` (0-indexed).
fn months_since_ref(date: NaiveDate, reference: NaiveDate) -> i64 {
    (date.year() as i64 - reference.year() as i64) * 12
        + (date.month() as i64 - reference.month() as i64)
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RecurringConfig {
    #[serde(default)]
    tasks: Vec<RecurringTask>,
}

/// Loads all tasks from `$APP_CONFIG_DIR/recurring.toml` (default: `config/recurring.toml`).
/// Returns an empty list if the file does not exist.
pub fn load_config() -> Vec<RecurringTask> {
    let cfg_dir = std::env::var("APP_CONFIG_DIR").unwrap_or_else(|_| "config".to_string());
    let path = format!("{}/recurring.toml", cfg_dir);

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to read recurring.toml at {}: {}", path, e);
            }
            return Vec::new();
        }
    };

    match toml::from_str::<RecurringConfig>(&content) {
        Ok(cfg) => cfg.tasks,
        Err(e) => {
            warn!("Failed to parse recurring.toml: {}", e);
            Vec::new()
        }
    }
}

/// Returns only the tasks that are due on today's local date.
pub fn due_today() -> Vec<RecurringTask> {
    load_config().into_iter().filter(|t| t.is_due_today()).collect()
}

// ---------------------------------------------------------------------------
// Ticket printing
// ---------------------------------------------------------------------------

/// Prints a distinctly-formatted recurring-task ticket for `task`.
///
/// The ticket uses `~` separators and a `RECURRING … [ RECURRING ]` header
/// to make it immediately distinguishable from regular TODO tickets.
pub async fn print_ticket(task: &RecurringTask) {
    let width = printer::line_width();
    let sep = "~".repeat(width);

    // Header: "RECURRING          [ RECURRING ]"
    let badge = "[ RECURRING ]";
    let label = "RECURRING";
    let gap = width.saturating_sub(label.len() + badge.len());
    let header = format!("{}{}{}", label, " ".repeat(gap), badge);

    // Sub-header (origin line printed by printer as the bold title)
    let origin = task.title.clone();

    // First body line: date + schedule
    let today = Local::now();
    let date_schedule = format!(
        "{}  |  {}",
        today.format("%a %d %b %Y"),
        task.schedule_display()
    );

    let mut lines = vec![date_schedule, sep.clone(), String::new()];

    if !task.description.is_empty() {
        lines.extend(task.description.lines().map(str::to_string));
        lines.push(String::new());
    } else {
        // Pad so the ticket has physical presence on paper.
        lines.push(String::new());
        lines.push(String::new());
        lines.push(String::new());
    }

    lines.push(sep);

    let job = PrintJob::new(origin, header, lines);
    if let Err(e) = job.execute(0, 0).await {
        warn!("Failed to print recurring task '{}': {}", task.title, e);
    } else {
        info!("Recurring task ticket printed: '{}'", task.title);
    }
}

// ---------------------------------------------------------------------------
// Idempotent daily print
// ---------------------------------------------------------------------------

/// Prints tickets for all recurring tasks due today that have not yet been
/// printed today.  Safe to call multiple times — each task is printed at most
/// once per calendar day.
pub async fn print_due_today_if_not_printed() {
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let tasks = due_today();

    if tasks.is_empty() {
        return;
    }

    for task in &tasks {
        let already = db::recurring_printed_check(today.clone(), task.title.clone())
            .await
            .unwrap_or(false);

        if already {
            info!("Recurring task '{}' already printed today — skipping", task.title);
            continue;
        }

        print_ticket(task).await;

        if let Err(e) = db::recurring_printed_record(today.clone(), task.title.clone()).await {
            warn!("Failed to record recurring print for '{}': {}", task.title, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

fn ordinal_suffix(n: u32) -> &'static str {
    match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn task(schedule: &str) -> RecurringTask {
        RecurringTask { title: "test".into(), description: String::new(), schedule: schedule.into(), reference_date: None }
    }

    // --- parsing ---

    #[test]
    fn parse_daily() {
        assert_eq!(task("daily").parsed_schedule(), Some(Schedule::Daily(1)));
    }

    #[test]
    fn parse_n_daily() {
        assert_eq!(task("3:daily").parsed_schedule(), Some(Schedule::Daily(3)));
    }

    #[test]
    fn parse_weekly() {
        assert_eq!(task("weekly:monday").parsed_schedule(), Some(Schedule::Weekly(1, Weekday::Mon)));
        assert_eq!(task("weekly:fri").parsed_schedule(),    Some(Schedule::Weekly(1, Weekday::Fri)));
    }

    #[test]
    fn parse_n_weekly() {
        assert_eq!(task("2:weekly:monday").parsed_schedule(), Some(Schedule::Weekly(2, Weekday::Mon)));
    }

    #[test]
    fn parse_monthly() {
        assert_eq!(task("monthly:15").parsed_schedule(), Some(Schedule::Monthly(1, 15)));
    }

    #[test]
    fn parse_n_monthly() {
        assert_eq!(task("3:monthly:1").parsed_schedule(), Some(Schedule::Monthly(3, 1)));
    }

    #[test]
    fn parse_invalid() {
        assert_eq!(task("fortnightly").parsed_schedule(), None);
        assert_eq!(task("0:daily").parsed_schedule(), None);
    }

    // --- period helpers ---

    #[test]
    fn days_since_ref_on_reference() {
        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        assert_eq!(days_since_ref(epoch, epoch), 0);
    }

    #[test]
    fn weeks_since_ref_on_anchor_monday() {
        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        // 1970-01-05 is the first Monday >= epoch → week 0.
        let d = NaiveDate::from_ymd_opt(1970, 1, 5).unwrap();
        assert_eq!(weeks_since_ref(d, Weekday::Mon, epoch), 0);
        // 1970-01-12 is the second Monday → week 1.
        let d2 = NaiveDate::from_ymd_opt(1970, 1, 12).unwrap();
        assert_eq!(weeks_since_ref(d2, Weekday::Mon, epoch), 1);
    }

    #[test]
    fn biweekly_fires_on_even_weeks_epoch() {
        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let w0 = NaiveDate::from_ymd_opt(1970, 1, 5).unwrap();
        assert_eq!(weeks_since_ref(w0, Weekday::Mon, epoch) % 2, 0);
        let w1 = NaiveDate::from_ymd_opt(1970, 1, 12).unwrap();
        assert_ne!(weeks_since_ref(w1, Weekday::Mon, epoch) % 2, 0);
        let w2 = NaiveDate::from_ymd_opt(1970, 1, 19).unwrap();
        assert_eq!(weeks_since_ref(w2, Weekday::Mon, epoch) % 2, 0);
    }

    #[test]
    fn biweekly_with_reference_date() {
        // Reference is 2026-03-23 (a Monday).  That's week 0 → due.
        let reference = NaiveDate::from_ymd_opt(2026, 3, 23).unwrap();
        assert_eq!(weeks_since_ref(reference, Weekday::Mon, reference), 0);
        // 2026-03-30 (next Monday) → week 1 → not due for N=2.
        let next = NaiveDate::from_ymd_opt(2026, 3, 30).unwrap();
        assert_ne!(weeks_since_ref(next, Weekday::Mon, reference) % 2, 0);
        // 2026-04-06 (Monday after that) → week 2 → due for N=2.
        let due = NaiveDate::from_ymd_opt(2026, 4, 6).unwrap();
        assert_eq!(weeks_since_ref(due, Weekday::Mon, reference) % 2, 0);
    }

    // --- display ---

    #[test]
    fn display_n_weekly() {
        assert_eq!(task("2:weekly:monday").schedule_display(), "Every 2 Mondays");
    }

    #[test]
    fn display_n_monthly() {
        assert_eq!(task("3:monthly:1").schedule_display(), "Every 3 months on the 1st");
    }
}
