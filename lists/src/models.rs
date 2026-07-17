//! Data models for the Lists subsystem.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// A top-level list group (e.g. "Shopping Lists", "General Lists").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListGroup {
    pub id: i64,
    pub name: String,
}

/// A named list within a group (e.g. "Groceries", "Movies to Watch").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCategory {
    pub id: i64,
    pub group_id: i64,
    pub name: String,
    pub has_checkboxes: bool,
    pub has_quick_add: bool,
}

/// A saved common item template for a list (used for quick re-add).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonItem {
    pub id: i64,
    pub category_id: i64,
    pub name: String,
    pub quantity: Option<String>,
}

/// A single item on a list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListItem {
    pub id: i64,
    pub category_id: i64,
    /// Item name, e.g. "Milk".
    pub name: String,
    /// Optional quantity/unit string, e.g. "2L" or "x3".
    pub quantity: Option<String>,
    /// Whether the item has been ticked off.
    pub checked: bool,
    pub created_at: DateTime<Local>,
    /// Display position within the category (lower = earlier).
    pub position: i64,
}
