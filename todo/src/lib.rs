//! Business logic layer for Todo item management.
//!
//! This crate handles CRUD operations, summary calculations, and printing logic
//! for Todo items, interacting with the `db` crate for persistence.

pub mod todo_error;
pub mod todo_prelude;
pub mod models;

use db::models::TodoItem; // Import TodoItem from db
use db::todo_error::{TodoLibResult, TodoLibError}; // Import error types from db
use db::{todo_create, todo_read_all, todo_update, todo_delete, todo_read_one}; // Import DB functions, including new todo_read_one
use tracing::{info, warn, debug}; // Import debug macro
use printer::PrintJob; // Import PrintJob
use printer::printer_error::PrinterLibResult; // Import PrinterLibResult
use chrono::{Local, Datelike}; // Removed Timelike, kept Datelike for date_naive()

// --- Todo Summary Structure (Copied from tui/src/api.rs for internal use) ---
/// Summary statistics for pending Todo items.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoSummary {
    pub total_pending: usize,
    pub high_priority_pending: usize, // Priority >= 8
    pub due_today: usize,
    pub overdue: usize,
}
// ---------------------------------------------------------------------------

/// Helper function to create and execute a print job for a TodoItem.
async fn print_ticket(item: &TodoItem) -> PrinterLibResult {
    let title = format!("TODO TICKET #{}", item.id.unwrap_or(0));
    
    let mut lines = vec![
        format!("Title: {}", item.title),
        format!("Status: {}", if item.completed { "COMPLETED" } else { "PENDING" }),
    ];
    
    // NEW: Priority and Due Date
    lines.push(String::new());
    lines.push(format!("Priority: {}", item.priority));
    if let Some(due_date) = item.due_date {
        lines.push(format!("Due Date: {}", due_date.format("%Y-%m-%d %H:%M")));
    } else {
        lines.push("Due Date: None".to_string());
    }
    
    // Description is now required
    lines.push(String::new());
    lines.push("Description:".to_string());
    lines.extend(item.description.lines().map(|s| format!("  {}", s)));
    
    // Subtasks
    if let Some(subtasks) = &item.subtasks {
        lines.push(String::new());
        lines.push("Subtasks/Steps:".to_string());
        lines.extend(subtasks.lines().map(|s| format!("  {}", s)));
    }
    
    lines.push(String::new());
    lines.push(format!("Created: {}", item.created_at.format("%Y-%m-%d %H:%M")));
    lines.push(format!("Updated: {}", item.updated_at.format("%Y-%m-%d %H:%M")));
    
    if let Some(completed_at) = item.completed_at {
        lines.push(format!("Completed: {}", completed_at.format("%Y-%m-%d %H:%M")));
    }
    
    let job = PrintJob::new("Todo System".to_string(), title, lines);
    
    // Execute the job (VID/PID are ignored as per printer/src/lib.rs comment)
    // We use dummy IDs 0, 0 as the printer is initialized globally
    job.execute(0, 0).await
}

/// Checks if the item needs printing (only upon creation) and prints if necessary.
/// If printing succeeds, updates the item's printed_at timestamp in the database.
/// 
/// Note: This function is now only intended to be called immediately after creation.
async fn print_ticket_on_creation(item: &mut TodoItem) -> TodoLibResult {
    
    // 1. Never print if the item is completed or archived upon creation
    if item.completed || item.archived {
        return Ok(());
    }
    
    // 2. Attempt automatic print
    info!("Attempting to print ticket for newly created Todo ID {}", item.id.unwrap_or(0));
              
    match print_ticket(item).await {
        Ok(()) => {
            // Update printed_at timestamp locally
            item.printed_at = Some(Local::now());
            
            // Update the item in the DB to persist the new printed_at timestamp
            let item_to_update = item.clone();
            
            // We must use todo_update here to persist the printed_at timestamp
            db::todo_update(item_to_update).await?;
            
            info!("Ticket printed successfully for Todo ID {}", item.id.unwrap_or(0));
            Ok(())
        }
        Err(e) => {
            // Log error but don't fail the overall operation (creation)
            warn!("Failed to print ticket for Todo ID {}: {}", item.id.unwrap_or(0), e);
            // Do NOT update printed_at.
            Ok(())
        }
    }
}

/// Calculates summary statistics for pending todo items.
pub async fn get_summary() -> TodoLibResult<TodoSummary> {
    // Read all non-archived items
    let items = todo_read_all(false).await?;
    let now = Local::now();
    let today = now.date_naive();

    let mut total_pending = 0;
    let mut high_priority_pending = 0;
    let mut due_today = 0;
    let mut overdue = 0;

    for item in items.iter().filter(|i| !i.completed) {
        total_pending += 1;

        if item.priority >= 8 {
            high_priority_pending += 1;
        }

        if let Some(due_date) = item.due_date {
            let due_date_naive = due_date.date_naive();
            
            if due_date_naive == today {
                due_today += 1;
            } else if due_date < now {
                overdue += 1;
            }
        }
    }

    Ok(TodoSummary {
        total_pending,
        high_priority_pending,
        due_today,
        overdue,
    })
}


/// Creates a new TodoItem in the database. Returns the inserted item with its ID.
pub async fn create_item(mut item: TodoItem) -> TodoLibResult<TodoItem> {
    info!("Creating new todo item: {}", item.title);
    
    // Ensure new items are not archived
    item.archived = false;
    
    let new_item = todo_create(item).await?;
    
    // Attempt automatic print immediately after creation
    let mut printable_item = new_item.clone();
    print_ticket_on_creation(&mut printable_item).await?;
    
    // Return the item, potentially updated with printed_at timestamp
    Ok(printable_item)
}

/// Reads all TodoItems from the database, filtering out archived items by default.
pub async fn read_items() -> TodoLibResult<Vec<TodoItem>> {
    // debug!("Reading all non-archived todo items"); // Removed log entry

    // Pass false to todo_read_all to filter archived items
    todo_read_all(false).await
}

/// Updates an existing TodoItem in the database.
pub async fn update_item(item: TodoItem) -> TodoLibResult {
    let id = item.id.ok_or_else(|| TodoLibError::Unknown)?; // ID must be present for update
    info!("Updating todo item ID: {}", id);

    // 1. Perform the update
    // Note: Automatic printing on update is now disabled per user request.
    todo_update(item).await
}

/// Manually prints a ticket for a TodoItem by ID, regardless of timestamps.
pub async fn print_item(id: i64) -> TodoLibResult {
    info!("Manual print request for todo item ID: {}", id);
    
    let mut item = todo_read_one(id).await?;
    
    // Use the underlying print_ticket function for manual printing, 
    // and update the printed_at timestamp if successful.
    match print_ticket(&item).await {
        Ok(()) => {
            // Update printed_at timestamp locally and persist it
            item.printed_at = Some(Local::now());
            db::todo_update(item).await?;
            info!("Ticket manually printed successfully for Todo ID {}", id);
            Ok(())
        }
        Err(e) => {
            warn!("Failed manual print for Todo ID {}: {}", id, e);
            Err(TodoLibError::CannotInitialize(format!("Manual print failed: {}", e)))
        }
    }
}

/// Archives a TodoItem by ID (sets archived=true).
pub async fn archive_item(id: i64) -> TodoLibResult {
    info!("Archiving todo item ID: {}", id);

    let mut item = todo_read_one(id).await?;
    item.archived = true;
    
    // Update the item in the DB
    todo_update(item).await
}

/// Deletes a TodoItem by ID.
pub async fn delete_item(id: i64) -> TodoLibResult {
    info!("Deleting todo item ID: {}", id);

    todo_delete(id).await
}

/// Initializes the Todo subsystem.
pub fn init() -> TodoLibResult {
    info!("initializing todo");
    // The actual table creation is now handled by db::init()
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = init();
        assert!(result.is_ok());
    }
}
