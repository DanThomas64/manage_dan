use serde::{Deserialize, Serialize};
use chrono::{DateTime, Local};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: Option<i64>,
    pub title: String,
    pub description: String, // Changed from Option<String> to String (Required)
    pub completed: bool,
    
    // New timestamp fields
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    
    // New field for tracking ticket printing
    pub printed_at: Option<DateTime<Local>>,

    // New optional field for subtasks
    pub subtasks: Option<String>,

    // New field for archiving
    pub archived: bool,
}

impl TodoItem {
    /// Creates a new TodoItem, typically used before insertion into the database.
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
            subtasks: None, // Initialize subtasks as None
            archived: false, // Initialize archived as false
        }
    }
}
