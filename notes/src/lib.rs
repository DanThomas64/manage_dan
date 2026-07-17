pub mod models;
pub mod nb_client;
pub mod notes_error;
pub mod notes_prelude;

use crate::notes_prelude::*;

pub use models::{CreateLogRequest, CreateNoteRequest, LogEntry, Note, UpdateNoteRequest};

/// Notebook all daily log entries are written to, via nb's `daily` plugin.
const LOG_NOTEBOOK: &str = "log";

/// Notebook the todo `nb` backend stores its items in (see the `todo` crate).
/// Excluded from general note browsing for the same reason `LOG_NOTEBOOK` is.
const TODO_NOTEBOOK: &str = "todo";
pub use notes_error::NotesLibError;

pub fn init() -> NotesLibResult {
    info!("initializing notes");
    let out = std::process::Command::new("nb")
        .arg("--version")
        .output()
        .map_err(|_| NotesLibError::NbNotInstalled)?;
    if !out.status.success() {
        return Err(NotesLibError::CannotInitialize("nb --version failed".to_string()));
    }
    Ok(())
}

pub async fn create(req: CreateNoteRequest) -> NotesLibResult<Note> {
    let title = req.title.as_deref().unwrap_or("").trim();
    if title.is_empty() {
        return Err(NotesLibError::InvalidInput("title is required".to_string()));
    }
    let notebook = req.notebook.as_deref().unwrap_or("home");
    let tags = req.tags.unwrap_or_default();
    let nb_id = nb_client::nb_add(notebook, title, &req.content, &tags).await?;
    nb_client::nb_show(notebook, nb_id).await
}

pub async fn create_log(req: CreateLogRequest) -> NotesLibResult<()> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(NotesLibError::InvalidInput("title is required".to_string()));
    }
    let content = req.content.trim();
    if content.is_empty() {
        return Err(NotesLibError::InvalidInput("description is required".to_string()));
    }
    let tags = req.tags.unwrap_or_default();
    nb_client::nb_daily(LOG_NOTEBOOK, title, &tags, content).await
}

pub async fn get(nb_id: u64, notebook: &str) -> NotesLibResult<Note> {
    nb_client::nb_show(notebook, nb_id).await
}

pub async fn list(notebook: Option<String>, tag: Option<String>) -> NotesLibResult<Vec<Note>> {
    let mut notes = nb_client::nb_list(notebook.as_deref()).await?;
    if notebook.is_none() {
        notes.retain(|n| n.notebook != LOG_NOTEBOOK && n.notebook != TODO_NOTEBOOK);
    }
    if let Some(tag_filter) = tag {
        notes.retain(|n| n.tags.iter().any(|t| *t == tag_filter));
    }
    Ok(notes)
}

pub async fn recent_logs(days: i64) -> NotesLibResult<Vec<LogEntry>> {
    nb_client::nb_daily_entries(LOG_NOTEBOOK, days, None).await
}

/// Like [`recent_logs`], but only returns entries carrying `tag` — used to
/// scope the shared `log` notebook to a single project.
pub async fn recent_logs_tagged(days: i64, tag: &str) -> NotesLibResult<Vec<LogEntry>> {
    nb_client::nb_daily_entries(LOG_NOTEBOOK, days, Some(tag)).await
}

pub async fn update(nb_id: u64, notebook: &str, req: UpdateNoteRequest) -> NotesLibResult<Note> {
    if let Some(title) = req.title.as_deref() {
        if title.trim().is_empty() {
            return Err(NotesLibError::InvalidInput("title is required".to_string()));
        }
    }
    nb_client::nb_update(
        notebook,
        nb_id,
        req.title.as_deref(),
        req.content.as_deref(),
        req.tags.as_deref(),
    )
    .await
}

pub async fn delete(nb_id: u64, notebook: &str) -> NotesLibResult {
    nb_client::nb_delete(notebook, nb_id).await
}

/// Moves a note into the shared `archive` notebook at `dest_path` — used by
/// project archiving. Non-destructive: the note's content is preserved, just
/// relocated out of normal browsing.
pub async fn archive_note(note: &Note, dest_path: &str) -> NotesLibResult {
    nb_client::nb_move(&note.notebook, note.nb_id, &format!("archive:{}", dest_path)).await
}

/// Ensures the shared `archive` notebook exists — call before any
/// `nb move ... archive:...` (see `nb_client::nb_ensure_notebook`).
pub async fn ensure_archive_notebook() -> NotesLibResult<()> {
    nb_client::nb_ensure_notebook("archive").await
}

/// Ensures a notebook named `name` exists. Used by the project subsystem to
/// give each project its own notebook up front, rather than relying on
/// `nb add`/`nb daily`'s implicit lazy creation on first note.
pub async fn ensure_notebook(name: &str) -> NotesLibResult<()> {
    nb_client::nb_ensure_notebook(name).await
}

pub async fn search(query: &str) -> NotesLibResult<Vec<Note>> {
    let mut notes = nb_client::nb_search(query).await?;
    notes.retain(|n| n.notebook != LOG_NOTEBOOK && n.notebook != TODO_NOTEBOOK);
    Ok(notes)
}

pub async fn folders() -> NotesLibResult<Vec<String>> {
    let mut notebooks = nb_client::nb_notebooks().await?;
    notebooks.retain(|n| n != LOG_NOTEBOOK && n != TODO_NOTEBOOK);
    Ok(notebooks)
}

pub async fn tags() -> NotesLibResult<Vec<String>> {
    nb_client::nb_tags().await
}

pub async fn print(nb_id: u64, notebook: &str) -> NotesLibResult {
    let note = get(nb_id, notebook).await?;
    let width = printer::line_width();
    let sep = "─".repeat(width);

    let title = if note.title.is_empty() {
        "Untitled Note".to_string()
    } else {
        note.title.clone()
    };
    let origin = format!("NOTE [{}]", note.notebook);

    let mut lines: Vec<String> = Vec::new();

    let mut meta: Vec<String> = Vec::new();
    if !note.notebook.is_empty() {
        meta.push(format!("notebook: {}", note.notebook));
    }
    if !note.tags.is_empty() {
        meta.push(format!("tags: {}", note.tags.join(", ")));
    }
    if !meta.is_empty() {
        lines.push(meta.join("  •  "));
    }

    lines.push(sep.clone());
    lines.push(String::new());

    for line in note.content.lines() {
        lines.push(line.to_string());
    }

    lines.push(String::new());
    lines.push(sep);
    lines.push(format!(
        "Created: {}  |  Updated: {}",
        note.created_at.format("%d %b %Y"),
        note.updated_at.format("%d %b %Y"),
    ));

    printer::PrintJob::new(origin, title, lines)
        .with_qr(format!("manage-dan://notes/{}:{}", note.notebook, nb_id))
        .execute(0, 0)
        .await?;

    Ok(())
}
