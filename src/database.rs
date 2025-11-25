use crate::datatypes::Task;
use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};

pub fn init_db(db_path: &str) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS printed_tasks (
            id INTEGER PRIMARY KEY,
            updated TEXT NOT NULL,
            last_printed TEXT
        )",
        [],
    )?;
    // Simple migration to add the column if it doesn't exist.
    // We ignore the result because this will fail if the column already exists.
    conn.execute("ALTER TABLE printed_tasks ADD COLUMN last_printed TEXT", [])
        .ok();
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
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO printed_tasks (id, updated, last_printed) VALUES (?1, ?2, ?3)",
        params![task.id, task.updated, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatypes::{Label, Task};

    fn create_test_task(id: i32, updated: &str) -> Task {
        Task {
            id,
            title: "Test Task".to_string(),
            description: "Test Description".to_string(),
            updated: updated.to_string(),
            done: false,
            labels: Some(vec![Label {
                title: "Test".to_string(),
            }]),
            project_id: 1,
            due_date: "2025-01-02T12:00:00Z".to_string(),
            reminders: None,
        }
    }

    #[test]
    fn test_database_logic() -> Result<()> {
        let conn = init_db(":memory:")?;

        // 1. New task should need printing
        let task_v1 = create_test_task(1, "2025-01-01T12:00:00Z");
        assert!(needs_printing(&conn, &task_v1)?);

        // 2. Mark as printed
        mark_as_printed(&conn, &task_v1)?;

        // 3. Same task should not need printing again
        assert!(!needs_printing(&conn, &task_v1)?);

        // 4. An updated task should need printing
        let task_v2 = create_test_task(1, "2025-01-01T13:00:00Z");
        assert!(needs_printing(&conn, &task_v2)?);

        // 5. Mark updated task as printed
        mark_as_printed(&conn, &task_v2)?;
        assert!(!needs_printing(&conn, &task_v2)?);

        Ok(())
    }
}
