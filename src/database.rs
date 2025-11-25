use crate::datatypes::Task;
use anyhow::Result;
use rusqlite::{params, Connection};

pub fn init_db(db_path: &str) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS printed_tasks (
            id INTEGER PRIMARY KEY,
            updated TEXT NOT NULL
        )",
        [],
    )?;
    Ok(conn)
}

pub fn needs_printing(conn: &Connection, task: &Task) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT updated FROM printed_tasks WHERE id = ?1")?;
    let mut rows = stmt.query(params![task.id])?;

    if let Some(row) = rows.next()? {
        let last_updated: String = row.get(0)?;
        Ok(last_updated != task.updated)
    } else {
        Ok(true) // Not in DB, so it's new and needs printing
    }
}

pub fn mark_as_printed(conn: &Connection, task: &Task) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO printed_tasks (id, updated) VALUES (?1, ?2)",
        params![task.id, task.updated],
    )?;
    Ok(())
}
