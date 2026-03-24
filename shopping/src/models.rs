//! Data models for the Shopping subsystem.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// A named shopping list (e.g. "Groceries", "Toiletries").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShoppingCategory {
    pub id: i64,
    pub name: String,
}

/// A single item on a shopping list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShoppingItem {
    pub id: i64,
    pub category_id: i64,
    /// Item name, e.g. "Milk".
    pub name: String,
    /// Optional quantity/unit string, e.g. "2L" or "x3".
    pub quantity: Option<String>,
    /// Whether the item has been ticked off.
    pub checked: bool,
    pub created_at: DateTime<Local>,
}
