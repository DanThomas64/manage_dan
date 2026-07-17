//! Business logic layer for Todo item management.
//!
//! Persistence is delegated to one of two configurable backends (see
//! [`BackendKind`]): a self-hosted Vikunja instance via the `vikunja` crate,
//! or the `nb` CLI (same tool the `notes` crate shells out to). Only one
//! backend is active per process, selected at `init()` time. Every public
//! CRUD/read function below is a thin dispatcher so callers (and the
//! background monitor/summary tasks) don't need to know which backend is
//! active. The only local SQLite usage besides backend-specific bookkeeping
//! is the lightweight `printed_tasks` table (managed by the `db` crate)
//! which tracks when a physical ticket was last printed for each task.

pub mod todo_error;
pub mod todo_prelude;
pub mod models;
pub mod backends;
pub mod monitor;
pub mod daily_summary;
pub mod completed_summary;
pub mod recurring;
pub mod reminders;

use std::sync::OnceLock;

use chrono::Local;
use tracing::{info, warn};

use crate::models::TodoItem;
use crate::todo_error::{TodoLibError, TodoLibResult};
use printer::PrintJob;

/// Which storage backend is active for this process. Set once by [`init`].
enum BackendKind {
    Vikunja,
    Nb { notebook: String },
}

static BACKEND: OnceLock<BackendKind> = OnceLock::new();

fn backend() -> &'static BackendKind {
    BACKEND.get().expect("todo backend not initialized")
}

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

    let mut lines = Vec::new();

    // Project — centred at the top of the body so it stands out on pickup.
    if let Some(ref project) = item.project_title {
        let label = format!("[ {} ]", project.to_uppercase());
        let padding = width.saturating_sub(label.len()) / 2;
        lines.push(format!("{}{}", " ".repeat(padding), label));
        lines.push(sep.clone());
    }

    lines.push(info_row);

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

pub(crate) async fn print_ticket_on_creation(item: &mut TodoItem) -> TodoLibResult {
    if item.completed || item.archived {
        return Ok(());
    }

    let id = item.id.unwrap_or(0);
    info!("Attempting to print ticket for newly created Todo ID {}", id);

    // Claim the print atomically before doing it. Backends like `nb` shell
    // out several times during creation, which takes long enough for the
    // background print monitor's own poll to see the new item first and
    // print it — whichever side claims the hash wins, the other skips.
    let hash = crate::monitor::content_hash(item);
    match db::printed_claim(id, hash).await {
        Ok(true) => {}
        Ok(false) => {
            info!("Todo {} already printed (monitor won the race) — skipping duplicate print", id);
            item.printed_at = Some(Local::now());
            return Ok(());
        }
        Err(e) => {
            warn!("Failed to claim print for Todo {}: {}", id, e);
            return Ok(());
        }
    }

    match print_ticket(item).await {
        Ok(()) => {
            item.printed_at = Some(Local::now());
            info!("Ticket printed for Todo ID {}", id);
        }
        Err(e) => {
            warn!("Failed to print ticket for Todo ID {}: {}", id, e);
            // Undo the claim so the print monitor's next poll retries it.
            if let Err(e2) = db::printed_at_delete(id).await {
                warn!("Failed to revert print claim for Todo {}: {}", id, e2);
            }
        }
    }
    Ok(())
}

// --- CRUD (dispatched to the active backend) ---

/// Creates a new TodoItem and prints a ticket.
pub async fn create_item(item: TodoItem) -> TodoLibResult<TodoItem> {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::create_item(item).await,
        BackendKind::Nb { notebook } => backends::nb::create_item(notebook, item).await,
    }
}

/// Returns all top-level (non-subtask) items across all projects/folders.
pub async fn read_items() -> TodoLibResult<Vec<TodoItem>> {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::read_items().await,
        BackendKind::Nb { notebook } => backends::nb::read_items(notebook).await,
    }
}

/// Updates a TodoItem, replacing its subtasks entirely.
pub async fn update_item(item: TodoItem) -> TodoLibResult {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::update_item(item).await,
        BackendKind::Nb { notebook } => backends::nb::update_item(notebook, item).await,
    }
}

/// Marks a task as completed or pending without touching any other fields.
pub async fn complete_item(id: i64, completed: bool) -> TodoLibResult {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::complete_item(id, completed).await,
        BackendKind::Nb { notebook } => backends::nb::complete_item(notebook, id, completed).await,
    }
}

/// Manually prints a ticket for a TodoItem by ID.
pub async fn print_item(id: i64) -> TodoLibResult {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::print_item(id).await,
        BackendKind::Nb { notebook } => backends::nb::print_item(notebook, id).await,
    }
}

/// Archives a TodoItem (deletes it — neither backend has a native archive concept).
pub async fn archive_item(id: i64) -> TodoLibResult {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::archive_item(id).await,
        BackendKind::Nb { notebook } => backends::nb::archive_item(notebook, id).await,
    }
}

/// Moves every todo item belonging to a project into the shared `archive`
/// notebook, as part of project archiving. No-op under the Vikunja backend —
/// Vikunja-backend tasks are left untouched by project archiving (see the
/// `project` crate's archive orchestration for why).
pub async fn archive_project_todos(project_slug: &str) -> TodoLibResult {
    match backend() {
        BackendKind::Vikunja => Ok(()),
        BackendKind::Nb { notebook } => backends::nb::archive_project_todos(notebook, project_slug).await,
    }
}

/// Moves every todo item belonging to a project back out of the shared
/// `archive` notebook, as part of un-archiving (restoring) a project. No-op
/// under the Vikunja backend, matching `archive_project_todos`.
pub async fn restore_project_todos(project_slug: &str) -> TodoLibResult {
    match backend() {
        BackendKind::Vikunja => Ok(()),
        BackendKind::Nb { notebook } => backends::nb::restore_project_todos(notebook, project_slug).await,
    }
}

/// Deletes a TodoItem and all its subtasks.
pub async fn delete_item(id: i64) -> TodoLibResult {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::delete_item(id).await,
        BackendKind::Nb { notebook } => backends::nb::delete_item(notebook, id).await,
    }
}

/// Fetches a single TodoItem by id.
pub async fn get_item(id: i64) -> TodoLibResult<TodoItem> {
    match backend() {
        BackendKind::Vikunja => backends::vikunja::get_item(id).await,
        BackendKind::Nb { notebook } => backends::nb::get_item(notebook, id).await,
    }
}

/// Returns summary statistics for pending todo items. Backend-agnostic —
/// only depends on the dispatched `read_items()`.
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

/// Initializes the Todo subsystem: verifies/connects to whichever backend
/// `backend_name` selects ("nb" or anything else defaulting to "vikunja")
/// and records it as the active [`BackendKind`] for the rest of the process.
pub fn init(backend_name: &str, base_url: &str, api_token: &str, project_id: i64, nb_notebook: &str) -> TodoLibResult {
    info!("initializing todo (backend: {})", backend_name);

    let kind = if backend_name == "nb" {
        backends::nb::check_nb_installed(nb_notebook)?;
        BackendKind::Nb { notebook: nb_notebook.to_string() }
    } else {
        vikunja::init(base_url, api_token, project_id).map_err(TodoLibError::Vikunja)?;
        BackendKind::Vikunja
    };

    BACKEND
        .set(kind)
        .map_err(|_| TodoLibError::CannotInitialize("todo backend already initialized".to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // init() requires a running Vikunja instance or `nb`; tested via integration tests.
        assert!(true);
    }
}
