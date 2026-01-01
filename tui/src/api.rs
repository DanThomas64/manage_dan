use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use chrono::{DateTime, Local}; // Import chrono types

// --- Data Structures copied from app/src/nogo.rs ---
// We must redefine these here to deserialize the response correctly.

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Status {
    Init,
    Go,
    Nogo,
    Degraded,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SystemsStatus {
    pub db: Status,
    pub log: Status,
    pub notes: Status,
    pub project: Status,
    pub printer: Status,
    pub todo: Status,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SystemsGoNogo {
    pub gono: Status,
}

#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub systems: SystemsStatus,
    pub overall: SystemsGoNogo,
}
// ---------------------------------------------------

// --- Todo Data Structure copied from db/src/models.rs (Canonical source) ---

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
            subtasks: None,
            archived: false,
        }
    }
}

// ---------------------------------------------------

pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Self {
        ApiClient {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("Failed to build HTTP client"),
            base_url: base_url.to_string(),
        }
    }

    pub async fn fetch_status(&self) -> Result<StatusResponse> {
        let url = format!("{}/api/v1/status", self.base_url);
        let response = self.client.get(&url).send().await?.error_for_status()?;
        let status_response: StatusResponse = response.json().await?;
        Ok(status_response)
    }

    // --- Todo CRUD Methods ---

    pub async fn create_todo(&self, item: TodoItem) -> Result<TodoItem> {
        let url = format!("{}/api/v1/todo", self.base_url);
        let response = self.client.post(&url).json(&item).send().await?.error_for_status()?;
        let new_item: TodoItem = response.json().await?;
        Ok(new_item)
    }

    pub async fn fetch_todos(&self) -> Result<Vec<TodoItem>> {
        // Note: API endpoint should handle filtering archived items
        let url = format!("{}/api/v1/todo", self.base_url);
        let response = self.client.get(&url).send().await?.error_for_status()?;
        let items: Vec<TodoItem> = response.json().await?;
        Ok(items)
    }

    pub async fn update_todo(&self, item: TodoItem) -> Result<()> {
        let id = item.id.ok_or(anyhow::anyhow!("Cannot update item without ID"))?;
        let url = format!("{}/api/v1/todo/{}", self.base_url, id);
        self.client.put(&url).json(&item).send().await?.error_for_status()?;
        Ok(())
    }
    
    pub async fn print_todo(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/todo/{}/print", self.base_url, id);
        self.client.post(&url).send().await?.error_for_status()?;
        Ok(())
    }
    
    // NEW: Archive method
    pub async fn archive_todo(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/todo/{}/archive", self.base_url, id);
        self.client.post(&url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn delete_todo(&self, id: i64) -> Result<()> {
        let url = format!("{}/api/v1/todo/{}", self.base_url, id);
        self.client.delete(&url).send().await?.error_for_status()?;
        Ok(())
    }
}
