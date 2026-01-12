//! Database models used across the application.
//!
//! Defines the structure for data entities stored in the database, such as `TodoItem` and `LogEntry`.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Local};

/// Represents a single Todo item stored in the database.
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

    // NEW: Due date and Priority
    pub due_date: Option<DateTime<Local>>,
    pub priority: u8, // 0-10, 0 being default/no priority set
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
            due_date: None,
            priority: 0,
        }
    }
}

/// Represents a single log entry stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: DateTime<Local>,
    pub level: String,
    pub target: String,
    pub message: String,
}
