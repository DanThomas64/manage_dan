//! `nb`-backed todo storage.
//!
//! Todos live in a single dedicated `nb` notebook (configured name, default
//! `"todo"`). Vikunja's per-task `project_id` is approximated by one
//! subfolder per project inside that notebook (`todo:work/`,
//! `todo:personal/`, ...) — a stepping stone toward a future system where
//! each project gets its own notebook with its own `todo/` folder. All
//! folder-name <-> project-title mapping lives behind
//! [`resolve_project_folder`] / [`folder_to_project_title`] so that future
//! migration only touches this module.
//!
//! `nb` assigns todo ids *per folder*, not notebook-wide (two different
//! project folders can each have a local id `1`). A small SQLite table
//! (`db::todo_nb_index_*`) maps each `(folder, local_id)` pair to a stable
//! synthetic `i64` so this backend can hand out ids exactly like the
//! Vikunja backend does.
//!
//! Priority has no native `nb` field, so it's encoded as a custom metadata
//! header — an HTML comment `<!-- priority: N -->` appended below nb's own
//! generated content (i.e. below the `## Tags` section, where present) by
//! reading the file back after `nb todo add` and rewriting it with
//! `nb edit --overwrite`. It's invisible when the note is rendered and
//! trivially regex-extractable regardless of its exact position — parsing
//! scans the whole file for the header rather than assuming a fixed spot.
//! Reminders are round-tripped as `remind-<compact-iso>` tags.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use tracing::{info, warn};

use crate::models::{Subtask, TodoItem};
use crate::todo_error::{TodoLibError, TodoLibResult};
use crate::print_ticket_on_creation;

const REMINDER_TAG_PREFIX: &str = "remind-";

// --- Shell-out helper ---

async fn run(args: &[String]) -> TodoLibResult<String> {
    let mut cmd = tokio::process::Command::new("nb");
    cmd.args(args).arg("--no-color");
    let out = cmd.output().await.map_err(|e| {
        TodoLibError::CannotInitialize(format!("failed to run nb: {}", e))
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(TodoLibError::Nb(if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        }));
    }
    Ok(stdout)
}

// --- Selector / project-folder helpers ---

fn selector(folder: &str, local_id: i64) -> String {
    if folder.is_empty() {
        local_id.to_string()
    } else {
        format!("{}/{}", folder, local_id)
    }
}

/// Maps a `TodoItem.project_title` to the nb folder that stores it.
fn resolve_project_folder(project_title: &Option<String>) -> String {
    project_title.clone().unwrap_or_default()
}

/// Maps an nb folder name back to a `TodoItem.project_title`.
fn folder_to_project_title(folder: &str) -> Option<String> {
    if folder.is_empty() {
        None
    } else {
        Some(folder.to_string())
    }
}

// --- Output parsing ---

/// Parses `folder`, `local_id`, and the generated filename out of nb's
/// `Added: [notebook:folder/n] ... notebook:folder/TIMESTAMP.todo.md "..."`
/// style output.
fn parse_added_ref(out: &str) -> TodoLibResult<(String, i64, String)> {
    let line = out
        .lines()
        .find(|l| l.contains("Added:") || l.contains("Updated:"))
        .ok_or_else(|| TodoLibError::Nb(format!("unexpected nb output: {}", out)))?;

    let bracket = line
        .find('[')
        .and_then(|s| line[s + 1..].find(']').map(|e| &line[s + 1..s + 1 + e]))
        .ok_or_else(|| TodoLibError::Nb(format!("unexpected nb output: {}", out)))?;

    let rest = bracket.split_once(':').map(|(_, r)| r).unwrap_or(bracket);
    let (folder, local_id) = match rest.rsplit_once('/') {
        Some((f, id)) => (
            f.to_string(),
            id.parse()
                .map_err(|_| TodoLibError::Nb(format!("bad id in: {}", bracket)))?,
        ),
        None => (
            String::new(),
            rest.parse()
                .map_err(|_| TodoLibError::Nb(format!("bad id in: {}", bracket)))?,
        ),
    };

    let filename = line
        .split_whitespace()
        .find(|tok| tok.ends_with(".todo.md"))
        .unwrap_or("")
        .to_string();

    Ok((folder, local_id, filename))
}

/// Extracts the creation timestamp encoded in nb's todo filenames
/// (`.../<folder/>YYYYMMDDHHMMSS.todo.md`).
fn parse_created_at(path_or_filename: &str) -> Option<DateTime<Local>> {
    let base = path_or_filename.rsplit('/').next()?;
    let ts = base.strip_suffix(".todo.md")?;
    let naive = NaiveDateTime::parse_from_str(ts, "%Y%m%d%H%M%S").ok()?;
    Local.from_local_datetime(&naive).single()
}

fn parse_due(s: &str) -> Option<DateTime<Local>> {
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Local.from_local_datetime(&dt).single());
    }
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Local))
}

fn parse_reminder_tag(s: &str) -> Option<DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S").ok()?;
    Local.from_local_datetime(&naive).single()
}

fn format_reminder_tag(dt: &DateTime<Local>) -> String {
    format!("{}{}", REMINDER_TAG_PREFIX, dt.format("%Y%m%dT%H%M%S"))
}

struct ParsedNote {
    priority: u8,
    completed: bool,
    title: String,
    due: Option<DateTime<Local>>,
    description: String,
    subtasks: Vec<Subtask>,
    labels: Vec<String>,
    reminders: Vec<DateTime<Local>>,
}

/// Parses the fixed layout nb generates/reads for a todo note:
/// ```text
/// # [ ] Title
///
/// ## Due
///
/// 2026-07-20
///
/// ## Description
///
/// text...
///
/// ## Tasks
///
/// - [ ] sub one
/// - [x] sub two
///
/// ## Tags
///
/// #foo #bar
///
/// <!-- priority: N -->        (optional, our own convention, appended below everything nb generates)
/// ```
/// Every nb-generated section is omitted entirely when empty. The priority
/// header can be found anywhere in the file — parsing scans every line for
/// it rather than assuming a fixed position.
/// Parses the `<!-- priority: N -->` header out of a line, wherever it
/// appears in the file.
fn parse_priority_header(line: &str) -> Option<u8> {
    let inner = line.trim().strip_prefix("<!--")?.strip_suffix("-->")?;
    let p = inner.trim().strip_prefix("priority:")?;
    p.trim().parse().ok()
}

fn parse_note_content(content: &str) -> TodoLibResult<ParsedNote> {
    let mut priority = 0u8;
    let lines: Vec<&str> = content
        .lines()
        .filter(|line| match parse_priority_header(line) {
            Some(p) => {
                priority = p;
                false
            }
            None => true,
        })
        .collect();

    let mut idx = 0;
    while idx < lines.len() && lines[idx].trim().is_empty() {
        idx += 1;
    }

    let title_line = lines
        .get(idx)
        .ok_or_else(|| TodoLibError::Nb("empty todo file".to_string()))?;
    idx += 1;
    let rest = title_line.trim_start_matches('#').trim();
    let (completed, title) = if let Some(t) = rest.strip_prefix("[x]") {
        (true, t.trim().to_string())
    } else if let Some(t) = rest.strip_prefix("[ ]") {
        (false, t.trim().to_string())
    } else {
        (false, rest.to_string())
    };

    let mut sections: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    for line in &lines[idx..] {
        if let Some(heading) = line.trim().strip_prefix("## ") {
            let heading = heading.trim().to_string();
            sections.entry(heading.clone()).or_default();
            current = Some(heading);
        } else if let Some(name) = &current {
            sections.get_mut(name).unwrap().push(line.to_string());
        }
    }
    let section_lines = |name: &str| -> Vec<String> {
        let mut v = sections.get(name).cloned().unwrap_or_default();
        while v.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
            v.remove(0);
        }
        while v.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            v.pop();
        }
        v
    };

    let due = section_lines("Due").first().and_then(|l| parse_due(l.trim()));
    let description = section_lines("Description").join("\n");

    let mut subtasks = Vec::new();
    for line in section_lines("Tasks") {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("- [x]") {
            subtasks.push(Subtask {
                id: Some(subtasks.len() as i64 + 1),
                title: rest.trim().to_string(),
                done: true,
            });
        } else if let Some(rest) = t.strip_prefix("- [ ]") {
            subtasks.push(Subtask {
                id: Some(subtasks.len() as i64 + 1),
                title: rest.trim().to_string(),
                done: false,
            });
        }
    }

    let mut labels = Vec::new();
    let mut reminders = Vec::new();
    if let Some(line) = section_lines("Tags").first() {
        for tok in line.split_whitespace() {
            let tag = tok.trim_start_matches('#');
            if tag.is_empty() {
                continue;
            }
            if let Some(rest) = tag.strip_prefix(REMINDER_TAG_PREFIX) {
                if let Some(dt) = parse_reminder_tag(rest) {
                    reminders.push(dt);
                    continue;
                }
            }
            labels.push(tag.to_string());
        }
    }

    Ok(ParsedNote {
        priority,
        completed,
        title,
        due,
        description,
        subtasks,
        labels,
        reminders,
    })
}

// --- Folder / listing helpers ---

/// Extracts folder names from `nb ...:folders` output.
///
/// When there are zero folders, `nb` prints a friendly `"0 folders."`
/// summary plus unrelated import-help boilerplate (exit 0, not an error)
/// instead of an empty list — e.g.:
/// ```text
/// 0 folders.
///
/// Import a file:
///   nb import (<path> | <url>)
/// Help information:
///   nb help import
/// ```
/// None of those lines are real folder entries, so only lines that look
/// like an actual nb reference (`[notebook:id] ...`) are accepted — the
/// same convention `list_paths_in_folder` already relies on.
fn parse_folder_names(raw: &str) -> Vec<String> {
    raw.lines()
        .filter(|l| l.trim_start().starts_with('['))
        .filter_map(|l| l.split_whitespace().last().map(str::to_string))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Lists the top-level project folders inside the todo notebook (one level
/// deep — matches the one-folder-per-project convention, no nesting).
async fn list_folders(notebook: &str) -> TodoLibResult<Vec<String>> {
    let out = run(&[format!("{}:folders", notebook)]).await?;
    Ok(parse_folder_names(&out))
}

/// Lists `(local_id, file_path)` for every todo item directly inside
/// `folder` (root when empty), skipping folder entries.
async fn list_paths_in_folder(notebook: &str, folder: &str) -> TodoLibResult<Vec<(i64, String)>> {
    let mut args = vec![format!("{}:list", notebook)];
    if !folder.is_empty() {
        args.push(format!("{}/", folder));
    }
    args.push("--paths".to_string());
    let out = run(&args).await?;

    Ok(out
        .lines()
        .filter(|l| !l.contains('\u{1F4C2}')) // 📂 folder icon — skip subfolders
        .filter_map(|l| {
            let trimmed = l.trim();
            let bracket = trimmed.strip_prefix('[')?;
            let (ref_part, remainder) = bracket.split_once(']')?;
            let local = ref_part.rsplit_once(':').map(|(_, id)| id).unwrap_or(ref_part);
            let local = local.rsplit_once('/').map(|(_, id)| id).unwrap_or(local);
            let local_id: i64 = local.parse().ok()?;
            let path = remainder.split_whitespace().last()?.to_string();
            Some((local_id, path))
        })
        .collect())
}

async fn hydrate_item(notebook: &str, folder: &str, local_id: i64, path: &str) -> TodoLibResult<TodoItem> {
    let content = run(&[format!("{}:show", notebook), selector(folder, local_id)]).await?;
    let parsed = parse_note_content(&content)?;
    let id = db::todo_nb_index_get_or_create(folder.to_string(), local_id)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?;

    let created_at = parse_created_at(path).unwrap_or_else(Local::now);
    let updated_at = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(DateTime::<Local>::from)
        .unwrap_or(created_at);

    Ok(TodoItem {
        id: Some(id),
        title: parsed.title,
        description: parsed.description,
        completed: parsed.completed,
        created_at,
        updated_at,
        completed_at: if parsed.completed { Some(updated_at) } else { None },
        printed_at: db::printed_at_get(id).await.unwrap_or(None),
        subtasks: parsed.subtasks,
        archived: false,
        due_date: parsed.due,
        priority: parsed.priority,
        project_title: folder_to_project_title(folder),
        labels: parsed.labels,
        reminders: parsed.reminders,
    })
}

// --- Create / update shared core ---

/// Creates the nb todo note for `item` (in its resolved project folder),
/// attaches the priority header, and marks completion state — shared by
/// `create_item` and `update_item` (which deletes-and-recreates).
async fn create_raw(notebook: &str, item: &TodoItem) -> TodoLibResult<(String, i64)> {
    let folder = resolve_project_folder(&item.project_title);
    let todo_cmd = format!("{}:todo", notebook);

    let mut args: Vec<String> = vec![todo_cmd.clone(), "add".to_string()];
    if !folder.is_empty() {
        args.push(format!("{}/", folder));
    }
    args.push(item.title.clone());

    if !item.description.is_empty() {
        args.push("--description".to_string());
        args.push(item.description.clone());
    }
    if let Some(due) = item.due_date {
        args.push("--due".to_string());
        args.push(due.to_rfc3339());
    }
    let mut tags: Vec<String> = item.labels.clone();
    tags.extend(item.reminders.iter().map(format_reminder_tag));
    if !tags.is_empty() {
        args.push("--tags".to_string());
        args.push(tags.join(","));
    }
    for sub in &item.subtasks {
        args.push("--task".to_string());
        args.push(sub.title.clone());
    }

    let out = run(&args).await?;
    let (folder, local_id, _filename) = parse_added_ref(&out)?;

    if item.priority > 0 {
        // `nb edit` has no `--append`, so read back what nb just generated
        // and rewrite the whole file with the header below it (below where
        // the `## Tags` section would be, matching nb's own section order).
        let sel = selector(&folder, local_id);
        let current = run(&[format!("{}:show", notebook), sel.clone()]).await?;
        let new_content = format!(
            "{}\n\n<!-- priority: {} -->\n",
            current.trim_end(),
            item.priority
        );
        run(&[
            format!("{}:edit", notebook),
            sel,
            "--overwrite".to_string(),
            "--content".to_string(),
            new_content,
        ])
        .await?;
    }

    // nb creates tasks open by default — mark any that should start done.
    for (i, sub) in item.subtasks.iter().enumerate() {
        if sub.done {
            run(&[
                todo_cmd.clone(),
                "do".to_string(),
                selector(&folder, local_id),
                (i + 1).to_string(),
            ])
            .await?;
        }
    }

    if item.completed {
        run(&[todo_cmd, "do".to_string(), selector(&folder, local_id)]).await?;
    }

    Ok((folder, local_id))
}

// --- Public CRUD ---

pub async fn create_item(notebook: &str, item: TodoItem) -> TodoLibResult<TodoItem> {
    info!("Creating new nb todo item: {}", item.title);
    let (folder, local_id) = create_raw(notebook, &item).await?;
    let id = db::todo_nb_index_get_or_create(folder, local_id)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?;
    let mut result = get_item(notebook, id).await?;
    print_ticket_on_creation(&mut result).await?;
    Ok(result)
}

pub async fn read_items(notebook: &str) -> TodoLibResult<Vec<TodoItem>> {
    let mut scopes: Vec<String> = vec![String::new()];
    scopes.extend(list_folders(notebook).await?);

    let mut items = Vec::new();
    for folder in scopes {
        let entries = match list_paths_in_folder(notebook, &folder).await {
            Ok(e) => e,
            Err(e) => {
                warn!("nb todo read_items: failed to list folder '{}': {}", folder, e);
                continue;
            }
        };
        for (local_id, path) in entries {
            match hydrate_item(notebook, &folder, local_id, &path).await {
                Ok(item) => items.push(item),
                Err(e) => warn!(
                    "nb todo read_items: failed to load '{}' id {}: {}",
                    folder, local_id, e
                ),
            }
        }
    }
    Ok(items)
}

/// Replaces an item wholesale — nb has no in-place structured edit for
/// due/priority/tags/subtasks together, so this deletes and recreates the
/// note, then repoints the existing synthetic id at the new
/// `(folder, local_id)` pair so the external id stays stable.
pub async fn update_item(notebook: &str, item: TodoItem) -> TodoLibResult {
    let id = item.id.ok_or(TodoLibError::Unknown)?;
    info!("Updating nb todo item ID: {}", id);
    let (old_folder, old_local_id) = db::todo_nb_index_resolve(id)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?
        .ok_or(TodoLibError::NotFound(id))?;

    run(&[
        format!("{}:todo", notebook),
        "delete".to_string(),
        selector(&old_folder, old_local_id),
        "--force".to_string(),
    ])
    .await?;

    let (new_folder, new_local_id) = create_raw(notebook, &item).await?;
    db::todo_nb_index_update(id, new_folder, new_local_id)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?;
    Ok(())
}

/// Flips only the completion state, leaving everything else untouched.
pub async fn complete_item(notebook: &str, id: i64, completed: bool) -> TodoLibResult {
    info!("Setting nb todo item {} done={}", id, completed);
    let (folder, local_id) = db::todo_nb_index_resolve(id)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?
        .ok_or(TodoLibError::NotFound(id))?;

    let sub = if completed { "do" } else { "undo" };
    run(&[
        format!("{}:todo", notebook),
        sub.to_string(),
        selector(&folder, local_id),
    ])
    .await?;
    Ok(())
}

pub async fn print_item(notebook: &str, id: i64) -> TodoLibResult {
    info!("Manual print request for nb todo item ID: {}", id);
    let item = get_item(notebook, id).await?;
    match crate::print_ticket(&item).await {
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

/// Archives a TodoItem — deletes it (no native archive concept, same
/// simplification as the Vikunja backend).
pub async fn archive_item(notebook: &str, id: i64) -> TodoLibResult {
    info!("Archiving (deleting) nb todo item ID: {}", id);
    delete_item(notebook, id).await
}

pub async fn delete_item(notebook: &str, id: i64) -> TodoLibResult {
    info!("Deleting nb todo item ID: {}", id);
    let (folder, local_id) = db::todo_nb_index_resolve(id)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?
        .ok_or(TodoLibError::NotFound(id))?;

    run(&[
        format!("{}:todo", notebook),
        "delete".to_string(),
        selector(&folder, local_id),
        "--force".to_string(),
    ])
    .await?;

    db::printed_at_delete(id).await.ok();
    db::todo_nb_index_delete(id).await.ok();
    Ok(())
}

pub async fn get_item(notebook: &str, id: i64) -> TodoLibResult<TodoItem> {
    let (folder, local_id) = db::todo_nb_index_resolve(id)
        .await
        .map_err(|e| TodoLibError::Db(e.to_string()))?
        .ok_or(TodoLibError::NotFound(id))?;

    let entries = list_paths_in_folder(notebook, &folder).await?;
    let path = entries
        .into_iter()
        .find(|(lid, _)| *lid == local_id)
        .map(|(_, p)| p)
        .ok_or(TodoLibError::NotFound(id))?;

    hydrate_item(notebook, &folder, local_id, &path).await
}

/// Verifies the `nb` binary is available, mirroring `notes::init()`, and
/// ensures the configured notebook exists (unlike `home`/`log`, which the
/// user is expected to have already created, `todo` is new infrastructure
/// this backend introduces — so it self-provisions).
pub fn check_nb_installed(notebook: &str) -> TodoLibResult {
    let out = std::process::Command::new("nb")
        .arg("--version")
        .output()
        .map_err(|_| TodoLibError::CannotInitialize("nb binary not found".to_string()))?;
    if !out.status.success() {
        return Err(TodoLibError::CannotInitialize(
            "nb --version exited with an error".to_string(),
        ));
    }

    // Best-effort: ignore the error if the notebook already exists. If
    // creation genuinely fails for another reason, subsequent `nb todo`
    // calls against it will surface a clear error.
    let _ = std::process::Command::new("nb")
        .args(["notebooks", "add", notebook])
        .output();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_folder_names;

    #[test]
    fn empty_folder_listing_yields_no_folders() {
        let raw = "0 folders.\n\nImport a file:\n  nb import (<path> | <url>)\nHelp information:\n  nb help import\n";
        assert_eq!(parse_folder_names(raw), Vec::<String>::new());
    }

    #[test]
    fn real_folder_listing_is_parsed() {
        let raw = "[todo:1] \u{1F4C2} work\n[todo:2] \u{1F4C2} personal\n";
        assert_eq!(parse_folder_names(raw), vec!["work".to_string(), "personal".to_string()]);
    }
}
