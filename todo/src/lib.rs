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
pub mod daily_summary;
pub mod recurring;
pub mod reminders;

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

// --- HTML stripping ---

/// Converts an HTML description (as stored by Vikunja's rich-text editor) to
/// plain text suitable for printing.
///
/// Block-level closing tags become newlines; all remaining markup is removed;
/// common HTML entities are decoded; consecutive blank lines are collapsed.
fn strip_html(html: &str) -> String {
    // Block elements that should become line breaks.
    let s = html
        .replace("</p>",          "\n")
        .replace("</div>",        "\n")
        .replace("</li>",         "\n")
        .replace("</h1>",         "\n")
        .replace("</h2>",         "\n")
        .replace("</h3>",         "\n")
        .replace("</h4>",         "\n")
        .replace("</blockquote>", "\n")
        .replace("<br>",          "\n")
        .replace("<br/>",         "\n")
        .replace("<br />",        "\n");

    // Strip all remaining tags.
    let mut plain = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _   => if !in_tag { plain.push(c) },
        }
    }

    // Decode common HTML entities.
    let plain = plain
        .replace("&amp;",  "&")
        .replace("&lt;",   "<")
        .replace("&gt;",   ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;",  "'")
        .replace("&apos;", "'");

    // Collapse runs of blank lines and trim each line.
    let mut out: Vec<&str> = Vec::new();
    let mut last_blank = false;
    for line in plain.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !last_blank { out.push(""); }
            last_blank = true;
        } else {
            out.push(t);
            last_blank = false;
        }
    }

    out.join("\n").trim().to_string()
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

// --- Printing ---

pub(crate) async fn print_ticket(item: &TodoItem) -> printer::printer_error::PrinterLibResult {
    // Use the backend's actual line width so separators and badge alignment
    // match the physical receipt on both USB and terminal.
    let width = printer::line_width();
    let sep = "-".repeat(width);

    let id = item.id.unwrap_or(0);
    let status = if item.completed { "COMPLETED" } else { "PENDING" };

    // Header line 1: "TODO #42  [ PENDING ]" right-aligned badge.
    let badge = format!("[ {} ]", status);
    let id_str = format!("TODO #{}", id);
    let gap = width.saturating_sub(id_str.len() + badge.len());
    let title = format!("{}{}{}", id_str, " ".repeat(gap), badge);

    // Header line 2: task title (shown as origin)
    let origin = item.title.clone();

    // Priority: Vikunja uses 0=Unset, 1=Low, 2=Medium, 3=High, 4=Urgent, 5=Do Now
    let pri_label = match item.priority {
        1 => "LOW",
        2 => "MEDIUM",
        3 => "HIGH",
        4 => "URGENT",
        5 => "DO NOW",
        _ => "UNSET",
    };
    let filled = item.priority.min(5) as usize;
    let bar = format!("[{}{}]", "#".repeat(filled), ".".repeat(5 - filled));
    let due_str = item.due_date
        .map(|d| d.format("%a %d %b").to_string())
        .unwrap_or_else(|| "None".to_string());
    let info_row = format!("Due: {}  |  Pri: {} {}", due_str, bar, pri_label);

    let mut lines = vec![info_row];

    if let Some(ref project) = item.project_title {
        lines.push(format!("Project: {}", project));
    }
    if !item.labels.is_empty() {
        lines.push(format!("Labels: {}", item.labels.join(", ")));
    }

    lines.push(sep.clone());
    lines.push(String::new());

    // Description
    let has_desc = !item.description.is_empty();
    let has_subs = !item.subtasks.is_empty();

    if has_desc {
        lines.extend(item.description.lines().map(str::to_string));
    }

    // Subtasks
    if has_subs {
        if has_desc {
            lines.push(String::new());
        }
        let done_count = item.subtasks.iter().filter(|s| s.done).count();
        lines.push(format!("Subtasks [{}/{}]", done_count, item.subtasks.len()));
        for sub in &item.subtasks {
            let marker = if sub.done { "[x]" } else { "[ ]" };
            lines.push(format!("  {} {}", marker, sub.title));
        }
    }

    // Pad with blank lines when the body is sparse so the ticket has presence.
    if !has_desc && !has_subs {
        lines.push(String::new());
        lines.push(String::new());
        lines.push(String::new());
    }

    // Footer
    lines.push(String::new());
    lines.push(sep);
    lines.push(format!(
        "Created: {}  |  Updated: {}",
        item.created_at.format("%d %b %Y"),
        item.updated_at.format("%d %b %Y"),
    ));

    PrintJob::new(origin, title, lines)
        .with_qr(format!("manage-dan://todo/{}", id))
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
    let project_title = client.get_project(full.project_id).await.ok().map(|p| p.title);
    let mut result = from_vikunja_task(full, None, project_title);

    // 4. Attempt automatic print
    print_ticket_on_creation(&mut result).await?;

    Ok(result)
}

/// Returns all top-level (non-subtask) items across all accessible Vikunja projects.
pub async fn read_items() -> TodoLibResult<Vec<TodoItem>> {
    let client = VikunjaClient::get()?;
    let (tasks, projects) = tokio::join!(client.list_all_tasks(), client.list_projects());
    let tasks = tasks?;

    let project_map: std::collections::HashMap<i64, String> = projects
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.id, p.title))
        .collect();

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
    let project_title = client.get_project(task.project_id).await.ok().map(|p| p.title);
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
        if item.priority >= 3 {
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
/// Fetches a single TodoItem by its Vikunja task ID.
pub async fn get_item(id: i64) -> TodoLibResult<TodoItem> {
    let client = VikunjaClient::get()?;
    let task = client.get_task(id).await?;
    let printed_at = db::printed_at_get(id).await.unwrap_or(None);
    let project_title = client.get_project(task.project_id).await.ok().map(|p| p.title);
    Ok(from_vikunja_task(task, printed_at, project_title))
}

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
