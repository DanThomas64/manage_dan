use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NoteStatus {
    Raw,
    Note,
    Article,
}

impl NoteStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NoteStatus::Raw => "raw",
            NoteStatus::Note => "note",
            NoteStatus::Article => "article",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "note" => NoteStatus::Note,
            "article" => NoteStatus::Article,
            _ => NoteStatus::Raw,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: Option<i64>,
    pub uuid: String,
    pub title: String,
    pub content: String,
    pub status: NoteStatus,
    pub tags: Vec<String>,
    pub folder: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub title: Option<String>,
    pub content: String,
    pub tags: Option<Vec<String>>,
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNoteRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub status: Option<NoteStatus>,
    pub tags: Option<Vec<String>>,
    pub folder: Option<String>,
}
