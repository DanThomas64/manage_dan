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
}

/// A local mirror of a `notes::models::Note`, minus its full `content` —
/// list/browse views only ever render a short preview, never the whole
/// body (confirmed against `frontend/index.html`'s note-card rendering), so
/// only a truncated `preview` is cached; opening a single note still reads
/// live. Kept in sync the same way as `TodoCacheRow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteCacheRow {
    pub notebook: String,
    pub nb_id: u64,
    pub title: String,
    pub preview: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub source_mtime: Option<DateTime<Local>>,
    pub synced_at: DateTime<Local>,
}
