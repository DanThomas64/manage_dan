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

// --- Todo Summary Structure ---

/// Summary statistics for pending Todo items (mirrors todo::TodoSummary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoSummary {
    pub total_pending: usize,
    pub high_priority_pending: usize, // Priority >= 8
    pub due_today: usize,
    pub overdue: usize,
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

    /// Fetches the Todo summary statistics.
    pub async fn fetch_summary(&self) -> Result<TodoSummary> {
        let url = format!("{}/api/v1/todo/summary", self.base_url);
        let response = self.client.get(&url).send().await?.error_for_status()?;
        let summary: TodoSummary = response.json().await?;
        Ok(summary)
    }

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

    /// Updates an existing Todo item.
    pub async fn update_todo(&self, item: TodoItem) -> Result<()> {
        let id = item.id.ok_or(anyhow::anyhow!("Cannot update item without ID"))?;
        let url = format!("{}/api/v1/todo/{}", self.base_url, id);
        self.client.put(&url).json(&item).send().await?.error_for_status()?;
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
}
