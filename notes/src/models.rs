use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub nb_id: u64,
    pub notebook: String,
    /// Path of the subfolder this note lives in within `notebook`, e.g.
    /// `"Projects/Sub"` — empty string for the notebook's root.
    pub folder: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub title: Option<String>,
    pub content: String,
    pub tags: Option<Vec<String>>,
    pub notebook: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateLogRequest {
    pub title: String,
    pub content: String,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub date: String,
    pub time: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNoteRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub notebook: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MoveNoteRequest {
    /// Destination subfolder path within the same notebook, empty for root.
    pub dest_folder: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateFolderRequest {
    pub notebook: String,
    /// Full folder path to create, e.g. `"Projects/Sub"`.
    pub path: String,
}
