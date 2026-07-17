//! Vikunja-backed todo storage. All persistence is delegated to a
//! self-hosted Vikunja instance via the `vikunja` crate.

use chrono::{DateTime, Local, Utc};
use tracing::{info, warn};

use vikunja::VikunjaClient;
use vikunja::models::{TaskPayload, VikunjaTask};

use crate::models::{Subtask, TodoItem};
use crate::todo_error::{TodoLibError, TodoLibResult};
use crate::{print_ticket, print_ticket_on_creation, strip_html};

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
        priority: item.priority.min(5) as i64,
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

fn from_vikunja_task(
    task: VikunjaTask,
    printed_at: Option<DateTime<Local>>,
    project_title: Option<String>,
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

    let labels: Vec<String> = task.labels.iter().map(|l| l.title.clone()).collect();

    let reminders: Vec<chrono::DateTime<Local>> = task.reminder_dates.iter()
        .filter_map(|r| r.reminder)
        .map(|dt| dt.with_timezone(&Local))
        .collect();

    TodoItem {
        id: Some(task.id),
        title: task.title,
        description: strip_html(&task.description.unwrap_or_default()),
        completed: task.done,
        created_at: task.created.map(|dt| dt.with_timezone(&Local)).unwrap_or(now),
        updated_at: task.updated.map(|dt| dt.with_timezone(&Local)).unwrap_or(now),
        completed_at: task.done_at.map(|dt| dt.with_timezone(&Local)),
        printed_at,
        subtasks,
        archived: false,
        due_date: task.due_date.map(|dt| dt.with_timezone(&Local)),
        priority: (task.priority.clamp(0, 5)) as u8,
        project_title,
        labels,
        reminders,
    }
}

// --- Labels ---

/// Resolves each label title to a Vikunja label id, creating a new label
/// when no existing one matches (case-insensitively). Vikunja labels are
/// structured entities referenced by id, not free text, so titles must be
/// resolved before they can be attached to a task.
async fn resolve_label_ids(client: &VikunjaClient, titles: &[String]) -> TodoLibResult<Vec<i64>> {
    let mut ids = Vec::with_capacity(titles.len());
    for title in titles {
        let existing = client.list_labels(Some(title)).await?;
        let found = existing.into_iter().find(|l| l.title.eq_ignore_ascii_case(title));
        let id = match found {
            Some(l) => l.id,
            None => client.create_label(title).await?.id,
        };
        ids.push(id);
    }
    Ok(ids)
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

    // 3. Set labels
    if !item.labels.is_empty() {
        let label_ids = resolve_label_ids(client, &item.labels).await?;
        client.set_task_labels(parent.id, &label_ids).await?;
    }

    // 4. Fetch the full task with subtasks populated
    let full = client.get_task(parent.id).await?;
    let project_title = client.get_project(full.project_id).await.ok().map(|p| p.identifier);
    let mut result = from_vikunja_task(full, None, project_title);

    // 5. Attempt automatic print
    print_ticket_on_creation(&mut result).await?;

    Ok(result)
}

/// Returns all top-level (non-subtask) items across all accessible Vikunja projects.
pub async fn read_items() -> TodoLibResult<Vec<TodoItem>> {
    let client = VikunjaClient::get()?;
    let (tasks, projects) = tokio::join!(client.list_all_tasks(), client.list_projects());
    let tasks = tasks?;

    let project_map: std::collections::HashMap<i64, String> = match projects {
        Ok(list) => list.into_iter().map(|p| (p.id, p.identifier)).collect(),
        Err(e) => {
            warn!("read_items: list_projects failed, project titles will be missing: {}", e);
            std::collections::HashMap::new()
        }
    };

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
            let project_title = project_map.get(&t.project_id).cloned();
            from_vikunja_task(t, printed_at, project_title)
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

    // 4. Set the complete label set (bulk endpoint removes anything not passed).
    let label_ids = resolve_label_ids(client, &item.labels).await?;
    client.set_task_labels(id, &label_ids).await?;

    Ok(())
}

/// Marks a task as completed or pending without touching any other fields.
///
/// This is the correct path for a simple done-toggle: it fetches the current
/// task state from Vikunja, flips only the `done` flag, and posts back.
/// Subtasks are left entirely untouched.
pub async fn complete_item(id: i64, completed: bool) -> TodoLibResult {
    info!("Setting todo item {} done={}", id, completed);
    let client = VikunjaClient::get()?;
    let current = client.get_task(id).await?;
    let payload = TaskPayload {
        title: current.title.clone(),
        description: if current.description.as_deref().unwrap_or("").is_empty() {
            None
        } else {
            current.description.clone()
        },
        done: completed,
        due_date: current.due_date,
        priority: current.priority,
    };
    client.update_task(id, payload).await?;
    Ok(())
}

/// Manually prints a ticket for a TodoItem by ID.
pub async fn print_item(id: i64) -> TodoLibResult {
    info!("Manual print request for todo item ID: {}", id);
    let client = VikunjaClient::get()?;
    let task = client.get_task(id).await?;
    let printed_at = db::printed_at_get(id).await.unwrap_or(None);
    let project_title = client.get_project(task.project_id).await.ok().map(|p| p.identifier);
    let item = from_vikunja_task(task, printed_at, project_title);

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

/// Fetches a single TodoItem by its Vikunja task ID.
pub async fn get_item(id: i64) -> TodoLibResult<TodoItem> {
    let client = VikunjaClient::get()?;
    let task = client.get_task(id).await?;
    let printed_at = db::printed_at_get(id).await.unwrap_or(None);
    let project_title = client.get_project(task.project_id).await.ok().map(|p| p.identifier);
    Ok(from_vikunja_task(task, printed_at, project_title))
}
