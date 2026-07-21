use serde::{Deserialize, Serialize};
use chrono::{DateTime, Local};

/// A single subtask, backed by an nb task line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    /// Todo item id. `None` for subtasks that have not yet been created.
    pub id: Option<i64>,
    pub title: String,
    pub done: bool,
}

/// Application-level representation of a todo item.
///
/// `printed_at` is tracked locally in SQLite since `nb` has no equivalent
/// concept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Todo item id. `None` before the item has been persisted.
    pub id: Option<i64>,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    /// Last time a physical ticket was printed for this item.
    pub printed_at: Option<DateTime<Local>>,
    /// Subtasks, backed by nb task lines.
    pub subtasks: Vec<Subtask>,
    /// Archived items are deleted; this field is kept for API compatibility
    /// only and is always `false` on items returned by the app.
    pub archived: bool,
    pub due_date: Option<DateTime<Local>>,
    /// Priority 0–5 (0=Unset, 1=Low, 2=Medium, 3=High, 4=Urgent, 5=Do Now).
    pub priority: u8,
    /// Name of the project this task belongs to.
    #[serde(default)]
    pub project_title: Option<String>,
    /// Label titles attached to this task.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Reminder datetimes set on this task.
    #[serde(default)]
    pub reminders: Vec<DateTime<Local>>,
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
            project_title: None,
            labels: Vec::new(),
            reminders: Vec::new(),
        }
    }
}
