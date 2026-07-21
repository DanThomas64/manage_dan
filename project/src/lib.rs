//! Project management subsystem.
//!
//! Groups a named scope of todos, notes, lists, and log entries under one
//! "project", plus a dedicated filesystem folder for code/reference files.
//! Items created without picking a project keep working exactly as before —
//! project association is additive (a shared tag, an nb todo-folder, a lists
//! group), not a new storage layer for those subsystems.
//!
//! Project-scoped todos need no new plumbing in the `todo` crate: its `nb`
//! backend already maps `TodoItem.project_title` 1:1 onto an nb subfolder,
//! so creating a todo with `project_title = Some(project.slug)` and later
//! filtering `todo::read_items()` by that same slug is enough. Archiving a
//! project's nb todos does need one small addition to `todo`
//! (`archive_project_todos`), since the folder/local-id internals required
//! to move those files are private to `todo`'s nb backend.

pub mod project_error;
pub mod project_prelude;
pub mod models;

use std::sync::OnceLock;

use rusqlite::{params, OptionalExtension};

use crate::models::{Project, ProjectDetail};
use crate::project_prelude::*;

static BASE_DIR: OnceLock<String> = OnceLock::new();

fn base_dir() -> &'static str {
    BASE_DIR.get().expect("project subsystem not initialized").as_str()
}

/// Expands a leading `~/` to the user's home directory. No-op otherwise.
fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home.trim_end_matches('/'), rest);
        }
    }
    path.to_string()
}

/// Lowercases, collapses runs of non-alphanumeric characters into a single
/// `_`, and trims leading/trailing `_`. Used to derive a project's `slug`
/// (also its nb todo-folder name and filesystem directory name) from its
/// display name.
fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_was_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('_');
            last_was_dash = true;
        }
    }
    slug.trim_matches('_').to_string()
}

fn row_to_project(row: &rusqlite::Row) -> rusqlite::Result<Project> {
    let archived_str: Option<String> = row.get(6)?;
    let created_str: String = row.get(7)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        tag: row.get(3)?,
        list_group_id: row.get(4)?,
        fs_path: row.get(5)?,
        archived_at: archived_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Local))
        }),
        created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&chrono::Local))
            .unwrap_or_else(|_| chrono::Local::now()),
    })
}

/// Initializes the project subsystem: creates the `projects` table and
/// records the (un-expanded) base directory for project folders.
pub fn init(base_dir: &str) -> ProjectLibResult {
    info!("initializing project (base_dir: {})", base_dir);

    let conn = rusqlite::Connection::open(db::DB_FILE).map_err(db::db_error::DbLibError::Sqlite)?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name          TEXT NOT NULL UNIQUE,
            slug          TEXT NOT NULL UNIQUE,
            tag           TEXT NOT NULL UNIQUE,
            list_group_id INTEGER NOT NULL REFERENCES shopping_list_groups(id),
            fs_path       TEXT NOT NULL,
            archived_at   TEXT,
            created_at    TEXT NOT NULL
        );
        ",
    )
    .map_err(db::db_error::DbLibError::Sqlite)?;

    BASE_DIR
        .set(base_dir.to_string())
        .map_err(|_| ProjectLibError::CannotInitialize("project subsystem already initialized".to_string()))
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

/// Returns all projects (including archived ones), ordered by creation.
pub async fn list_projects() -> ProjectLibResult<Vec<Project>> {
    db::execute_async(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, slug, tag, list_group_id, fs_path, archived_at, created_at
             FROM projects ORDER BY id",
        )?;
        let rows: rusqlite::Result<Vec<Project>> = stmt.query_map([], row_to_project)?.collect();
        rows
    })
    .await
    .map_err(ProjectLibError::Db)
}

/// Fetches a single project by id.
pub async fn get_project(id: i64) -> ProjectLibResult<Project> {
    db::execute_async(move |conn| {
        conn.query_row(
            "SELECT id, name, slug, tag, list_group_id, fs_path, archived_at, created_at
             FROM projects WHERE id = ?1",
            params![id],
            row_to_project,
        )
        .optional()
    })
    .await
    .map_err(ProjectLibError::Db)?
    .ok_or(ProjectLibError::NotFound(id))
}

/// Creates a new project: a lists group (with a default "General" category),
/// a dedicated nb notebook, a filesystem directory, and the project's own
/// DB row.
pub async fn create_project(name: &str) -> ProjectLibResult<Project> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ProjectLibError::InvalidInput("name is required".to_string()));
    }
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(ProjectLibError::InvalidInput(
            "name must contain at least one alphanumeric character".to_string(),
        ));
    }

    let existing = list_projects().await?;
    if existing.iter().any(|p| p.name.eq_ignore_ascii_case(name) || p.slug == slug) {
        return Err(ProjectLibError::DuplicateName(name.to_string()));
    }

    let tag = format!("project-{}", slug);
    let group = lists::add_group(name).await?;
    lists::add_category(group.id, "General").await?;
    notes::ensure_notebook(&slug).await?;

    let fs_path = format!("{}/{}", expand_home(base_dir()).trim_end_matches('/'), slug);
    std::fs::create_dir_all(&fs_path)?;

    let now = chrono::Local::now();
    let now_str = now.to_rfc3339();

    let name_owned = name.to_string();
    let slug_owned = slug.clone();
    let tag_owned = tag.clone();
    let fs_path_owned = fs_path.clone();
    let group_id = group.id;

    let id = db::execute_async(move |conn| {
        conn.execute(
            "INSERT INTO projects (name, slug, tag, list_group_id, fs_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![name_owned, slug_owned, tag_owned, group_id, fs_path_owned, now_str],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
    .map_err(ProjectLibError::Db)?;

    Ok(Project {
        id,
        name: name.to_string(),
        slug,
        tag,
        list_group_id: group_id,
        fs_path,
        archived_at: None,
        created_at: now,
    })
}

// ---------------------------------------------------------------------------
// Scoped fetch helpers
// ---------------------------------------------------------------------------

/// Returns every todo item scoped to `project`, matched by nb folder / slug
/// (see the crate-level doc comment on why no `todo`-crate changes are
/// needed for this) — a real indexed cache lookup (`todo::read_items_by_project`)
/// rather than fetching every project's todos and filtering in Rust.
pub async fn project_todos(project: &Project) -> ProjectLibResult<Vec<todo::models::TodoItem>> {
    Ok(todo::read_items_by_project(&project.slug).await?)
}

/// Returns every note tagged with `project`'s tag — a real indexed cache
/// lookup (`notes::list_by_tag`) rather than fetching every note and
/// filtering in Rust.
pub async fn project_notes(project: &Project) -> ProjectLibResult<Vec<notes::models::Note>> {
    Ok(notes::list_by_tag(&project.tag).await?)
}

/// Returns log entries from the last `days` days tagged with `project`'s tag.
pub async fn project_logs(project: &Project, days: i64) -> ProjectLibResult<Vec<notes::models::LogEntry>> {
    Ok(notes::recent_logs_tagged(days, &project.tag).await?)
}

/// Returns every list category in `project`'s dedicated list group.
pub async fn project_lists(project: &Project) -> ProjectLibResult<Vec<lists::models::ListCategory>> {
    Ok(lists::list_categories(project.list_group_id).await?)
}

/// Aggregates everything scoped to `project` in one call. Once archived,
/// returns metadata only (empty vecs) — archived content lives in the
/// `archive` notebook / zip, not fetched live.
pub async fn project_detail(id: i64) -> ProjectLibResult<ProjectDetail> {
    let project = get_project(id).await?;
    if project.archived_at.is_some() {
        return Ok(ProjectDetail {
            project,
            todos: Vec::new(),
            notes: Vec::new(),
            logs: Vec::new(),
            lists: Vec::new(),
        });
    }

    let (todos, notes, logs, lists) = tokio::join!(
        project_todos(&project),
        project_notes(&project),
        project_logs(&project, 30),
        project_lists(&project),
    );

    // Each section is best-effort: one backend being unreachable (e.g. `nb`
    // not being installed) shouldn't blank out the rest of an otherwise-
    // healthy project page.
    Ok(ProjectDetail {
        project,
        todos: todos.unwrap_or_else(|e| { warn!("project_detail: todos fetch failed: {}", e); Vec::new() }),
        notes: notes.unwrap_or_else(|e| { warn!("project_detail: notes fetch failed: {}", e); Vec::new() }),
        logs: logs.unwrap_or_else(|e| { warn!("project_detail: logs fetch failed: {}", e); Vec::new() }),
        lists: lists.unwrap_or_else(|e| { warn!("project_detail: lists fetch failed: {}", e); Vec::new() }),
    })
}

// ---------------------------------------------------------------------------
// Archiving
// ---------------------------------------------------------------------------

/// Archives a project: non-destructively moves its tagged notes and nb todos
/// into the shared `archive` notebook, zips its filesystem directory and
/// removes the live copy, then marks the DB row archived. Idempotent —
/// archiving an already-archived project just returns it unchanged. Log
/// entries tagged with the project are deliberately left untouched in the
/// shared `log` notebook, and the project's DB row / list group are never
/// deleted (only `archived_at` is set).
pub async fn archive_project(id: i64) -> ProjectLibResult<Project> {
    let project = get_project(id).await?;
    if project.archived_at.is_some() {
        return Ok(project);
    }

    // `nb move` requires its destination notebook to already exist.
    notes::ensure_archive_notebook().await?;

    let tagged_notes = notes::list(None, Some(project.tag.clone())).await?;
    for note in &tagged_notes {
        let dest = format!("{}/{}", project.slug, note.title);
        notes::archive_note(note, &dest).await?;
    }
    // One resync after the whole loop, not per note (`archive_note` itself
    // only clears the stale old-location cache row) — matches
    // `todo::archive_project_todos`'s own post-bulk-move `sync_cache()` call.
    if let Err(e) = notes::sync_cache().await {
        warn!("archive_project: notes cache resync failed: {}", e);
    }

    todo::archive_project_todos(&project.slug).await?;

    let base = expand_home(base_dir());
    let base = base.trim_end_matches('/');
    let archive_dir = format!("{}/.archive", base);
    std::fs::create_dir_all(&archive_dir)?;
    // Canonicalize the output path — `.current_dir(base)` below changes the
    // zip subprocess's cwd (needed so `<slug>` alone resolves to the source
    // directory and gets recorded inside the zip without the full `base`
    // prefix), so a merely `base`-relative zip_path would otherwise resolve
    // against the wrong (already-changed) cwd when `base` itself is relative.
    let zip_path = std::fs::canonicalize(&archive_dir)?.join(format!("{}.zip", project.slug));

    let status = tokio::process::Command::new("zip")
        .arg("-r")
        .arg(&zip_path)
        .arg(&project.slug)
        .current_dir(base)
        .status()
        .await?;
    if !status.success() {
        return Err(ProjectLibError::ArchiveFailed(format!(
            "zip exited with status {}",
            status
        )));
    }
    std::fs::remove_dir_all(&project.fs_path)?;

    let now = chrono::Local::now();
    let now_str = now.to_rfc3339();
    db::execute_async(move |conn| {
        conn.execute(
            "UPDATE projects SET archived_at = ?1 WHERE id = ?2",
            params![now_str, id],
        )?;
        Ok(())
    })
    .await
    .map_err(ProjectLibError::Db)?;

    Ok(Project {
        archived_at: Some(now),
        ..project
    })
}

/// Restores an archived project: the reverse of `archive_project`. Moves
/// its notes and nb todos back out of the shared `archive` notebook (notes
/// via `notes::restore_archived_notes`, todos by folder), unzips the
/// filesystem folder back into place and removes the zip, then clears
/// `archived_at`. Idempotent — restoring an already-live project just
/// returns it unchanged.
pub async fn restore_project(id: i64) -> ProjectLibResult<Project> {
    let project = get_project(id).await?;
    if project.archived_at.is_none() {
        return Ok(project);
    }

    notes::restore_archived_notes(&project.slug, &project.slug).await?;
    todo::restore_project_todos(&project.slug).await?;

    let base = expand_home(base_dir());
    let base = base.trim_end_matches('/');
    let zip_path = format!("{}/.archive/{}.zip", base, project.slug);
    if std::path::Path::new(&zip_path).exists() {
        let status = tokio::process::Command::new("unzip")
            .arg("-o")
            .arg(&zip_path)
            .arg("-d")
            .arg(base)
            .status()
            .await?;
        if !status.success() {
            return Err(ProjectLibError::ArchiveFailed(format!(
                "unzip exited with status {}",
                status
            )));
        }
        std::fs::remove_file(&zip_path)?;
    }

    db::execute_async(move |conn| {
        conn.execute("UPDATE projects SET archived_at = NULL WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .map_err(ProjectLibError::Db)?;

    Ok(Project {
        archived_at: None,
        ..project
    })
}

/// Permanently deletes an archived project: destroys its archived notes and
/// todos (the whole `archive:<slug>/` folder — todos live nested at
/// `archive:<slug>/todo/`, so deleting the parent removes both in one call),
/// its own now-empty nb notebook, its archive zip, its `lists` group, and
/// the `projects` row itself. Irreversible, and only allowed on a project
/// that's already archived — a live project can't be nuked without
/// archiving it first.
pub async fn delete_project(id: i64) -> ProjectLibResult<()> {
    let project = get_project(id).await?;
    if project.archived_at.is_none() {
        return Err(ProjectLibError::InvalidInput(
            "project must be archived before it can be permanently deleted".to_string(),
        ));
    }

    notes::delete_archived_folder(&project.slug).await?;
    notes::delete_notebook(&project.slug).await?;

    let base = expand_home(base_dir());
    let base = base.trim_end_matches('/');
    let zip_path = format!("{}/.archive/{}.zip", base, project.slug);
    std::fs::remove_file(&zip_path).ok();

    // The `projects` row must go before its `lists` group — `list_group_id`
    // is a foreign key into `shopping_list_groups`, so deleting the group
    // first while this row still references it violates the constraint.
    let group_id = project.list_group_id;
    db::execute_async(move |conn| {
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .map_err(ProjectLibError::Db)?;

    lists::delete_group(group_id).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Test Project"), "test_project");
        assert_eq!(slugify("Q1 Planning / Ops"), "q1_planning_ops");
        assert_eq!(slugify("  --weird__name!! "), "weird_name");
    }
}
