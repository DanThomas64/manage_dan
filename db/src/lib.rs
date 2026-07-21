//! Database access layer for the application.
//!
//! Manages the SQLite database connection, schema initialization, log persistence,
//! and a lightweight `printed_tasks` table for tracking when tasks were last printed.

pub mod db_error;
pub mod db_prelude;
pub mod models;

use crate::db_error::{DbLibError, DbLibResult};
use crate::db_prelude::*;
use crate::models::{LogEntry, NoteCacheRow, TodoCacheRow};
use rusqlite::{params, OptionalExtension};
use tokio_rusqlite::Connection;
use rusqlite::{Result as RusqliteResult, Row};
use chrono::{DateTime, Local};
use std::collections::HashMap;
use tokio::sync::OnceCell;

pub const DB_FILE: &str = "app.sqlite";

// A single shared connection handle, opened lazily on first use and reused
// for the process lifetime. `tokio_rusqlite::Connection` is a cheap-clone
// handle to a channel backed by one dedicated background thread that already
// serializes all `.call()`s onto a single `rusqlite::Connection` — so this
// is a correctly-serialized "pool of one" rather than a fresh connection
// (and OS thread) opened and torn down on every single query, which is what
// happened here before.
static CONNECTION: OnceCell<Connection> = OnceCell::const_new();

async fn get_connection() -> DbLibResult<Connection> {
    let conn = CONNECTION
        .get_or_try_init(|| async {
            let conn = Connection::open(DB_FILE).await?;
            // WAL mode itself is persisted in the database file by init()'s
            // synchronous PRAGMA below; `synchronous` is per-connection, so
            // it's set here once for the one connection this process uses.
            conn.call(|c| c.execute_batch("PRAGMA synchronous = NORMAL;"))
                .await?;
            Ok::<Connection, DbLibError>(conn)
        })
        .await?;
    Ok(conn.clone())
}

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

    // Persisted in the database file itself — readers (API requests) no
    // longer block on the background monitor's writes, or vice versa.
    conn.pragma_update(None, "journal_mode", "WAL")?;

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
            task_id INTEGER PRIMARY KEY,
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
    // Migration: rename the column back from its original name (it predates
    // the removal of the Vikunja todo backend — it always just stored a
    // todo item id, named after the only backend that existed at the time).
    // Best-effort: no-op on a database that already has the new name.
    let _ = conn.execute(
        "ALTER TABLE printed_tasks RENAME COLUMN vikunja_task_id TO task_id",
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

    conn.execute(
        "CREATE TABLE IF NOT EXISTS todo_nb_index (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            folder   TEXT NOT NULL,
            local_id INTEGER NOT NULL,
            UNIQUE(folder, local_id)
        )",
        [],
    )?;

    // --- Local read cache mirroring nb-backed todos and notes ---
    // `nb` remains the source of truth; these tables let list/browse
    // views read local SQL instead of shelling out (or making HTTP calls)
    // on every request. Kept in sync by the write path (upserted right
    // after a successful create/update) and a periodic background
    // reconciliation pass — see `todo::monitor` and `notes::monitor`.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS todo_cache (
            id            INTEGER PRIMARY KEY,
            title         TEXT NOT NULL,
            description   TEXT NOT NULL,
            completed     INTEGER NOT NULL,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL,
            completed_at  TEXT,
            printed_at    TEXT,
            due_date      TEXT,
            priority      INTEGER NOT NULL,
            project_title TEXT,
            labels        TEXT NOT NULL,
            subtasks      TEXT NOT NULL,
            reminders     TEXT NOT NULL,
            archived      INTEGER NOT NULL,
            source_mtime  TEXT,
            synced_at     TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_cache_project ON todo_cache(project_title)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS note_cache (
            notebook      TEXT NOT NULL,
            nb_id         INTEGER NOT NULL,
            title         TEXT NOT NULL,
            preview       TEXT NOT NULL,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL,
            source_mtime  TEXT,
            synced_at     TEXT NOT NULL,
            PRIMARY KEY (notebook, nb_id)
        )",
        [],
    )?;

    // Normalized per-tag rows (rather than a JSON blob on note_cache) so an
    // exact-match project-tag lookup (`note_cache_get_by_tag`) is an indexed
    // query, not a scan.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS note_cache_tags (
            notebook TEXT NOT NULL,
            nb_id    INTEGER NOT NULL,
            tag      TEXT NOT NULL,
            PRIMARY KEY (notebook, nb_id, tag)
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_note_cache_tags_tag ON note_cache_tags(tag)",
        [],
    )?;

    Ok(())
}

// --- Logging ---

/// Writes a log event to the database.
pub async fn log_event(level: &str, target: &str, message: &str) -> DbLibResult {
    let conn = get_connection().await?;
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

/// Returns the printed_at timestamp for a given task ID, if recorded.
pub async fn printed_at_get(task_id: i64) -> DbLibResult<Option<DateTime<Local>>> {
    let opt_str = execute_async(move |conn| {
        conn.query_row(
            "SELECT printed_at FROM printed_tasks WHERE task_id = ?1",
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
            conn.prepare("SELECT task_id, printed_at FROM printed_tasks")?;
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

/// Records (or updates) the printed_at timestamp for a task.
/// Does not modify the stored content_hash.
pub async fn printed_at_set(task_id: i64, printed_at: DateTime<Local>) -> DbLibResult {
    let ts = printed_at.to_rfc3339();
    execute_async(move |conn| {
        conn.execute(
            "INSERT INTO printed_tasks (task_id, printed_at)
             VALUES (?1, ?2)
             ON CONFLICT(task_id) DO UPDATE SET printed_at = excluded.printed_at",
            params![task_id, ts],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error setting printed_at: {}", e)))
}

/// Returns the stored content hash for a task, if any.
pub async fn printed_hash_get(task_id: i64) -> DbLibResult<Option<String>> {
    let opt = execute_async(move |conn| {
        conn.query_row(
            "SELECT content_hash FROM printed_tasks WHERE task_id = ?1",
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
            "INSERT INTO printed_tasks (task_id, printed_at, content_hash)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id) DO UPDATE SET
               printed_at   = excluded.printed_at,
               content_hash = excluded.content_hash",
            params![task_id, ts, content_hash],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error setting printed record: {}", e)))
}

/// Atomically claims the right to print a task with the given content hash.
///
/// Returns `true` if the caller won the claim (no row existed yet, or the
/// stored hash differs) and should proceed to print; `false` if another
/// caller already recorded this exact hash. Both the creation path and the
/// background print monitor can observe a brand-new task at nearly the same
/// time — this single atomic `INSERT ... ON CONFLICT ... WHERE` statement is
/// what guarantees only one of them actually prints it.
pub async fn printed_claim(task_id: i64, content_hash: String) -> DbLibResult<bool> {
    let ts = Local::now().to_rfc3339();
    execute_async(move |conn| {
        let changed = conn.execute(
            "INSERT INTO printed_tasks (task_id, printed_at, content_hash)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id) DO UPDATE SET
               printed_at   = excluded.printed_at,
               content_hash = excluded.content_hash
             WHERE printed_tasks.content_hash IS NOT excluded.content_hash",
            params![task_id, ts, content_hash],
        )?;
        Ok(changed > 0)
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error claiming print: {}", e)))
}

/// Removes the printed_at record for a task (e.g. when the task is deleted).
pub async fn printed_at_delete(task_id: i64) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute(
            "DELETE FROM printed_tasks WHERE task_id = ?1",
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

// --- todo_nb_index ---
//
// The `nb` CLI assigns todo item IDs per-folder, not notebook-wide (two
// different project folders can each have a local id `1`). This table maps
// each `(folder, local_id)` pair to a stable, globally unique synthetic id
// so the nb-backed todo backend can hand out a stable, notebook-wide plain `i64`
// backend does.

/// Returns the synthetic id for `(folder, local_id)`, creating one if it
/// doesn't exist yet. Stable across restarts.
pub async fn todo_nb_index_get_or_create(folder: String, local_id: i64) -> DbLibResult<i64> {
    execute_async(move |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO todo_nb_index (folder, local_id) VALUES (?1, ?2)",
            params![folder, local_id],
        )?;
        conn.query_row(
            "SELECT id FROM todo_nb_index WHERE folder = ?1 AND local_id = ?2",
            params![folder, local_id],
            |row| row.get(0),
        )
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error resolving todo_nb_index: {}", e)))
}

/// Resolves a synthetic id back to its `(folder, local_id)` pair.
pub async fn todo_nb_index_resolve(id: i64) -> DbLibResult<Option<(String, i64)>> {
    execute_async(move |conn| {
        conn.query_row(
            "SELECT folder, local_id FROM todo_nb_index WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading todo_nb_index: {}", e)))
}

/// Repoints an existing synthetic id at a new `(folder, local_id)` pair —
/// used when an nb-backed todo item is deleted-and-recreated on update, so
/// the external-facing id stays stable across the operation.
pub async fn todo_nb_index_update(id: i64, folder: String, local_id: i64) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute(
            "UPDATE todo_nb_index SET folder = ?1, local_id = ?2 WHERE id = ?3",
            params![folder, local_id, id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error updating todo_nb_index: {}", e)))
}

/// Removes the index entry for a deleted todo item.
pub async fn todo_nb_index_delete(id: i64) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute("DELETE FROM todo_nb_index WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error deleting todo_nb_index: {}", e)))
}

// --- Datetime <-> TEXT column helpers, shared by the cache tables below ---

fn parse_dt(s: String) -> RusqliteResult<DateTime<Local>> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Local))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn parse_opt_dt(s: Option<String>) -> RusqliteResult<Option<DateTime<Local>>> {
    s.map(parse_dt).transpose()
}

// --- todo_cache ---
//
// A local mirror of the fully-hydrated todo items `nb` otherwise
// have to be asked for on every read. `nb` stays the source of
// truth; this table is upserted right after a successful write and
// reconciled on a timer by `todo::monitor`'s background sync pass.

fn row_to_todo_cache_row(row: &Row) -> RusqliteResult<TodoCacheRow> {
    let labels_json: String = row.get(11)?;
    let subtasks_json: String = row.get(12)?;
    let reminders_json: String = row.get(13)?;
    Ok(TodoCacheRow {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        completed: row.get::<_, i64>(3)? != 0,
        created_at: parse_dt(row.get(4)?)?,
        updated_at: parse_dt(row.get(5)?)?,
        completed_at: parse_opt_dt(row.get(6)?)?,
        printed_at: parse_opt_dt(row.get(7)?)?,
        due_date: parse_opt_dt(row.get(8)?)?,
        priority: row.get::<_, i64>(9)? as u8,
        project_title: row.get(10)?,
        labels: serde_json::from_str(&labels_json).unwrap_or_default(),
        subtasks: serde_json::from_str(&subtasks_json).unwrap_or_default(),
        reminders: serde_json::from_str(&reminders_json).unwrap_or_default(),
        archived: row.get::<_, i64>(14)? != 0,
        source_mtime: parse_opt_dt(row.get(15)?)?,
        synced_at: parse_dt(row.get(16)?)?,
    })
}

const TODO_CACHE_COLUMNS: &str = "id, title, description, completed, created_at, updated_at,
     completed_at, printed_at, due_date, priority, project_title,
     labels, subtasks, reminders, archived, source_mtime, synced_at";

/// Inserts or fully replaces the cached row for a todo item — called by the
/// write-path dispatcher right after a successful create/update/complete
/// against `nb`, and by the background sync pass for items whose
/// source has changed since the last sync.
pub async fn todo_cache_upsert(row: TodoCacheRow) -> DbLibResult {
    execute_async(move |conn| {
        let labels_json = serde_json::to_string(&row.labels).unwrap_or_else(|_| "[]".to_string());
        let subtasks_json = serde_json::to_string(&row.subtasks).unwrap_or_else(|_| "[]".to_string());
        let reminders_json = serde_json::to_string(&row.reminders).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO todo_cache (
                id, title, description, completed, created_at, updated_at,
                completed_at, printed_at, due_date, priority, project_title,
                labels, subtasks, reminders, archived, source_mtime, synced_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                description = excluded.description,
                completed = excluded.completed,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at,
                printed_at = excluded.printed_at,
                due_date = excluded.due_date,
                priority = excluded.priority,
                project_title = excluded.project_title,
                labels = excluded.labels,
                subtasks = excluded.subtasks,
                reminders = excluded.reminders,
                archived = excluded.archived,
                source_mtime = excluded.source_mtime,
                synced_at = excluded.synced_at",
            params![
                row.id,
                row.title,
                row.description,
                row.completed as i64,
                row.created_at.to_rfc3339(),
                row.updated_at.to_rfc3339(),
                row.completed_at.map(|d| d.to_rfc3339()),
                row.printed_at.map(|d| d.to_rfc3339()),
                row.due_date.map(|d| d.to_rfc3339()),
                row.priority as i64,
                row.project_title,
                labels_json,
                subtasks_json,
                reminders_json,
                row.archived as i64,
                row.source_mtime.map(|d| d.to_rfc3339()),
                row.synced_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error upserting todo_cache: {}", e)))
}

/// Returns every cached todo — the read path `todo::read_items()` uses once
/// the cache is live, instead of a live `nb` fetch.
pub async fn todo_cache_get_all() -> DbLibResult<Vec<TodoCacheRow>> {
    execute_async(|conn| {
        let sql = format!("SELECT {} FROM todo_cache", TODO_CACHE_COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let rows: RusqliteResult<Vec<TodoCacheRow>> = stmt.query_map([], row_to_todo_cache_row)?.collect();
        rows
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading todo_cache: {}", e)))
}

pub async fn todo_cache_get(id: i64) -> DbLibResult<Option<TodoCacheRow>> {
    execute_async(move |conn| {
        let sql = format!("SELECT {} FROM todo_cache WHERE id = ?1", TODO_CACHE_COLUMNS);
        conn.query_row(&sql, params![id], row_to_todo_cache_row).optional()
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading todo_cache row: {}", e)))
}

/// Returns every cached todo belonging to `project_title` — a real indexed
/// SQL filter (see `idx_todo_cache_project`), replacing the old
/// fetch-everything-then-filter-in-Rust `project::project_todos` used.
pub async fn todo_cache_get_by_project(project_title: String) -> DbLibResult<Vec<TodoCacheRow>> {
    execute_async(move |conn| {
        let sql = format!("SELECT {} FROM todo_cache WHERE project_title = ?1", TODO_CACHE_COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let rows: RusqliteResult<Vec<TodoCacheRow>> =
            stmt.query_map(params![project_title], row_to_todo_cache_row)?.collect();
        rows
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading todo_cache by project: {}", e)))
}

pub async fn todo_cache_delete(id: i64) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute("DELETE FROM todo_cache WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error deleting todo_cache row: {}", e)))
}

/// Returns every cached todo id — used by the background sync pass to
/// detect items removed externally (deleted via the raw `nb` CLI, outside
/// this app) since the last sync, by diffing against a freshly-listed id
/// set and deleting whatever's missing from it.
pub async fn todo_cache_get_ids() -> DbLibResult<Vec<i64>> {
    execute_async(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM todo_cache")?;
        let rows: RusqliteResult<Vec<i64>> = stmt.query_map([], |row| row.get(0))?.collect();
        rows
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading todo_cache ids: {}", e)))
}

/// Returns the stored source-file mtime for a cached todo, if any — lets the
/// background sync pass (`nb` backend only) skip re-reading and re-parsing
/// an item whose file hasn't changed since the last sync, so sync cost
/// scales with how much changed, not with how many items exist in total.
pub async fn todo_cache_get_source_mtime(id: i64) -> DbLibResult<Option<DateTime<Local>>> {
    let opt = execute_async(move |conn| {
        conn.query_row(
            "SELECT source_mtime FROM todo_cache WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading todo_cache source_mtime: {}", e)))?;

    Ok(opt
        .flatten()
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Local))))
}

// --- note_cache / note_cache_tags ---
//
// A local mirror of note list/browse metadata (title, tag set, a truncated
// content preview — never the full body, see `NoteCacheRow`'s doc comment).
// Tags live in a separate normalized table so an exact-tag lookup
// (`note_cache_get_by_tag`, what project-note scoping needs) is an indexed
// join, not a scan over a JSON blob.

fn row_to_note_cache_row(row: &Row) -> RusqliteResult<NoteCacheRow> {
    Ok(NoteCacheRow {
        notebook: row.get(0)?,
        nb_id: row.get::<_, i64>(1)? as u64,
        title: row.get(2)?,
        preview: row.get(3)?,
        tags: Vec::new(), // filled in by attach_tags()
        created_at: parse_dt(row.get(4)?)?,
        updated_at: parse_dt(row.get(5)?)?,
        source_mtime: parse_opt_dt(row.get(6)?)?,
        synced_at: parse_dt(row.get(7)?)?,
    })
}

const NOTE_CACHE_COLUMNS: &str = "notebook, nb_id, title, preview, created_at, updated_at, source_mtime, synced_at";

/// Batches in every row's tags with two queries total (one for the notes
/// already fetched, one for all tags), rather than one extra query per note.
fn attach_tags(conn: &rusqlite::Connection, rows: &mut [NoteCacheRow]) -> RusqliteResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare("SELECT notebook, nb_id, tag FROM note_cache_tags")?;
    let tag_rows: Vec<(String, i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<RusqliteResult<_>>()?;

    let mut by_key: HashMap<(String, i64), Vec<String>> = HashMap::new();
    for (nb, id, tag) in tag_rows {
        by_key.entry((nb, id)).or_default().push(tag);
    }
    for row in rows.iter_mut() {
        if let Some(tags) = by_key.remove(&(row.notebook.clone(), row.nb_id as i64)) {
            row.tags = tags;
        }
    }
    Ok(())
}

/// Inserts or fully replaces the cached row (and tag set) for a note —
/// called by the write-path dispatcher right after a successful
/// create/update against `nb`, and by the background sync pass.
pub async fn note_cache_upsert(row: NoteCacheRow) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute(
            "INSERT INTO note_cache (
                notebook, nb_id, title, preview, created_at, updated_at, source_mtime, synced_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
            ON CONFLICT(notebook, nb_id) DO UPDATE SET
                title = excluded.title,
                preview = excluded.preview,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                source_mtime = excluded.source_mtime,
                synced_at = excluded.synced_at",
            params![
                row.notebook,
                row.nb_id as i64,
                row.title,
                row.preview,
                row.created_at.to_rfc3339(),
                row.updated_at.to_rfc3339(),
                row.source_mtime.map(|d| d.to_rfc3339()),
                row.synced_at.to_rfc3339(),
            ],
        )?;

        conn.execute(
            "DELETE FROM note_cache_tags WHERE notebook = ?1 AND nb_id = ?2",
            params![row.notebook, row.nb_id as i64],
        )?;
        for tag in &row.tags {
            conn.execute(
                "INSERT OR IGNORE INTO note_cache_tags (notebook, nb_id, tag) VALUES (?1, ?2, ?3)",
                params![row.notebook, row.nb_id as i64, tag],
            )?;
        }
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error upserting note_cache: {}", e)))
}

pub async fn note_cache_get_all() -> DbLibResult<Vec<NoteCacheRow>> {
    execute_async(|conn| {
        let sql = format!("SELECT {} FROM note_cache", NOTE_CACHE_COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows: Vec<NoteCacheRow> =
            stmt.query_map([], row_to_note_cache_row)?.collect::<RusqliteResult<_>>()?;
        attach_tags(conn, &mut rows)?;
        Ok(rows)
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading note_cache: {}", e)))
}

pub async fn note_cache_get_by_notebook(notebook: String) -> DbLibResult<Vec<NoteCacheRow>> {
    execute_async(move |conn| {
        let sql = format!("SELECT {} FROM note_cache WHERE notebook = ?1", NOTE_CACHE_COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows: Vec<NoteCacheRow> =
            stmt.query_map(params![notebook], row_to_note_cache_row)?.collect::<RusqliteResult<_>>()?;
        attach_tags(conn, &mut rows)?;
        Ok(rows)
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading note_cache by notebook: {}", e)))
}

/// Returns every cached note tagged with `tag` — a real indexed join
/// against `note_cache_tags` (see `idx_note_cache_tags_tag`), replacing the
/// old fetch-everything-then-filter-in-Rust `project::project_notes` used.
pub async fn note_cache_get_by_tag(tag: String) -> DbLibResult<Vec<NoteCacheRow>> {
    execute_async(move |conn| {
        let sql = format!(
            "SELECT {} FROM note_cache nc
             JOIN note_cache_tags nct ON nct.notebook = nc.notebook AND nct.nb_id = nc.nb_id
             WHERE nct.tag = ?1",
            NOTE_CACHE_COLUMNS.split(',').map(|c| format!("nc.{}", c.trim())).collect::<Vec<_>>().join(", ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows: Vec<NoteCacheRow> =
            stmt.query_map(params![tag], row_to_note_cache_row)?.collect::<RusqliteResult<_>>()?;
        attach_tags(conn, &mut rows)?;
        Ok(rows)
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading note_cache by tag: {}", e)))
}

pub async fn note_cache_delete(notebook: String, nb_id: u64) -> DbLibResult {
    execute_async(move |conn| {
        conn.execute(
            "DELETE FROM note_cache WHERE notebook = ?1 AND nb_id = ?2",
            params![notebook, nb_id as i64],
        )?;
        conn.execute(
            "DELETE FROM note_cache_tags WHERE notebook = ?1 AND nb_id = ?2",
            params![notebook, nb_id as i64],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error deleting note_cache row: {}", e)))
}

/// Returns every cached `(notebook, nb_id)` key, optionally scoped to one
/// notebook — used by the background sync pass to detect notes removed
/// externally since the last sync, the same way `todo_cache_get_ids` does.
pub async fn note_cache_get_keys(notebook: Option<String>) -> DbLibResult<Vec<(String, u64)>> {
    execute_async(move |conn| {
        let rows: Vec<(String, i64)> = if let Some(nb) = &notebook {
            let mut stmt = conn.prepare("SELECT notebook, nb_id FROM note_cache WHERE notebook = ?1")?;
            let result: RusqliteResult<Vec<(String, i64)>> =
                stmt.query_map(params![nb], |r| Ok((r.get(0)?, r.get(1)?)))?.collect();
            result?
        } else {
            let mut stmt = conn.prepare("SELECT notebook, nb_id FROM note_cache")?;
            let result: RusqliteResult<Vec<(String, i64)>> =
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect();
            result?
        };
        Ok(rows.into_iter().map(|(nb, id)| (nb, id as u64)).collect())
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading note_cache keys: {}", e)))
}

/// Returns the stored source-file mtime for a cached note, if any — same
/// skip-unchanged use as `todo_cache_get_source_mtime`.
pub async fn note_cache_get_source_mtime(notebook: String, nb_id: u64) -> DbLibResult<Option<DateTime<Local>>> {
    let opt = execute_async(move |conn| {
        conn.query_row(
            "SELECT source_mtime FROM note_cache WHERE notebook = ?1 AND nb_id = ?2",
            params![notebook, nb_id as i64],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
    })
    .await
    .map_err(|e| DbLibError::Internal(format!("DB error reading note_cache source_mtime: {}", e)))?;

    Ok(opt
        .flatten()
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Local))))
}

// --- Generic helper ---

pub async fn execute_async<F, T>(f: F) -> DbLibResult<T>
where
    F: FnOnce(&mut rusqlite::Connection) -> RusqliteResult<T> + Send + 'static,
    T: Send + 'static,
{
    let conn = get_connection().await?;
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
