//! API handlers and routing for the application server.
//!
//! This module defines the HTTP endpoints using the `warp` framework, handling
//! requests related to system status and Todo item management.

use crate::prelude::*;
use warp::{Filter, Rejection, Reply, http::StatusCode};
use todo::todo_prelude::TodoItem;
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

/// PATCH /api/v1/todo/:id/done - Toggle completed state without touching other fields
#[derive(Deserialize)]
pub struct SetDoneBody { pub done: bool }

pub async fn set_todo_done_handler(id: i64, body: SetDoneBody) -> Result<impl Reply, Rejection> {
    match todo::complete_item(id, body.done).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to set done for todo item {}: {}", id, e);
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

// --- List Endpoints ---

/// GET /api/v1/lists/groups
pub async fn list_groups_handler() -> Result<impl Reply, Rejection> {
    match lists::list_groups().await {
        Ok(groups) => Ok(warp::reply::json(&groups)),
        Err(e) => {
            error!("Failed to list groups: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/groups
#[derive(Deserialize)]
pub struct AddGroupBody { pub name: String }

pub async fn add_group_handler(body: AddGroupBody) -> Result<impl Reply, Rejection> {
    match lists::add_group(&body.name).await {
        Ok(group) => Ok(warp::reply::with_status(warp::reply::json(&group), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add group: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/groups/:id
pub async fn delete_group_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_group(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete group {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// GET /api/v1/lists/groups/:group_id/categories
pub async fn list_categories_handler(group_id: i64) -> Result<impl Reply, Rejection> {
    match lists::list_categories(group_id).await {
        Ok(cats) => Ok(warp::reply::json(&cats)),
        Err(e) => {
            error!("Failed to list categories for group {}: {}", group_id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/groups/:group_id/categories
#[derive(Deserialize)]
pub struct AddCategoryBody { pub name: String }

pub async fn add_category_handler(group_id: i64, body: AddCategoryBody) -> Result<impl Reply, Rejection> {
    match lists::add_category(group_id, &body.name).await {
        Ok(cat) => Ok(warp::reply::with_status(warp::reply::json(&cat), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add category: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/categories/:id
pub async fn delete_category_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_category(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// GET /api/v1/lists/categories/:id/items
pub async fn list_items_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::list_items(id).await {
        Ok(items) => Ok(warp::reply::json(&items)),
        Err(e) => {
            error!("Failed to list items for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/items
#[derive(Deserialize)]
pub struct AddItemBody { pub name: String, pub quantity: Option<String> }

pub async fn add_item_handler(cat_id: i64, body: AddItemBody) -> Result<impl Reply, Rejection> {
    match lists::add_item(cat_id, &body.name, body.quantity.as_deref()).await {
        Ok(item) => Ok(warp::reply::with_status(warp::reply::json(&item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add item: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// PATCH /api/v1/lists/items/:id/check
#[derive(Deserialize)]
pub struct CheckItemBody { pub checked: bool }

pub async fn check_item_handler(id: i64, body: CheckItemBody) -> Result<impl Reply, Rejection> {
    match lists::check_item(id, body.checked).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to check item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/items/:id
pub async fn delete_item_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/clear
pub async fn clear_checked_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::clear_checked(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to clear checked items for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/print
pub async fn print_list_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::print_list(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to print list for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

// --- Common Items Endpoints ---

/// GET /api/v1/lists/categories/:id/common
pub async fn list_common_items_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::list_common_items(id).await {
        Ok(items) => Ok(warp::reply::json(&items)),
        Err(e) => {
            error!("Failed to list common items for category {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/categories/:id/common
#[derive(Deserialize)]
pub struct AddCommonItemBody { pub name: String, pub quantity: Option<String> }

pub async fn add_common_item_handler(id: i64, body: AddCommonItemBody) -> Result<impl Reply, Rejection> {
    match lists::add_common_item(id, &body.name, body.quantity.as_deref()).await {
        Ok(item) => Ok(warp::reply::with_status(warp::reply::json(&item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add common item: {}", e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// DELETE /api/v1/lists/common/:id
pub async fn delete_common_item_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::delete_common_item(id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to delete common item {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
        }
    }
}

/// POST /api/v1/lists/common/:id/add
pub async fn add_item_from_common_handler(id: i64) -> Result<impl Reply, Rejection> {
    match lists::add_item_from_common(id).await {
        Ok(item) => Ok(warp::reply::with_status(warp::reply::json(&item), StatusCode::CREATED)),
        Err(e) => {
            error!("Failed to add item from common {}: {}", id, e);
            Err(warp::reject::custom(ApiError::ListsOperationFailed))
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
    LogOperationFailed,
    ListsOperationFailed,
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
    } else if let Some(ApiError::ListsOperationFailed) = err.find() {
        Ok(warp::reply::with_status("Lists operation failed", StatusCode::INTERNAL_SERVER_ERROR))
    } else {
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
        
    // PATCH /api/v1/todo/:id/done
    let set_done = todo_base
        .and(warp::path::param::<i64>())
        .and(warp::path("done"))
        .and(warp::patch())
        .and(warp::body::json())
        .and_then(|id, body| set_todo_done_handler(id, body));

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

    summary.or(create).or(read_all).or(update).or(set_done).or(print).or(archive).or(delete)
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

/// Defines routes for the generic lists module.
///
/// URL structure:
///   /api/v1/lists/groups                          — list groups CRUD
///   /api/v1/lists/groups/:gid/categories          — categories within a group
///   /api/v1/lists/categories/:id/items            — items within a category
///   /api/v1/lists/items/:id/check                 — toggle item checked
fn list_routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let lists      = warp::path("lists");
    let groups_seg = warp::path("groups");
    let categories = warp::path("categories");
    let items      = warp::path("items");

    // GET /api/v1/lists/groups
    let list_groups = lists
        .and(groups_seg)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_groups_handler);

    // POST /api/v1/lists/groups
    let add_group = lists
        .and(groups_seg)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(add_group_handler);

    // DELETE /api/v1/lists/groups/:id
    let delete_group = lists
        .and(groups_seg)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_group_handler);

    // GET /api/v1/lists/groups/:group_id/categories
    let list_cats = lists
        .and(groups_seg)
        .and(warp::path::param::<i64>())
        .and(categories)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_categories_handler);

    // POST /api/v1/lists/groups/:group_id/categories
    let add_cat = lists
        .and(groups_seg)
        .and(warp::path::param::<i64>())
        .and(categories)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|gid, body| add_category_handler(gid, body));

    // DELETE /api/v1/lists/categories/:id
    let delete_cat = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_category_handler);

    // GET /api/v1/lists/categories/:id/items
    let list_items = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(items)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_items_handler);

    // POST /api/v1/lists/categories/:id/items
    let add_item = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(items)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|id, body| add_item_handler(id, body));

    // PATCH /api/v1/lists/items/:id/check
    let check_item = lists
        .and(items)
        .and(warp::path::param::<i64>())
        .and(warp::path("check"))
        .and(warp::path::end())
        .and(warp::patch())
        .and(warp::body::json())
        .and_then(|id, body| check_item_handler(id, body));

    // DELETE /api/v1/lists/items/:id
    let delete_item = lists
        .and(items)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_item_handler);

    // POST /api/v1/lists/categories/:id/clear
    let clear = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("clear"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(clear_checked_handler);

    // POST /api/v1/lists/categories/:id/print
    let print = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(warp::path("print"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(print_list_handler);

    let common_seg = warp::path("common");

    // GET /api/v1/lists/categories/:id/common
    let list_common = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(common_seg)
        .and(warp::path::end())
        .and(warp::get())
        .and_then(list_common_items_handler);

    // POST /api/v1/lists/categories/:id/common
    let add_common = lists
        .and(categories)
        .and(warp::path::param::<i64>())
        .and(common_seg)
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|id, body| add_common_item_handler(id, body));

    // DELETE /api/v1/lists/common/:id
    let delete_common = lists
        .and(common_seg)
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(warp::delete())
        .and_then(delete_common_item_handler);

    // POST /api/v1/lists/common/:id/add
    let add_from_common = lists
        .and(common_seg)
        .and(warp::path::param::<i64>())
        .and(warp::path("add"))
        .and(warp::path::end())
        .and(warp::post())
        .and_then(add_item_from_common_handler);

    list_groups
        .or(add_group)
        .or(delete_group)
        .or(list_cats)
        .or(add_cat)
        .or(delete_cat)
        .or(list_items)
        .or(add_item)
        .or(check_item)
        .or(delete_item)
        .or(clear)
        .or(print)
        .or(list_common)
        .or(add_common)
        .or(delete_common)
        .or(add_from_common)
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
        .or(log_routes())
        .or(list_routes())
    )
    .recover(handle_rejection)
}

/// Starts the HTTP server.
pub async fn start_server(systems_status: SystemsStatus, go_nogo_status: SystemsGoNogo) {
    let routes = routes(systems_status, go_nogo_status);
    let addr = ([0, 0, 0, 0], 8080);
    info!("Starting API server on http://0.0.0.0:8080");
    warp::serve(routes).run(addr).await;
}
