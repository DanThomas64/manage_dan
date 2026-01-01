pub mod db_error;
pub mod db_prelude;
pub mod models; // New module for TodoItem
pub mod todo_error; // New module for TodoLibError

use crate::db_error::DbLibResult;
use crate::db_prelude::*;
use crate::models::TodoItem; // Import TodoItem from local module
use crate::todo_error::{TodoLibError, TodoLibResult}; // Import Todo types from local module
use rusqlite::params;
use tokio_rusqlite::{Connection, Error as TokioSqliteError};
use rusqlite::{Result as RusqliteResult, Row};
use chrono::{DateTime, Local, TimeZone}; // Import chrono types

// Define DB file location (assuming it's a constant used internally)
const DB_FILE: &str = "app.sqlite";

/// Helper function to convert a database row into a TodoItem.
fn row_to_todo_item(row: &Row) -> RusqliteResult<TodoItem> {
    // Helper to parse RFC3339 string into DateTime<Local>
    let parse_datetime = |s: String| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Local))
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
    };

    let completed_at_str: Option<String> = row.get(5)?;
    let completed_at = completed_at_str
        .map(parse_datetime)
        .transpose()?;
        
    // Read printed_at (Index 7)
    let printed_at_str: Option<String> = row.get(7)?;
    let printed_at = printed_at_str
        .map(parse_datetime)
        .transpose()?;
        
    // Read subtasks (Index 8)
    let subtasks: Option<String> = row.get(8)?;
    
    // NEW: Read archived (Index 9)
    let archived: bool = row.get(9)?;

    Ok(TodoItem {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?, // Now required String
        completed: row.get(3)?,
        created_at: row.get::<_, String>(4).and_then(parse_datetime)?,
        updated_at: row.get::<_, String>(6).and_then(parse_datetime)?,
        completed_at,
        printed_at,
        subtasks,
        archived, // NEW
    })
}

/// Initializes the database connection and ensures the log and todo tables exist.
pub fn init() -> DbLibResult {
    info!("initializing db");

    // Use synchronous rusqlite for schema setup during synchronous initialization phase
    let conn = rusqlite::Connection::open(DB_FILE)?;

    // 1. Create log table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS log (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            level TEXT NOT NULL,
            target TEXT NOT NULL,
            message TEXT TEXT NOT NULL
        )",
        [],
    )?;

    // 2. Create todo table, including new timestamp fields
    // NOTE: description is NOT NULL, subtasks is added, archived is added.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS todo (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            description TEXT NOT NULL,
            completed INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            completed_at TEXT,
            updated_at TEXT NOT NULL,
            printed_at TEXT,
            subtasks TEXT,
            archived INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;
    
    // 2b. Simple migration: Add printed_at column if it doesn't exist in an older database.
    let _ = conn.execute("ALTER TABLE todo ADD COLUMN printed_at TEXT", []);
    
    // 2c. Simple migration: Add subtasks column if it doesn't exist.
    let _ = conn.execute("ALTER TABLE todo ADD COLUMN subtasks TEXT", []);

    // 2d. Simple migration: Add archived column if it doesn't exist.
    let _ = conn.execute("ALTER TABLE todo ADD COLUMN archived INTEGER NOT NULL DEFAULT 0", []);

    Ok(())
}

/// Logs a significant event to the database asynchronously.
pub async fn log_event(level: &str, target: &str, message: &str) -> DbLibResult {
    let conn = Connection::open(DB_FILE).await?;
    let timestamp = chrono::Local::now().to_rfc3339();

    // Convert borrowed slices to owned Strings so they can be moved into the 'static closure
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
    .map_err(|e| e.into()) // Convert TokioSqliteError to DbLibError
}

/// Executes a synchronous database operation asynchronously on the Tokio thread pool.
/// This is a generic helper for other modules (like todo) to interact with the DB.
pub async fn execute_async<F, T>(f: F) -> DbLibResult<T>
where
    // F must return Result<T, rusqlite::Error>
    F: FnOnce(&mut rusqlite::Connection) -> RusqliteResult<T> + Send + 'static,
    T: Send + 'static,
{
    let conn = Connection::open(DB_FILE).await?;
    
    // conn.call(f) returns Result<T, tokio_rusqlite::Error>
    conn.call(f).await.map_err(|e| e.into())
}

// --- Todo CRUD Logic (Moved from todo/src/lib.rs) ---

/// Creates a new TodoItem in the database. Returns the inserted item with its ID.
pub async fn todo_create(item: TodoItem) -> TodoLibResult<TodoItem> {
    let now = Local::now().to_rfc3339();
    
    execute_async(move |conn: &mut rusqlite::Connection| {
        conn.execute(
            "INSERT INTO todo (title, description, completed, created_at, completed_at, updated_at, printed_at, subtasks, archived) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                item.title, 
                item.description, 
                item.completed, 
                now, // created_at
                item.completed_at.map(|dt| dt.to_rfc3339()), // completed_at (None)
                now, // updated_at
                item.printed_at.map(|dt| dt.to_rfc3339()), // printed_at
                item.subtasks, 
                item.archived, // NEW
            ],
        )?;
        let id = conn.last_insert_rowid();
        
        // Retrieve the newly inserted item (must select 10 columns now)
        let mut stmt = conn.prepare("SELECT id, title, description, completed, created_at, completed_at, updated_at, printed_at, subtasks, archived FROM todo WHERE id = ?1")?;
        let new_item = stmt.query_row(params![id], row_to_todo_item)?;
        
        Ok(new_item)
    }).await.map_err(|e| TodoLibError::CannotInitialize(format!("DB error during creation: {}", e)))
}

/// Reads a single TodoItem by ID.
pub async fn todo_read_one(id: i64) -> TodoLibResult<TodoItem> {
    execute_async(move |conn: &mut rusqlite::Connection| {
        // Must select 10 columns now
        let mut stmt = conn.prepare("SELECT id, title, description, completed, created_at, completed_at, updated_at, printed_at, subtasks, archived FROM todo WHERE id = ?1")?;
        let item = stmt.query_row(params![id], row_to_todo_item)?;
        Ok(item)
    }).await.map_err(|e| TodoLibError::CannotInitialize(format!("DB error during read: {}", e)))
}

/// Reads all TodoItems from the database.
/// If `include_archived` is false, only non-archived items are returned.
pub async fn todo_read_all(include_archived: bool) -> TodoLibResult<Vec<TodoItem>> {
    execute_async(move |conn: &mut rusqlite::Connection| {
        let query = if include_archived {
            "SELECT id, title, description, completed, created_at, completed_at, updated_at, printed_at, subtasks, archived FROM todo ORDER BY id ASC"
        } else {
            "SELECT id, title, description, completed, created_at, completed_at, updated_at, printed_at, subtasks, archived FROM todo WHERE archived = 0 ORDER BY id ASC"
        };
        
        let mut stmt = conn.prepare(query)?;
        let item_iter = stmt.query_map(params![], row_to_todo_item)?;
        
        let items: RusqliteResult<Vec<TodoItem>> = item_iter.collect();
        items
    }).await.map_err(|e| TodoLibError::CannotInitialize(format!("DB error during read: {}", e)))
}

/// Updates an existing TodoItem in the database.
pub async fn todo_update(item: TodoItem) -> TodoLibResult {
    let id = item.id.ok_or_else(|| TodoLibError::Unknown)?;
    let now = Local::now().to_rfc3339();
    
    // Determine completed_at status based on item.completed flag
    let completed_at_value = if item.completed {
        // Use the value provided in `item.completed_at` if present,
        // otherwise we set it to `now` if `item.completed` is true.
        item.completed_at.map(|dt| dt.to_rfc3339()).or(Some(now.clone()))
    } else {
        // If completed is false, clear completed_at
        None
    };

    execute_async(move |conn: &mut rusqlite::Connection| {
        let rows_affected = conn.execute(
            "UPDATE todo SET 
                title = ?1, 
                description = ?2, 
                completed = ?3, 
                completed_at = ?4, 
                updated_at = ?5,
                printed_at = ?6,
                subtasks = ?7,
                archived = ?8
             WHERE id = ?9",
            params![
                item.title, 
                item.description, 
                item.completed, 
                completed_at_value, // ?4
                now, // updated_at ?5 (DB sets this explicitly)
                item.printed_at.map(|dt| dt.to_rfc3339()), // printed_at ?6
                item.subtasks, // ?7
                item.archived, // NEW ?8
                id // ?9
            ],
        )?;
        if rows_affected == 0 {
            // Note: We rely on the caller (todo module) to handle logging if needed.
        }
        Ok(())
    }).await.map_err(|e| TodoLibError::CannotInitialize(format!("DB error during update: {}", e)))
}

/// Deletes a TodoItem by ID.
pub async fn todo_delete(id: i64) -> TodoLibResult {
    execute_async(move |conn: &mut rusqlite::Connection| {
        let rows_affected = conn.execute("DELETE FROM todo WHERE id = ?1", params![id])?;
        if rows_affected == 0 {
            // Note: We rely on the caller (todo module) to handle logging if needed.
        }
        Ok(())
    }).await.map_err(|e| TodoLibError::CannotInitialize(format!("DB error during delete: {}", e)))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        // Note: Testing DB initialization requires handling the file system interaction.
        // For simplicity, we rely on the synchronous nature of `rusqlite::Connection::open`
        // and schema creation.
        let result = init();
        assert!(result.is_ok());
    }
}
