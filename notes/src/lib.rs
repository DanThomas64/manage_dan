pub mod models;
pub mod nb_client;
pub mod notes_error;
pub mod notes_prelude;

use crate::notes_prelude::*;

pub use models::{CreateNoteRequest, Note, UpdateNoteRequest};
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
    let notebook = req.notebook.as_deref().unwrap_or("home");
    let title = req.title.as_deref().unwrap_or("");
    let tags = req.tags.unwrap_or_default();
    let nb_id = nb_client::nb_add(notebook, title, &req.content, &tags).await?;
    nb_client::nb_show(notebook, nb_id).await
}

pub async fn get(nb_id: u64, notebook: &str) -> NotesLibResult<Note> {
    nb_client::nb_show(notebook, nb_id).await
}

pub async fn list(notebook: Option<String>, tag: Option<String>) -> NotesLibResult<Vec<Note>> {
    let mut notes = nb_client::nb_list(notebook.as_deref()).await?;
    if let Some(tag_filter) = tag {
        notes.retain(|n| n.tags.iter().any(|t| *t == tag_filter));
    }
    Ok(notes)
}

pub async fn update(nb_id: u64, notebook: &str, req: UpdateNoteRequest) -> NotesLibResult<Note> {
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

pub async fn search(query: &str) -> NotesLibResult<Vec<Note>> {
    nb_client::nb_search(query).await
}

pub async fn folders() -> NotesLibResult<Vec<String>> {
    nb_client::nb_notebooks().await
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
