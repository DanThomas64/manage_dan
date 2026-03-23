//! Database models used across the application.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Local};

/// Represents a single log entry stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: DateTime<Local>,
    pub level: String,
    pub target: String,
    pub message: String,
}
