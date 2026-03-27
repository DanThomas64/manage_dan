//! Database access layer for the application.
//!
//! Manages the SQLite database connection, schema initialization, log persistence,
//! and a lightweight `printed_tasks` table for tracking when tasks were last printed.

pub mod db_error;
pub mod db_prelude;
pub mod models;

use crate::db_error::{DbLibError, DbLibResult};
use crate::db_prelude::*;
use crate::models::LogEntry;
use rusqlite::{params, OptionalExtension};
use tokio_rusqlite::Connection;
use rusqlite::{Result as RusqliteResult, Row};
use chrono::{DateTime, Local};
use std::collections::HashMap;

pub const DB_FILE: &str = "app.sqlite";

/// Helper to convert a database row into a LogEntry.
fn row_to_log_entry(row: &Row) -> RusqliteResult<LogEntry> {
    let parse_datetime = |s: String| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Local))
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                0, rusqlite::types::Type::Text, Box::new(e),
            ))
    };
    Ok(LogEntry {
        id: row.get(0)?,
        timestamp: row.get::<_, String>(1).and_then(parse_datetime)?,
        level: row.get(2)?,
        target: row.get(3)?,
        message: row.get(4)?,
    })
}

/// Initializes the database and ensures all required tables exist.
pub fn init() -> DbLibResult {
    info!("initializing db");
    let conn = rusqlite::Connection::open(DB_FILE)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS log (
            id        INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            level     TEXT NOT NULL,
            target    TEXT NOT NULL,
            message   TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS printed_tasks (
            vikunja_task_id INTEGER PRIMARY KEY,
            printed_at      TEXT NOT NULL,
            content_hash    TEXT
        )",
        [],
    )?;
    // Migration: add content_hash to existing databases.
    let _ = conn.execute(
        "ALTER TABLE printed_tasks ADD COLUMN content_hash TEXT",
        [],
    );

    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS recurring_printed (
            date       TEXT NOT NULL,
            task_title TEXT NOT NULL,
            PRIMARY KEY (date, task_title)
        )",
        [],
    )?;

    Ok(())
}

// --- Logging ---

/// Writes a log event to the database.
pub async fn log_event(level: &str, target: &str, message: &str) -> DbLibResult {
    let conn = Connection::open(DB_FILE).await?;
    let timestamp = chrono::Local::now().to_rfc3339();
    let level = level.to_string();
    let target = target.to_string();
    let message = message.to_string();

    conn.call(move |conn| {
        conn.execute(
            "INSERT INTO log (timestamp, level, target, message) VALUES (?1, ?2, ?3, ?4)",
            params![timestamp, level, target, message],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| e.into())
}

/// Reads the latest N log entries.
pub async fn log_read_latest(limit: u32) -> DbLibResult<Vec<LogEntry>> {
    execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, level, target, message FROM log ORDER BY id DESC LIMIT ?1",
        )?;
        let entries: RusqliteResult<Vec<LogEntry>> =
            stmt.query_map(params![limit], row_to_log_entry)?.collect();
        entries
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading logs: {}", e)))
}

// --- printed_tasks ---

/// Returns the printed_at timestamp for a given Vikunja task ID, if recorded.
pub async fn printed_at_get(task_id: i64) -> DbLibResult<Option<DateTime<Local>>> {
    let opt_str = execute_async(move |conn| {
        conn.query_row(
            "SELECT printed_at FROM printed_tasks WHERE vikunja_task_id = ?1",
            params![task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading printed_at: {}", e)))?;

    Ok(opt_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|dt| dt.with_timezone(&Local))
    }))
}

/// Returns printed_at timestamps for all tracked task IDs.
pub async fn printed_at_get_all() -> DbLibResult<HashMap<i64, DateTime<Local>>> {
    execute_async(move |conn| {
        let mut stmt =
            conn.prepare("SELECT vikunja_task_id, printed_at FROM printed_tasks")?;
        let rows: RusqliteResult<Vec<(i64, String)>> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect();
        rows
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading printed_at_all: {}", e)))
    .map(|rows| {
        rows.into_iter()
            .filter_map(|(id, s)| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| (id, dt.with_timezone(&Local)))
            })
            .collect()
    })
}

/// Records (or updates) the printed_at timestamp for a Vikunja task.
/// Does not modify the stored content_hash.
pub async fn printed_at_set(task_id: i64, printed_at: DateTime<Local>) -> DbLibResult {
    let ts = printed_at.to_rfc3339();
    execute_async(move |conn| {
        conn.execute(
            "INSERT INTO printed_tasks (vikunja_task_id, printed_at)
             VALUES (?1, ?2)
             ON CONFLICT(vikunja_task_id) DO UPDATE SET printed_at = excluded.printed_at",
            params![task_id, ts],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error setting printed_at: {}", e)))
}

/// Returns the stored content hash for a Vikunja task, if any.
pub async fn printed_hash_get(task_id: i64) -> DbLibResult<Option<String>> {
    let opt = execute_async(move |conn| {
        conn.query_row(
            "SELECT content_hash FROM printed_tasks WHERE vikunja_task_id = ?1",
            params![task_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading content_hash: {}", e)))?;

    Ok(opt.flatten())
}

/// Records (or updates) the printed_at timestamp AND content hash together.
/// Used by the print monitor after successfully printing a task.
pub async fn printed_record_set(
    task_id: i64,
    printed_at: DateTime<Local>,
    content_hash: String,
) -> DbLibResult {
    let ts = printed_at.to_rfc3339();
    execute_async(move |conn| {
        conn.execute(
            "INSERT INTO printed_tasks (vikunja_task_id, printed_at, content_hash)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(vikunja_task_id) DO UPDATE SET
               printed_at   = excluded.printed_at,
               content_hash = excluded.content_hash",
            params![task_id, ts, content_hash],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error setting printed record: {}", e)))
}

/// Removes the printed_at record for a Vikunja task (e.g. when the task is deleted).
pub async fn printed_at_delete(task_id: i64) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute(
            "DELETE FROM printed_tasks WHERE vikunja_task_id = ?1",
            params![task_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error deleting printed_at: {}", e)))
}

// --- Settings ---

/// Reads a settings value by key.
pub async fn setting_get(key: &'static str) -> DbLibResult<Option<String>> {
    execute_async(move |conn| {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading setting '{}': {}", key, e)))
}

/// Writes (upserts) a settings value.
pub async fn setting_set(key: &'static str, value: String) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error writing setting '{}': {}", key, e)))
}

// --- recurring_printed ---

/// Returns true if the given recurring task has already been printed on the given date.
pub async fn recurring_printed_check(date: String, task_title: String) -> DbLibResult<bool> {
    execute_async(move |conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM recurring_printed WHERE date = ?1 AND task_title = ?2",
            params![date, task_title],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error checking recurring_printed: {}", e)))
}

/// Records that the given recurring task was printed on the given date.
pub async fn recurring_printed_record(date: String, task_title: String) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO recurring_printed (date, task_title) VALUES (?1, ?2)",
            params![date, task_title],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error recording recurring_printed: {}", e)))
}

// --- Generic helper ---

pub async fn execute_async<F, T>(f: F) -> DbLibResult<T>
where
    F: FnOnce(&mut rusqlite::Connection) -> RusqliteResult<T> + Send + 'static,
    T: Send + 'static,
{
    let conn = Connection::open(DB_FILE).await?;
    conn.call(f).await.map_err(|e| e.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = init();
        assert!(result.is_ok());
    }
}
