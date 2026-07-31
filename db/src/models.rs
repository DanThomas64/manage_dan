//! Database models used across the application.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Local};

/// Represents a single log entry stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: DateTime<Local>,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// A cached subtask — structurally identical to `todo::models::Subtask`.
/// `db` can't import that type directly (`todo` already depends on `db`),
/// so this is the cache-layer's own mirror; conversion at the call site in
/// the `todo` crate is a plain field-for-field mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSubtask {
    pub id: Option<i64>,
    pub title: String,
    pub done: bool,
}

/// A local mirror of a `todo::models::TodoItem`, fast to read/write locally
/// (SQLite) instead of round-tripping through `nb` on every read. `nb`
/// remains the source of truth — this is a cache, kept in sync by the write
/// path (upserted right after a successful create/update) and a periodic
/// background reconciliation pass (see `todo::monitor`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoCacheRow {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub printed_at: Option<DateTime<Local>>,
    pub due_date: Option<DateTime<Local>>,
    pub priority: u8,
    pub project_title: Option<String>,
    pub labels: Vec<String>,
    pub subtasks: Vec<CachedSubtask>,
    pub reminders: Vec<DateTime<Local>>,
    pub archived: bool,
    /// Source file's mtime at last sync — lets the background sync pass
    /// skip re-reading/re-parsing files that haven't changed since.
    pub source_mtime: Option<DateTime<Local>>,
    pub synced_at: DateTime<Local>,
    /// 0=Not Started, 1=In Progress, 2=Blocked — mirrors `todo::models::TodoStatus`'s
    /// own `as_u8`/`from_u8`; kept as a plain `u8` here since `db` doesn't
    /// depend on the `todo` crate.
    pub status: u8,
}

/// A local mirror of a `notes::models::Note`, minus its full `content` —
/// list/browse views only ever render a short preview, never the whole
/// body (confirmed against `frontend/index.html`'s note-card rendering), so
/// only a truncated `preview` is cached; opening a single note still reads
/// live. Kept in sync the same way as `TodoCacheRow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteCacheRow {
    pub notebook: String,
    /// Path of the subfolder the note lives in within `notebook`, e.g.
    /// `"Projects/Sub"` — empty string for the notebook's root. `nb_id` is
    /// only unique *within* a given (notebook, folder) pair (`nb` numbers
    /// items per-listing-scope, not per-notebook), so this is part of the
    /// row's real identity, not just display metadata.
    pub folder: String,
    pub nb_id: u64,
    pub title: String,
    pub preview: String,
    pub tags: Vec<String>,
    /// `Some(url)` for a bookmark, `None` for a regular note — see
    /// `notes::models::Note.url`.
    pub url: Option<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub source_mtime: Option<DateTime<Local>>,
    pub synced_at: DateTime<Local>,
}

/// A per-occurrence "paid" tracking row for a `finances` recurring item or
/// recurring transfer — see the `recurring_occurrence_status` table comment
/// in `db::init`. Kept independent of any `finances` types (this crate
/// doesn't depend on `finances`, and shouldn't — see `app::finances_occurrences`
/// for the module that actually joins this against real recurring items);
/// `period_start`/`paid_date` are plain ISO (`YYYY-MM-DD`) date strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringOccurrenceRow {
    pub recurring_id: String,
    /// `"item"` | `"transfer"`.
    pub kind: String,
    pub period_start: String,
    pub paid: bool,
    pub paid_date: Option<String>,
    pub updated_at: String,
}

/// A named, saved budget scenario — see the `budget_scenarios` table
/// comment in `db::init`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetScenario {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

/// One hypothetical income/expense item belonging to a `BudgetScenario`.
/// Deliberately mirrors `finances::models::PreviewItem`'s fields (this
/// crate doesn't depend on `finances`, so it's its own plain-string copy —
/// `app::finances_budget` converts between the two); `kind` is `"income"` |
/// `"expense"`, `frequency` is `"weekly"` | `"biweekly"` | `"monthly"` |
/// `"yearly"`, `reference_date` is an ISO (`YYYY-MM-DD`) date string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetScenarioItemRow {
    pub id: String,
    pub scenario_id: String,
    pub name: String,
    pub kind: String,
    pub amount: f64,
    pub frequency: String,
    pub reference_date: Option<String>,
    pub account: String,
    /// When set, applying this item's scenario auto-excludes the
    /// referenced real recurring item/transfer id from the projection —
    /// so an item representing "this payment, changed" never stacks with
    /// the real payment it stands in for. `None` for a genuinely new item.
    pub replaces_recurring_id: Option<String>,
}

/// One (category, account) budget allocation — see the
/// `budget_cap_allocations` table comment in `db::init`. A category's
/// overall budget cap is the sum of its allocations' `amount`s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCapAllocationRow {
    pub id: String,
    /// "stupid" | "survival".
    pub category: String,
    /// Which account this allocation's *projected future* spending should
    /// post against if included in a projection — a real spending entry's
    /// own account is separate and unaffected by this.
    pub account: String,
    pub amount: f64,
    /// When true (and `amount` > 0), the Overview tab folds this one
    /// allocation in as an ongoing monthly expense alongside ad-hoc
    /// preview items and applied scenarios — independent of any other
    /// allocation in the same category.
    pub include_in_projection: bool,
    pub updated_at: String,
}
