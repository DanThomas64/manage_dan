pub mod todo_error;
pub mod todo_prelude;
pub mod models;

use db::models::TodoItem; // Import TodoItem from db
use db::todo_error::{TodoLibResult, TodoLibError}; // Import error types from db
use db::{todo_create, todo_read_all, todo_update, todo_delete, todo_read_one}; // Import DB functions, including new todo_read_one
use tracing::{info, warn}; // Import info macro
use printer::PrintJob; // Import PrintJob
use printer::printer_error::PrinterLibResult; // Import PrinterLibResult
use chrono::Local; // Import Local for timestamp handling

/// Helper function to create and execute a print job for a TodoItem.
async fn print_ticket(item: &TodoItem) -> PrinterLibResult {
    let title = format!("TODO TICKET #{}", item.id.unwrap_or(0));
    
    let mut lines = vec![
        format!("Title: {}", item.title),
        format!("Status: {}", if item.completed { "COMPLETED" } else { "PENDING" }),
    ];
    
    // Description is now required
    lines.push(String::new());
    lines.push("Description:".to_string());
    lines.extend(item.description.lines().map(|s| format!("  {}", s)));
    
    // NEW: Subtasks
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

/// Checks if the item needs printing (updated_at > printed_at) and prints if necessary.
/// If printing succeeds, updates the item's printed_at timestamp in the database.
/// 
/// `force_print`: If true, skips the updated_at > printed_at check.
/// `skip_if_toggled_off`: If true, skips printing if the item was just toggled from completed to pending.
async fn print_ticket_if_needed(
    item: &mut TodoItem, 
    force_print: bool, 
    skip_if_toggled_off: bool,
    old_item: Option<&TodoItem>
) -> TodoLibResult {
    
    // 1. Never print if the item is completed or archived
    if item.completed || item.archived {
        return Ok(());
    }
    
    // 2. Check if we should skip printing because it was toggled from completed to pending
    if skip_if_toggled_off {
        if let Some(old) = old_item {
            // If it was completed before, but is not completed now, it was toggled off.
            if old.completed && !item.completed {
                info!("Skipping automatic print: Item ID {} toggled from completed to pending.", item.id.unwrap_or(0));
                return Ok(());
            }
        }
    }

    // 3. Check if printing is required based on timestamps or force flag
    let should_print = force_print || item.printed_at.map_or(true, |p_at| item.updated_at > p_at);
    
    if should_print {
        info!("Attempting to print ticket for Todo ID {} (Updated: {:?}, Printed: {:?}, Forced: {})", 
              item.id.unwrap_or(0), item.updated_at, item.printed_at, force_print);
              
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
                // Log error but don't fail the overall operation (creation/update)
                warn!("Failed to print ticket for Todo ID {}: {}", item.id.unwrap_or(0), e);
                // Do NOT update printed_at, so it tries again later if the item is updated again.
                Ok(())
            }
        }
    } else {
        Ok(())
    }
}


/// Creates a new TodoItem in the database. Returns the inserted item with its ID.
pub async fn create_item(mut item: TodoItem) -> TodoLibResult<TodoItem> {
    info!("Creating new todo item: {}", item.title);
    
    // Ensure new items are not archived
    item.archived = false;
    
    let new_item = todo_create(item).await?;
    
    // Attempt automatic print immediately after creation (no old item, not toggled off)
    let mut printable_item = new_item.clone();
    print_ticket_if_needed(&mut printable_item, false, false, None).await?;
    
    // Return the item, potentially updated with printed_at timestamp
    Ok(printable_item)
}

/// Reads all TodoItems from the database, filtering out archived items by default.
pub async fn read_items() -> TodoLibResult<Vec<TodoItem>> {
    info!("Reading all non-archived todo items");

    // Pass false to todo_read_all to filter archived items
    todo_read_all(false).await
}

/// Updates an existing TodoItem in the database.
pub async fn update_item(mut item: TodoItem) -> TodoLibResult {
    let id = item.id.ok_or_else(|| TodoLibError::Unknown)?; // ID must be present for update
    info!("Updating todo item ID: {}", id);

    // 1. Read old item state to check for status toggle
    let old_item = todo_read_one(id).await.ok();
    
    // 2. Perform the update first (this updates `updated_at` in the DB)
    todo_update(item.clone()).await?;
    
    // 3. Manually set `updated_at` to now for the print check, matching what the DB used.
    item.updated_at = Local::now(); 
    
    // 4. Attempt automatic print if needed, skipping if it was toggled off completion
    print_ticket_if_needed(&mut item, false, true, old_item.as_ref()).await
}

/// Manually prints a ticket for a TodoItem by ID, regardless of timestamps.
pub async fn print_item(id: i64) -> TodoLibResult {
    info!("Manual print request for todo item ID: {}", id);
    
    let mut item = todo_read_one(id).await?;
    
    // Force print, do not skip if toggled off (since this is manual), no old item needed
    print_ticket_if_needed(&mut item, true, false, None).await
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

// The synchronous init function remains the same, only ensuring the module is initialized.
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
