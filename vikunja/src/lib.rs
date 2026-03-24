//! HTTP client for the Vikunja task management API.

pub mod models;
pub mod vikunja_error;

use models::{CreateRelation, TaskPayload, VikunjaProject, VikunjaTask};
use std::sync::OnceLock;
use tracing::info;
use vikunja_error::{VikunjaError, VikunjaResult};

/// Deserialises the response body as `T`, converting any JSON error into a
/// `VikunjaError::Api` that includes the raw response text for diagnosis.
async fn decode<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> VikunjaResult<T> {
    let text = resp.text().await.map_err(VikunjaError::Http)?;
    serde_json::from_str(&text).map_err(|e| {
        VikunjaError::Api(format!("JSON decode error: {e}\nBody: {text}"))
    })
}

static VIKUNJA_CLIENT: OnceLock<VikunjaClient> = OnceLock::new();

/// Authenticated HTTP client for a single Vikunja project.
pub struct VikunjaClient {
    client: reqwest::Client,
    base_url: String,
    pub project_id: i64,
}

impl VikunjaClient {
    fn new(base_url: &str, api_token: &str, project_id: i64) -> VikunjaResult<Self> {
        let mut headers = reqwest::header::HeaderMap::new();
        let auth_value = format!("Bearer {}", api_token);
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth_value)
                .map_err(|e| VikunjaError::Api(e.to_string()))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(VikunjaError::Http)?;

        Ok(VikunjaClient {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            project_id,
        })
    }

    /// Returns the global client, or an error if not yet initialized.
    pub fn get() -> VikunjaResult<&'static VikunjaClient> {
        VIKUNJA_CLIENT.get().ok_or(VikunjaError::NotInitialized)
    }

    /// Creates a task in the configured project.
    pub async fn create_task(&self, payload: TaskPayload) -> VikunjaResult<VikunjaTask> {
        let url = format!(
            "{}/api/v1/projects/{}/tasks",
            self.base_url, self.project_id
        );
        let resp = self.client.put(&url).json(&payload).send().await?;
        self.check_response(resp).await
    }

    /// Fetches a single task by ID with subtasks expanded.
    pub async fn get_task(&self, id: i64) -> VikunjaResult<VikunjaTask> {
        let url = format!("{}/api/v1/tasks/{}?expand=subtasks", self.base_url, id);
        let resp = self.client.get(&url).send().await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(VikunjaError::NotFound(id));
        }
        self.check_response(resp).await
    }

    /// Lists all tasks in the configured project with subtasks expanded.
    pub async fn list_tasks(&self) -> VikunjaResult<Vec<VikunjaTask>> {
        let url = format!(
            "{}/api/v1/projects/{}/tasks?expand=subtasks&per_page=500",
            self.base_url, self.project_id
        );
        let resp = self.client.get(&url).send().await?;
        let resp = self.require_success(resp).await?;
        decode(resp).await
    }

    /// Lists ALL tasks across every accessible project, with subtasks expanded.
    /// Paginates automatically until all tasks are retrieved.
    pub async fn list_all_tasks(&self) -> VikunjaResult<Vec<VikunjaTask>> {
        const PAGE_SIZE: usize = 50;
        let mut all_tasks: Vec<VikunjaTask> = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!(
                "{}/api/v1/tasks?expand=subtasks&per_page={}&page={}",
                self.base_url, PAGE_SIZE, page
            );
            let resp = self.client.get(&url).send().await?;
            let resp = self.require_success(resp).await?;
            let batch: Vec<VikunjaTask> = decode(resp).await?;
            let done = batch.len() < PAGE_SIZE;
            all_tasks.extend(batch);
            if done {
                break;
            }
            page += 1;
        }

        Ok(all_tasks)
    }

    /// Updates an existing task.
    pub async fn update_task(&self, id: i64, payload: TaskPayload) -> VikunjaResult<VikunjaTask> {
        let url = format!("{}/api/v1/tasks/{}", self.base_url, id);
        let resp = self.client.post(&url).json(&payload).send().await?;
        self.check_response(resp).await
    }

    /// Fetches a single project by ID.
    pub async fn get_project(&self, id: i64) -> VikunjaResult<VikunjaProject> {
        let url = format!("{}/api/v1/projects/{}", self.base_url, id);
        let resp = self.client.get(&url).send().await?;
        let resp = self.require_success(resp).await?;
        decode(resp).await
    }

    /// Lists all projects accessible to the authenticated user.
    pub async fn list_projects(&self) -> VikunjaResult<Vec<VikunjaProject>> {
        let url = format!("{}/api/v1/projects?per_page=500", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let resp = self.require_success(resp).await?;
        decode(resp).await
    }

    /// Deletes a task by ID.
    pub async fn delete_task(&self, id: i64) -> VikunjaResult<()> {
        let url = format!("{}/api/v1/tasks/{}", self.base_url, id);
        let resp = self.client.delete(&url).send().await?;
        self.require_success(resp).await?;
        Ok(())
    }

    /// Creates a `subtask` relation: makes `child_id` a subtask of `parent_id`.
    pub async fn create_subtask_relation(
        &self,
        parent_id: i64,
        child_id: i64,
    ) -> VikunjaResult<()> {
        let url = format!("{}/api/v1/tasks/{}/relations", self.base_url, parent_id);
        let payload = CreateRelation {
            task_id: parent_id,
            other_task_id: child_id,
            relation_kind: "subtask".to_string(),
        };
        let resp = self.client.put(&url).json(&payload).send().await?;
        self.require_success(resp).await?;
        Ok(())
    }

    /// Removes the `subtask` relation between parent and child.
    pub async fn delete_subtask_relation(
        &self,
        parent_id: i64,
        child_id: i64,
    ) -> VikunjaResult<()> {
        let url = format!(
            "{}/api/v1/tasks/{}/relations/subtask/{}",
            self.base_url, parent_id, child_id
        );
        let resp = self.client.delete(&url).send().await?;
        self.require_success(resp).await?;
        Ok(())
    }

    // --- Helpers ---

    async fn check_response(&self, resp: reqwest::Response) -> VikunjaResult<VikunjaTask> {
        let resp = self.require_success(resp).await?;
        decode(resp).await
    }

    async fn require_success(
        &self,
        resp: reqwest::Response,
    ) -> VikunjaResult<reqwest::Response> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(VikunjaError::Api(format!("HTTP {}: {}", status, body)))
        }
    }
}

/// Initializes the global Vikunja client.
pub fn init(base_url: &str, api_token: &str, project_id: i64) -> VikunjaResult {
    info!(
        "Initializing Vikunja client: {} project {}",
        base_url, project_id
    );
    let client = VikunjaClient::new(base_url, api_token, project_id)?;
    VIKUNJA_CLIENT
        .set(client)
        .map_err(|_| VikunjaError::Api("Vikunja client already initialized".to_string()))
}
