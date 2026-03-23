//! Business logic layer for Todo item management.
//!
//! All task persistence is delegated to a self-hosted Vikunja instance via the
//! `vikunja` crate.  The only local SQLite usage is the lightweight
//! `printed_tasks` table (managed by the `db` crate) which tracks when a
//! physical ticket was last printed for each task.

pub mod todo_error;
pub mod todo_prelude;
pub mod models;
pub mod monitor;

use chrono::{DateTime, Local, Utc};
use tracing::{info, warn};

use vikunja::VikunjaClient;
use vikunja::models::{TaskPayload, VikunjaTask};

use crate::models::{Subtask, TodoItem};
use crate::todo_error::{TodoLibError, TodoLibResult};
use printer::PrintJob;

// --- Summary ---

/// Summary statistics for pending Todo items.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoSummary {
    pub total_pending: usize,
    pub high_priority_pending: usize,
    pub due_today: usize,
    pub overdue: usize,
}

// --- Mapping helpers ---

fn to_task_payload(item: &TodoItem) -> TaskPayload {
    TaskPayload {
        title: item.title.clone(),
        description: if item.description.is_empty() {
            None
        } else {
            Some(item.description.clone())
        },
        done: item.completed,
        due_date: item.due_date.map(|dt| dt.with_timezone(&Utc)),
        priority: item.priority as i64,
    }
}

fn subtask_payload(sub: &Subtask) -> TaskPayload {
    TaskPayload {
        title: sub.title.clone(),
        description: None,
        done: sub.done,
        due_date: None,
        priority: 0,
    }
}

pub(crate) fn from_vikunja_task(
    task: VikunjaTask,
    printed_at: Option<DateTime<Local>>,
) -> TodoItem {
    let now = Local::now();

    let subtasks: Vec<Subtask> = task
        .related_tasks
        .get("subtask")
        .map(|subs| {
            subs.iter()
                .map(|s| Subtask {
                    id: Some(s.id),
                    title: s.title.clone(),
                    done: s.done,
                })
                .collect()
        })
        .unwrap_or_default();

    TodoItem {
        id: Some(task.id),
        title: task.title,
        description: task.description.unwrap_or_default(),
        completed: task.done,
        created_at: task.created.map(|dt| dt.with_timezone(&Local)).unwrap_or(now),
        updated_at: task.updated.map(|dt| dt.with_timezone(&Local)).unwrap_or(now),
        completed_at: task.done_at.map(|dt| dt.with_timezone(&Local)),
        printed_at,
        subtasks,
        archived: false,
        due_date: task.due_date.map(|dt| dt.with_timezone(&Local)),
        priority: (task.priority.clamp(0, 255)) as u8,
    }
}

// --- Printing ---

pub(crate) async fn print_ticket(item: &TodoItem) -> printer::printer_error::PrinterLibResult {
    let title = format!("TODO TICKET #{}", item.id.unwrap_or(0));

    let mut lines = vec![
        format!("Title: {}", item.title),
        format!(
            "Status: {}",
            if item.completed { "COMPLETED" } else { "PENDING" }
        ),
        String::new(),
        format!("Priority: {}", item.priority),
    ];

    if let Some(due) = item.due_date {
        lines.push(format!("Due Date: {}", due.format("%Y-%m-%d %H:%M")));
    } else {
        lines.push("Due Date: None".to_string());
    }

    lines.push(String::new());
    lines.push("Description:".to_string());
    lines.extend(item.description.lines().map(|s| format!("  {}", s)));

    if !item.subtasks.is_empty() {
        lines.push(String::new());
        lines.push("Subtasks:".to_string());
        for sub in &item.subtasks {
            let marker = if sub.done { "[x]" } else { "[ ]" };
            lines.push(format!("  {} {}", marker, sub.title));
        }
    }

    lines.push(String::new());
    lines.push(format!("Created: {}", item.created_at.format("%Y-%m-%d %H:%M")));
    lines.push(format!("Updated: {}", item.updated_at.format("%Y-%m-%d %H:%M")));

    if let Some(completed_at) = item.completed_at {
        lines.push(format!(
            "Completed: {}",
            completed_at.format("%Y-%m-%d %H:%M")
        ));
    }

    PrintJob::new("Todo System".to_string(), title, lines)
        .execute(0, 0)
        .await
}

async fn print_ticket_on_creation(item: &mut TodoItem) -> TodoLibResult {
    if item.completed || item.archived {
        return Ok(());
    }

    info!(
        "Attempting to print ticket for newly created Todo ID {}",
        item.id.unwrap_or(0)
    );

    match print_ticket(item).await {
        Ok(()) => {
            let now = Local::now();
            item.printed_at = Some(now);
            let id = item.id.unwrap_or(0);
            if let Err(e) = db::printed_at_set(id, now).await {
                warn!("Failed to persist printed_at for Todo {}: {}", id, e);
            }
            info!("Ticket printed for Todo ID {}", id);
        }
        Err(e) => {
            warn!(
                "Failed to print ticket for Todo ID {}: {}",
                item.id.unwrap_or(0),
                e
            );
        }
    }
    Ok(())
}

// --- CRUD ---

/// Creates a new TodoItem in Vikunja and prints a ticket.
pub async fn create_item(item: TodoItem) -> TodoLibResult<TodoItem> {
    info!("Creating new todo item: {}", item.title);
    let client = VikunjaClient::get()?;

    // 1. Create parent task
    let parent = client.create_task(to_task_payload(&item)).await?;

    // 2. Create subtask tasks and link them
    for sub in &item.subtasks {
        let child = client.create_task(subtask_payload(sub)).await?;
        client.create_subtask_relation(parent.id, child.id).await?;
    }

    // 3. Fetch the full task with subtasks populated
    let full = client.get_task(parent.id).await?;
    let mut result = from_vikunja_task(full, None);

    // 4. Attempt automatic print
    print_ticket_on_creation(&mut result).await?;

    Ok(result)
}

/// Returns all top-level (non-subtask) items across all accessible Vikunja projects.
pub async fn read_items() -> TodoLibResult<Vec<TodoItem>> {
    let client = VikunjaClient::get()?;
    let tasks = client.list_all_tasks().await?;

    // Collect IDs of tasks that appear as subtasks of other tasks so we can
    // exclude them from the top-level list.
    let subtask_ids: std::collections::HashSet<i64> = tasks
        .iter()
        .flat_map(|t| {
            t.related_tasks
                .get("subtask")
                .into_iter()
                .flat_map(|subs| subs.iter().map(|s| s.id))
        })
        .collect();

    let printed_map = db::printed_at_get_all().await.unwrap_or_default();

    let items = tasks
        .into_iter()
        .filter(|t| !subtask_ids.contains(&t.id))
        .map(|t| {
            let printed_at = printed_map.get(&t.id).copied();
            from_vikunja_task(t, printed_at)
        })
        .collect();

    Ok(items)
}

/// Updates a TodoItem in Vikunja, replacing its subtasks entirely.
pub async fn update_item(item: TodoItem) -> TodoLibResult {
    let id = item.id.ok_or(TodoLibError::Unknown)?;
    info!("Updating todo item ID: {}", id);
    let client = VikunjaClient::get()?;

    // 1. Fetch current subtasks so we can clean them up
    let current = client.get_task(id).await?;
    if let Some(existing_subs) = current.related_tasks.get("subtask") {
        for sub in existing_subs {
            // Remove the relation first, then delete the child task
            client.delete_subtask_relation(id, sub.id).await?;
            client.delete_task(sub.id).await?;
        }
    }

    // 2. Update parent task
    client.update_task(id, to_task_payload(&item)).await?;

    // 3. Create new subtasks
    for sub in &item.subtasks {
        let child = client.create_task(subtask_payload(sub)).await?;
        client.create_subtask_relation(id, child.id).await?;
    }

    Ok(())
}

/// Manually prints a ticket for a TodoItem by ID.
pub async fn print_item(id: i64) -> TodoLibResult {
    info!("Manual print request for todo item ID: {}", id);
    let client = VikunjaClient::get()?;
    let task = client.get_task(id).await?;
    let printed_at = db::printed_at_get(id).await.unwrap_or(None);
    let item = from_vikunja_task(task, printed_at);

    match print_ticket(&item).await {
        Ok(()) => {
            let now = Local::now();
            if let Err(e) = db::printed_at_set(id, now).await {
                warn!("Failed to persist printed_at for Todo {}: {}", id, e);
            }
            info!("Ticket manually printed for Todo ID {}", id);
            Ok(())
        }
        Err(e) => Err(TodoLibError::CannotInitialize(format!(
            "Manual print failed: {}",
            e
        ))),
    }
}

/// Archives a TodoItem — deletes it from Vikunja (no native archive concept).
pub async fn archive_item(id: i64) -> TodoLibResult {
    info!("Archiving (deleting) todo item ID: {}", id);
    delete_item(id).await
}

/// Deletes a TodoItem and all its subtasks from Vikunja.
pub async fn delete_item(id: i64) -> TodoLibResult {
    info!("Deleting todo item ID: {}", id);
    let client = VikunjaClient::get()?;

    // Delete subtasks first
    let task = client.get_task(id).await?;
    if let Some(subs) = task.related_tasks.get("subtask") {
        for sub in subs {
            client.delete_task(sub.id).await?;
        }
    }

    client.delete_task(id).await?;
    db::printed_at_delete(id).await.ok();
    Ok(())
}

/// Returns summary statistics for pending todo items.
pub async fn get_summary() -> TodoLibResult<TodoSummary> {
    let items = read_items().await?;
    let now = Local::now();
    let today = now.date_naive();

    let mut total_pending = 0usize;
    let mut high_priority_pending = 0usize;
    let mut due_today = 0usize;
    let mut overdue = 0usize;

    for item in items.iter().filter(|i| !i.completed) {
        total_pending += 1;
        if item.priority >= 8 {
            high_priority_pending += 1;
        }
        if let Some(due) = item.due_date {
            let due_naive = due.date_naive();
            if due_naive == today {
                due_today += 1;
            } else if due < now {
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

/// Initializes the Todo subsystem (Vikunja client).
pub fn init(base_url: &str, api_token: &str, project_id: i64) -> TodoLibResult {
    info!("initializing todo");
    vikunja::init(base_url, api_token, project_id)
        .map_err(TodoLibError::Vikunja)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // init() requires a running Vikunja instance; tested via integration tests.
        assert!(true);
    }
}
