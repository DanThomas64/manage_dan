pub mod models;
pub mod monitor;
pub mod nb_client;
pub mod notes_error;
pub mod notes_prelude;

use crate::notes_prelude::*;
use chrono::{DateTime, Local};

pub use models::{CreateLogRequest, CreateNoteRequest, LogEntry, Note, UpdateNoteRequest};

/// Notebook all daily log entries are written to, via nb's `daily` plugin.
const LOG_NOTEBOOK: &str = "log";

/// Notebook the todo `nb` backend stores its items in (see the `todo` crate).
/// Excluded from general note browsing for the same reason `LOG_NOTEBOOK` is.
const TODO_NOTEBOOK: &str = "todo";

/// Shared notebook archived projects' notes/todos get moved into. Excluded
/// from general note browsing — it's only ever browsed deliberately (a
/// specific notebook filter), never mixed into an unscoped listing.
const ARCHIVE_NOTEBOOK: &str = "archive";

/// Notebooks left out of an unscoped ("all notebooks") listing — applied
/// before any note in them is read, not filtered out of the results
/// afterward, so browsing "All" never pays to hydrate a log entry, a todo
/// item, or an archived note only to immediately discard it.
const EXCLUDED_FROM_ALL: &[&str] = &[LOG_NOTEBOOK, TODO_NOTEBOOK, ARCHIVE_NOTEBOOK];

/// Notebooks the note cache never mirrors at all — `log` and `todo` are
/// wholly owned by other features with no code path that ever reads them
/// back out of `note_cache` (the daily-log view stays on its own
/// always-bounded live path, see `recent_logs`; todos live in a separate
/// `todo_cache` table). `archive` and every per-project notebook, unlike
/// `EXCLUDED_FROM_ALL` above, ARE still synced/cached — they're each
/// reachable via a scoped fetch (the "☰ → Archive" browse, and
/// project-note scoping) even though neither shows up in "All".
const EXCLUDED_FROM_CACHE: &[&str] = &[LOG_NOTEBOOK, TODO_NOTEBOOK];

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
    let note = nb_client::nb_show(notebook, nb_id).await?;
    if let Err(e) = db::note_cache_upsert(to_cache_row(&note, None)).await {
        warn!("create: failed to sync cache for note {}:{}: {}", notebook, nb_id, e);
    }
    Ok(note)
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

/// Lists notes, optionally scoped to one notebook — reads the local cache
/// instead of the live `nb` notebooks; kept fresh by the write path above
/// and the background sync pass (`notes::monitor`). When `notebook` is
/// `None`, `EXCLUDED_FROM_ALL` is applied here in Rust (the cache itself
/// still holds `archive`/per-project notebooks, just never surfaced in an
/// unscoped "All" listing) — mirroring exactly what the pre-cache
/// `nb_client::nb_list` used to filter before this change.
pub async fn list(notebook: Option<String>, tag: Option<String>) -> NotesLibResult<Vec<Note>> {
    let rows = if let Some(nb) = &notebook {
        db::note_cache_get_by_notebook(nb.clone()).await
    } else {
        db::note_cache_get_all().await
    }
    .map_err(|e| NotesLibError::Db(e.to_string()))?;

    let mut notes: Vec<Note> = rows.into_iter().map(from_cache_row).collect();
    if notebook.is_none() {
        notes.retain(|n| !EXCLUDED_FROM_ALL.contains(&n.notebook.as_str()));
    }
    if let Some(tag_filter) = tag {
        notes.retain(|n| n.tags.iter().any(|t| *t == tag_filter));
    }
    Ok(notes)
}

/// Returns every note tagged with `tag` — a real indexed join against
/// `note_cache_tags` (see `idx_note_cache_tags_tag`) rather than `list()`'s
/// fetch-everything-then-filter, used by the `project` crate to scope a
/// Project Detail page's notes without paying for every other note too.
pub async fn list_by_tag(tag: &str) -> NotesLibResult<Vec<Note>> {
    let rows = db::note_cache_get_by_tag(tag.to_string())
        .await
        .map_err(|e| NotesLibError::Db(e.to_string()))?;
    Ok(rows.into_iter().map(from_cache_row).collect())
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
    let note = nb_client::nb_update(
        notebook,
        nb_id,
        req.title.as_deref(),
        req.content.as_deref(),
        req.tags.as_deref(),
    )
    .await?;
    if let Err(e) = db::note_cache_upsert(to_cache_row(&note, None)).await {
        warn!("update: failed to sync cache for note {}:{}: {}", notebook, nb_id, e);
    }
    Ok(note)
}

pub async fn delete(nb_id: u64, notebook: &str) -> NotesLibResult {
    nb_client::nb_delete(notebook, nb_id).await?;
    if let Err(e) = db::note_cache_delete(notebook.to_string(), nb_id).await {
        warn!("delete: failed to remove cache row for note {}:{}: {}", notebook, nb_id, e);
    }
    Ok(())
}

/// Moves a note into the shared `archive` notebook at `dest_path` — used by
/// project archiving. Non-destructive: the note's content is preserved, just
/// relocated out of normal browsing. Only removes the stale cache row at its
/// old location immediately; the new `archive`-notebook row is picked up by
/// the next background sync pass rather than constructed here, since this
/// is called in a per-note loop over a whole project's tagged notes (see
/// `project::archive_project`) and every one of them needs the *old* row
/// gone regardless, but there's no per-note urgency for the *new* row to
/// appear before the next sync interval.
pub async fn archive_note(note: &Note, dest_path: &str) -> NotesLibResult {
    nb_client::nb_move(&note.notebook, note.nb_id, &format!("{}:{}", ARCHIVE_NOTEBOOK, dest_path)).await?;
    if let Err(e) = db::note_cache_delete(note.notebook.clone(), note.nb_id).await {
        warn!(
            "archive_note: failed to remove stale cache row for note {}:{}: {}",
            note.notebook, note.nb_id, e
        );
    }
    Ok(())
}

/// Moves every note archived under `archive:<folder>/` back into
/// `dest_notebook`'s root — the reverse of `archive_note`, used when
/// restoring a project. Returns the number of notes moved. Cache rows for
/// the moved notes (stale under `archive`, missing under `dest_notebook`)
/// are left for the next background sync pass rather than reconciled here —
/// this only ever runs during the rare, deliberate "restore a project"
/// action, and both notebooks get fully re-synced within one interval.
pub async fn restore_archived_notes(folder: &str, dest_notebook: &str) -> NotesLibResult<usize> {
    nb_client::nb_restore_folder(ARCHIVE_NOTEBOOK, folder, dest_notebook).await
}

/// Ensures the shared `archive` notebook exists — call before any
/// `nb move ... archive:...` (see `nb_client::nb_ensure_notebook`).
pub async fn ensure_archive_notebook() -> NotesLibResult<()> {
    nb_client::nb_ensure_notebook(ARCHIVE_NOTEBOOK).await
}

/// Ensures a notebook named `name` exists. Used by the project subsystem to
/// give each project its own notebook up front, rather than relying on
/// `nb add`/`nb daily`'s implicit lazy creation on first note.
pub async fn ensure_notebook(name: &str) -> NotesLibResult<()> {
    nb_client::nb_ensure_notebook(name).await
}

/// Permanently deletes `folder` (and everything in it) from the shared
/// `archive` notebook — used when permanently deleting an archived project.
/// Best-effort: a project that never had anything archived under it has no
/// such folder, which `nb` reports as an error; that's not a failure here.
pub async fn delete_archived_folder(folder: &str) -> NotesLibResult<()> {
    let _ = nb_client::nb_delete_folder(ARCHIVE_NOTEBOOK, folder).await;
    Ok(())
}

/// Permanently deletes a project's own dedicated notebook — used when
/// permanently deleting an archived project. Best-effort, same reasoning as
/// `delete_archived_folder`.
pub async fn delete_notebook(name: &str) -> NotesLibResult<()> {
    let _ = nb_client::nb_delete_notebook(name).await;
    Ok(())
}

pub async fn search(query: &str) -> NotesLibResult<Vec<Note>> {
    let mut notes = nb_client::nb_search(query).await?;
    notes.retain(|n| !EXCLUDED_FROM_ALL.contains(&n.notebook.as_str()));
    Ok(notes)
}

pub async fn folders() -> NotesLibResult<Vec<String>> {
    let mut notebooks = nb_client::nb_notebooks().await?;
    notebooks.retain(|n| n != LOG_NOTEBOOK && n != TODO_NOTEBOOK);
    Ok(notebooks)
}

/// Distinct tags across every cached note not in `EXCLUDED_FROM_ALL` — reads
/// the local cache instead of live `nb` notebooks, same reasoning as `list`.
pub async fn tags() -> NotesLibResult<Vec<String>> {
    let rows = db::note_cache_get_all().await.map_err(|e| NotesLibError::Db(e.to_string()))?;
    let mut all_tags: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in rows {
        if EXCLUDED_FROM_ALL.contains(&row.notebook.as_str()) {
            continue;
        }
        all_tags.extend(row.tags);
    }
    let mut result: Vec<String> = all_tags.into_iter().collect();
    result.sort();
    Ok(result)
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

/// Builds the cache row for a freshly-read note, truncating its body to a
/// preview — `note_cache` never stores full content (see `NoteCacheRow`'s
/// doc comment), only what list/browse views actually render.
fn to_cache_row(note: &Note, source_mtime: Option<DateTime<Local>>) -> db::models::NoteCacheRow {
    db::models::NoteCacheRow {
        notebook: note.notebook.clone(),
        nb_id: note.nb_id,
        title: note.title.clone(),
        preview: note.content.chars().take(300).collect(),
        tags: note.tags.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
        source_mtime,
        synced_at: Local::now(),
    }
}

/// Converts a cached row back into the API-facing `Note` shape —
/// `list()`/`tags()`'s cache-backed counterpart to `to_cache_row`. `content`
/// is the cache's truncated preview, not the full body (see `NoteCacheRow`'s
/// doc comment) — list/browse views never needed more than that anyway.
fn from_cache_row(row: db::models::NoteCacheRow) -> Note {
    Note {
        nb_id: row.nb_id,
        notebook: row.notebook,
        title: row.title,
        content: row.preview,
        tags: row.tags,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// Reconciles `note_cache` against the live `nb` notebooks — called on a
/// timer by the background monitor (`notes::monitor`), and once up front by
/// the write path right after a create/update/delete (see those functions).
/// Notes whose source file's mtime hasn't changed since the last sync are
/// skipped entirely (no re-read, no re-parse), so sync cost scales with how
/// much changed since the last pass, not with total note count.
pub async fn sync_cache() -> NotesLibResult<()> {
    let notebooks: Vec<String> = nb_client::nb_notebooks()
        .await?
        .into_iter()
        .filter(|n| !EXCLUDED_FROM_CACHE.contains(&n.as_str()))
        .collect();

    for notebook in &notebooks {
        let mut seen_ids = std::collections::HashSet::new();

        for (id, path) in nb_client::nb_list_paths(notebook).await? {
            seen_ids.insert(id);

            let current_mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(DateTime::<Local>::from);
            let cached_mtime = db::note_cache_get_source_mtime(notebook.clone(), id)
                .await
                .unwrap_or(None);

            if let (Some(cur), Some(cached)) = (current_mtime, cached_mtime) {
                if cur.timestamp() == cached.timestamp() {
                    continue; // unchanged since last sync — skip the read+parse
                }
            }

            match nb_client::parse_note_file(&path, id, notebook) {
                Ok(note) => {
                    if let Err(e) = db::note_cache_upsert(to_cache_row(&note, current_mtime)).await {
                        warn!("notes sync_cache: failed to cache '{}' id {}: {}", notebook, id, e);
                    }
                }
                Err(e) => warn!("notes sync_cache: failed to parse '{}' id {}: {}", notebook, id, e),
            }
        }

        // Remove cache rows for notes no longer present in this notebook's
        // live listing — handles deletes made externally, e.g. via the raw
        // `nb` CLI, bypassing this app entirely.
        if let Ok(cached_keys) = db::note_cache_get_keys(Some(notebook.clone())).await {
            for (nb, cached_id) in cached_keys {
                if !seen_ids.contains(&cached_id) {
                    let _ = db::note_cache_delete(nb, cached_id).await;
                }
            }
        }
    }

    Ok(())
}
