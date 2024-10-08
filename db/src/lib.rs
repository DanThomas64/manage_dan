pub mod db_error;
pub mod db_prelude;

use crate::db_prelude::*;
use tokio_rusqlite::Connection;

static DB_FILE: &str = "app.sqlite";

/// Initializes the database connection and ensures the log table exists.
pub fn init() -> DbLibResult {
    info!("initializing db");

    // Use synchronous rusqlite for schema setup during synchronous initialization phase
    let conn = rusqlite::Connection::open(DB_FILE)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS log (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            level TEXT NOT NULL,
            target TEXT NOT NULL,
            message TEXT NOT NULL
        )",
        [],
    )?;

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
            rusqlite::params![timestamp, level, target, message],
        )?;
        Ok(())
    })
    .await?;

    Ok(())
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
