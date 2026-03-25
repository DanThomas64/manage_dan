//! API client for interacting with the main application server.
//!
//! This module defines data structures mirroring the server's API responses
//! and provides an asynchronous client for fetching data and performing actions.

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use chrono::{DateTime, Local}; // Import chrono types

// --- Data Structures copied from app/src/nogo.rs ---
// We must redefine these here to deserialize the response correctly.

/// Operational status of a subsystem (mirrors app::nogo::Status).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Status {
    Init,
    Go,
    Nogo,
    Degraded,
    Unknown,
}

/// Status of all monitored subsystems (mirrors app::nogo::SystemsStatus).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SystemsStatus {
    pub db: Status,
    pub log: Status,
    pub notes: Status,
    pub project: Status,
    pub printer: Status,
    pub todo: Status,
    pub lists: Status,
}

/// Overall Go/NoGo status (mirrors app::nogo::SystemsGoNogo).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SystemsGoNogo {
    pub gono: Status,
}

/// Response structure for the /status endpoint.
#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub systems: SystemsStatus,
    pub overall: SystemsGoNogo,
}
// ---------------------------------------------------

// --- Log Data Structure ---

/// Represents a single log entry stored in the database (mirrors db::models::LogEntry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: DateTime<Local>,
    pub level: String,
    pub target: String,
    pub message: String,
}

// --- Todo Data Structures (mirrors todo::models) ---

/// A single subtask, backed by a Vikunja child task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub id: Option<i64>,
    pub title: String,
    pub done: bool,
}

/// Represents a single Todo item (mirrors todo::models::TodoItem).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: Option<i64>,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub printed_at: Option<DateTime<Local>>,
    /// Subtasks backed by Vikunja child tasks linked via a `subtask` relation.
    pub subtasks: Vec<Subtask>,
    pub archived: bool,
    pub due_date: Option<DateTime<Local>>,
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

// --- Lists Data Structures ---

/// A top-level list group (mirrors lists::models::ListGroup).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListGroup {
    pub id: i64,
    pub name: String,
}

/// A named list within a group (mirrors lists::models::ListCategory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCategory {
    pub id: i64,
    pub group_id: i64,
    pub name: String,
}

/// A saved common item template (mirrors lists::models::CommonItem).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonItem {
    pub id: i64,
    pub category_id: i64,
    pub name: String,
    pub quantity: Option<String>,
}

/// A single item on a list (mirrors lists::models::ListItem).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListItem {
    pub id: i64,
    pub category_id: i64,
    pub name: String,
    pub quantity: Option<String>,
    pub checked: bool,
    pub created_at: DateTime<Local>,
}

// ---------------------------------------------------

/// Client for making HTTP requests to the application API.
pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    /// Creates a new API client targeting the specified base URL.
    pub fn new(base_url: &str) -> Self {
        ApiClient {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("Failed to build HTTP client"),
            base_url: base_url.to_string(),
        }
    }

    /// Fetches the current system status.
    pub async fn fetch_status(&self) -> Result<StatusResponse> {
        let url = format!("{}/api/v1/status", self.base_url);
        let response = self.client.get(&url).send().await?.error_for_status()?;
        let status_response: StatusResponse = response.json().await?;
        Ok(status_response)
    }

    /// Fetches the latest log entries.
    pub async fn fetch_logs(&self, limit: u32) -> Result<Vec<LogEntry>> {
        let url = format!("{}/api/v1/logs?limit={}", self.base_url, limit);
        let response = self.client.get(&url).send().await?.error_for_status()?;
        let logs: Vec<LogEntry> = response.json().await?;
        Ok(logs)
    }

    // --- Todo CRUD Methods ---

    /// Creates a new Todo item.
    pub async fn create_todo(&self, item: TodoItem) -> Result<TodoItem> {
        let url = format!("{}/api/v1/todo", self.base_url);
        let response = self.client.post(&url).json(&item).send().await?.error_for_status()?;
        let new_item: TodoItem = response.json().await?;
        Ok(new_item)
    }

    /// Fetches all non-archived Todo items.
    pub async fn fetch_todos(&self) -> Result<Vec<TodoItem>> {
        // Note: API endpoint should handle filtering archived items
        let url = format!("{}/api/v1/todo", self.base_url);
        let response = self.client.get(&url).send().await?.error_for_status()?;
        let items: Vec<TodoItem> = response.json().await?;
        Ok(items)
    }

    /// Sets the completed state of a Todo item without touching any other fields.
    pub async fn complete_todo(&self, id: i64, done: bool) -> Result<()> {
        let url = format!("{}/api/v1/todo/{}/done", self.base_url, id);
        self.client
            .patch(&url)
            .json(&serde_json::json!({ "done": done }))
            .send().await?.error_for_status()?;
        Ok(())
    }
    
    /// Manually prints a ticket for a Todo item by ID.
    pub async fn print_todo(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/todo/{}/print", self.base_url, id);
        self.client.post(&url).send().await?.error_for_status()?;
        Ok(())
    }
    
    /// Archives a Todo item by ID.
    pub async fn archive_todo(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/todo/{}/archive", self.base_url, id);
        self.client.post(&url).send().await?.error_for_status()?;
        Ok(())
    }

    /// Deletes a Todo item by ID.
    pub async fn delete_todo(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/todo/{}", self.base_url, id);
        self.client.delete(&url).send().await?.error_for_status()?;
        Ok(())
    }

    // --- Lists Methods ---

    pub async fn fetch_list_groups(&self) -> Result<Vec<ListGroup>> {
        let url = format!("{}/api/v1/lists/groups", self.base_url);
        Ok(self.client.get(&url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn add_list_group(&self, name: &str) -> Result<ListGroup> {
        let url = format!("{}/api/v1/lists/groups", self.base_url);
        Ok(self.client.post(&url)
            .json(&serde_json::json!({ "name": name }))
            .send().await?.error_for_status()?.json().await?)
    }

    pub async fn delete_list_group(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/lists/groups/{}", self.base_url, id);
        self.client.delete(&url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn fetch_list_categories(&self, group_id: i64) -> Result<Vec<ListCategory>> {
        let url = format!("{}/api/v1/lists/groups/{}/categories", self.base_url, group_id);
        Ok(self.client.get(&url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn add_list_category(&self, group_id: i64, name: &str) -> Result<ListCategory> {
        let url = format!("{}/api/v1/lists/groups/{}/categories", self.base_url, group_id);
        Ok(self.client.post(&url)
            .json(&serde_json::json!({ "name": name }))
            .send().await?.error_for_status()?.json().await?)
    }

    pub async fn delete_list_category(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/lists/categories/{}", self.base_url, id);
        self.client.delete(&url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn fetch_list_items(&self, category_id: i64) -> Result<Vec<ListItem>> {
        let url = format!("{}/api/v1/lists/categories/{}/items", self.base_url, category_id);
        Ok(self.client.get(&url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn add_list_item(&self, category_id: i64, name: &str, quantity: Option<&str>) -> Result<ListItem> {
        let url = format!("{}/api/v1/lists/categories/{}/items", self.base_url, category_id);
        Ok(self.client.post(&url)
            .json(&serde_json::json!({ "name": name, "quantity": quantity }))
            .send().await?.error_for_status()?.json().await?)
    }

    pub async fn check_list_item(&self, id: i64, checked: bool) -> Result<()> {
        let url = format!("{}/api/v1/lists/items/{}/check", self.base_url, id);
        self.client.patch(&url)
            .json(&serde_json::json!({ "checked": checked }))
            .send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn delete_list_item(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/lists/items/{}", self.base_url, id);
        self.client.delete(&url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn clear_list_checked(&self, category_id: i64) -> Result<()> {
        let url = format!("{}/api/v1/lists/categories/{}/clear", self.base_url, category_id);
        self.client.post(&url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn print_list(&self, category_id: i64) -> Result<()> {
        let url = format!("{}/api/v1/lists/categories/{}/print", self.base_url, category_id);
        self.client.post(&url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn fetch_common_items(&self, category_id: i64) -> Result<Vec<CommonItem>> {
        let url = format!("{}/api/v1/lists/categories/{}/common", self.base_url, category_id);
        Ok(self.client.get(&url).send().await?.error_for_status()?.json().await?)
    }

    pub async fn add_common_item(&self, category_id: i64, name: &str, quantity: Option<&str>) -> Result<CommonItem> {
        let url = format!("{}/api/v1/lists/categories/{}/common", self.base_url, category_id);
        Ok(self.client.post(&url)
            .json(&serde_json::json!({ "name": name, "quantity": quantity }))
            .send().await?.error_for_status()?.json().await?)
    }

    pub async fn delete_common_item(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/lists/common/{}", self.base_url, id);
        self.client.delete(&url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn add_item_from_common(&self, common_id: i64) -> Result<ListItem> {
        let url = format!("{}/api/v1/lists/common/{}/add", self.base_url, common_id);
        Ok(self.client.post(&url).send().await?.error_for_status()?.json().await?)
    }
}
