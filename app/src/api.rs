//! API handlers and routing for the application server.
//!
//! This module defines the HTTP endpoints using the `warp` framework, handling
//! requests related to system status and Todo item management.

use crate::prelude::*;
use warp::{Filter, Rejection, Reply, http::StatusCode};
use std::sync::Arc;
use todo::todo_prelude::TodoItem; // FIX: Import TodoItem from the todo prelude
use todo::TodoSummary; // NEW: Import TodoSummary
use db::models::LogEntry; // NEW: Import LogEntry from db models
use warp::query::query; // NEW: Import query filter

/// State shared across API handlers.
#[derive(Clone)]
pub struct ApiState {
    // Placeholder for shared state if needed later
}

// --- Status Endpoints ---

/// Handler for the /status endpoint.
///
/// Returns the current status of all initialized systems and the overall Go/NoGo status.
pub async fn get_status(
    systems_status: SystemsStatus,
    go_nogo_status: SystemsGoNogo,
) -> Result<impl Reply, Rejection> {
    // Combine status into a single response object for easy consumption by the TUI
    #[derive(Serialize)]
    struct StatusResponse {
        systems: SystemsStatus,
        overall: SystemsGoNogo,
    }

    let response = StatusResponse {
        systems: systems_status,
        overall: go_nogo_status,
    };

    Ok(warp::reply::json(&response))
}

// --- Todo CRUD Endpoints ---

/// GET /api/v1/todo/summary - Read todo summary statistics
pub async fn read_todo_summary_handler() -> Result<impl Reply, Rejection> {
    match todo::get_summary().await {
        Ok(summary) => Ok(warp::reply::json(&summary)),
        Err(e) => {
            error!("Failed to read todo summary: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// POST /api/v1/todo - Create a new todo item
pub async fn create_todo_handler(item: TodoItem) -> Result<impl Reply, Rejection> {
    match todo::create_item(item).await {
        Ok(new_item) => Ok(warp::reply::with_status(warp::reply::json(&new_item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to create todo item: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// GET /api/v1/todo - Read all todo items (non-archived)
pub async fn read_todos_handler() -> Result<impl Reply, Rejection> {
    match todo::read_items().await {
        Ok(items) => Ok(warp::reply::json(&items)),
        Err(e) => {
            error!("Failed to read todo items: {}", e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// PUT /api/v1/todo/:id - Update an existing todo item
pub async fn update_todo_handler(id: i64, item: TodoItem) -> Result<impl Reply, Rejection> {
    if item.id != Some(id) {
        return Err(warp::reject::custom(ApiError::MismatchedId));
    }
    
    match todo::update_item(item).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to update todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// POST /api/v1/todo/:id/print - Manually print a todo item ticket
pub async fn print_todo_handler(id: i64) -> Result<impl Reply, Rejection> {
    match todo::print_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to print todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// POST /api/v1/todo/:id/archive - Archive a todo item
pub async fn archive_todo_handler(id: i64) -> Result<impl Reply, Rejection> {
    match todo::archive_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to archive todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

/// DELETE /api/v1/todo/:id - Delete a todo item
pub async fn delete_todo_handler(id: i64) -> Result<impl Reply, Rejection> {
    match todo::delete_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete todo item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::TodoOperationFailed))
        }
    }
}

// --- Log Endpoints ---

/// Query parameters for fetching logs.
#[derive(Deserialize)]
pub struct LogQuery {
    limit: Option<u32>,
}

/// GET /api/v1/logs - Read latest log entries
pub async fn read_logs_handler(query: LogQuery) -> Result<impl Reply, Rejection> {
    let limit = query.limit.unwrap_or(20); // Default to 20 logs
    
    match db::log_read_latest(limit).await {
        Ok(logs) => Ok(warp::reply::json(&logs)),
        Err(e) => {
            error!("Failed to read logs: {}", e);
            Err(warp::reject::custom(ApiError::LogOperationFailed))
        }
    }
}

// --- Error Handling ---

/// Custom API errors used for rejection handling.
#[derive(Debug)]
enum ApiError {
    TodoOperationFailed,
    MismatchedId,
    LogOperationFailed, // NEW
}

impl warp::reject::Reject for ApiError {}

/// Handles custom rejections and converts them into appropriate HTTP responses.
async fn handle_rejection(err: Rejection) -> Result<impl Reply, Rejection> {
    if let Some(ApiError::TodoOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Todo operation failed", StatusCode::INTERNAL_SERVER_ERROR))
    } else if let Some(ApiError::MismatchedId) = err.find() {
        Ok(warp::reply::with_status("ID in path does not match ID in body", StatusCode::BAD_REQUEST))
    } else if let Some(ApiError::LogOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Log operation failed", StatusCode::INTERNAL_SERVER_ERROR))
    } else {
        // Let warp handle other rejections (404, method not allowed, etc.)
        Err(err)
    }
}

// --- Route Definition ---

/// Defines routes related to Todo item management.
fn todo_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let todo_base = warp::path("todo");

    // GET /api/v1/todo/summary
    let summary = todo_base
        .and(warp::path("summary"))
        .and(warp::get())
        .and_then(read_todo_summary_handler);

    // POST /api/v1/todo
    let create = todo_base
        .and(warp::post())
        .and(warp::body::json())
        .and_then(create_todo_handler);

    // GET /api/v1/todo
    let read_all = todo_base
        .and(warp::get())
        .and_then(read_todos_handler);

    // PUT /api/v1/todo/:id
    let update = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::put())
        .and(warp::body::json())
        .and_then(|id, item| update_todo_handler(id, item));
        
    // POST /api/v1/todo/:id/print
    let print = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::path("print"))
        .and(warp::post())
        .and_then(print_todo_handler);
        
    // POST /api/v1/todo/:id/archive
    let archive = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::path("archive"))
        .and(warp::post())
        .and_then(archive_todo_handler);

    // DELETE /api/v1/todo/:id
    let delete = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::delete())
        .and_then(delete_todo_handler);

    summary.or(create).or(read_all).or(update).or(print).or(archive).or(delete)
}

/// Defines routes related to system status.
fn status_routes(
    systems_status: SystemsStatus,
    go_nogo_status: SystemsGoNogo,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let systems_status_filter = warp::any().map(move || systems_status);
    let go_nogo_status_filter = warp::any().map(move || go_nogo_status);

    warp::path("status")
        .and(warp::get())
        .and(systems_status_filter)
        .and(go_nogo_status_filter)
        .and_then(get_status)
}

/// Defines routes related to logging.
fn log_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::path("logs")
        .and(warp::get())
        .and(query::<LogQuery>())
        .and_then(read_logs_handler)
}


/// Defines the API routes.
pub fn routes(
    systems_status: SystemsStatus,
    go_nogo_status: SystemsGoNogo,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let api_v1 = warp::path("api").and(warp::path("v1"));

    api_v1.and(
        status_routes(systems_status, go_nogo_status)
        .or(todo_routes())
        .or(log_routes()) // Include log routes
    )
    .recover(handle_rejection)
}

/// Starts the HTTP server.
pub async fn start_server(systems_status: SystemsStatus, go_nogo_status: SystemsGoNogo) {
    let routes = routes(systems_status, go_nogo_status);
    let addr = ([127, 0, 0, 1], 8080);
    info!("Starting API server on http://{}:{}", addr.0.map(|b| b.to_string()).join("."), addr.1);
    warp::serve(routes).run(addr).await;
}
