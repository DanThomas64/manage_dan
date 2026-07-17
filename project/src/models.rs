//! Data models for the Project subsystem.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// A named project, grouping a todo scope, notes, lists, and log entries
/// under one umbrella, plus a filesystem folder for code/reference files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    /// Kebab-case, unique, auto-derived from `name`. Also the nb todo-folder
    /// name and the filesystem directory name under the project base dir.
    pub slug: String,
    /// `project-<slug>` — the tag used to scope notes and log entries.
    pub tag: String,
    /// FK into `lists::shopping_list_groups`.
    pub list_group_id: i64,
    pub fs_path: String,
    pub archived_at: Option<DateTime<Local>>,
    pub created_at: DateTime<Local>,
}

/// Aggregated view of everything scoped to one project. `todos`/`notes`/
/// `logs`/`lists` are left empty once the project is archived — archived
/// projects show static metadata only, no live re-fetch of moved content.
///
/// Serialize-only: this is an outgoing API DTO (`notes::models::LogEntry`
/// doesn't implement `Deserialize`, so this type can't derive it either).
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetail {
    pub project: Project,
    pub todos: Vec<todo::models::TodoItem>,
    pub notes: Vec<notes::models::Note>,
    pub logs: Vec<notes::models::LogEntry>,
    pub lists: Vec<lists::models::ListCategory>,
}
