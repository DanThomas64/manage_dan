use serde::{Deserialize, Serialize};
use chrono::{DateTime, Local};

/// A single subtask, backed by a Vikunja child task + `subtask` relation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    /// Vikunja task ID. `None` for subtasks that have not yet been created.
    pub id: Option<i64>,
    pub title: String,
    pub done: bool,
}

/// Application-level representation of a todo item.
///
/// Fields map to Vikunja task fields where possible; `printed_at` is tracked
/// locally in SQLite since Vikunja has no equivalent concept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Vikunja task ID. `None` before the item has been persisted.
    pub id: Option<i64>,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    /// Last time a physical ticket was printed for this item.
    pub printed_at: Option<DateTime<Local>>,
    /// Subtasks, backed by Vikunja child tasks linked via a `subtask` relation.
    pub subtasks: Vec<Subtask>,
    /// Archived items are deleted from Vikunja; this field is kept for API
    /// compatibility only and is always `false` on items returned by the app.
    pub archived: bool,
    pub due_date: Option<DateTime<Local>>,
    /// Priority 0–10 (0 = no priority). Stored as-is in Vikunja.
    pub priority: u8,
}

impl TodoItem {
    pub fn new(title: String, description: String) -> Self {
        let now = Local::now();
        TodoItem {
            id: None,
            title,
            description,
            completed: false,
            created_at: now,
            updated_at: now,
            completed_at: None,
            printed_at: None,
            subtasks: Vec::new(),
            archived: false,
            due_date: None,
            priority: 0,
        }
    }
}
