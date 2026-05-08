//! Background print monitor.
//!
//! Polls all accessible Vikunja projects on a configurable interval.  Every
//! top-level task is printed by default.  Adding a `"Don't Print"` label to a
//! task suppresses ticket printing for that task entirely.
//!
//! A content hash of the task's meaningful fields is stored in the local
//! `printed_tasks` table; a task is only reprinted when that hash changes,
//! preventing duplicate tickets for unchanged tasks.
//!
//! ## Reprint triggers
//! The following changes cause a reprint:
//! - Title or description changed
//! - Due date or priority changed
//! - Any subtask added, removed, renamed, or its done-status toggled
//!
//! Changes that do NOT trigger a reprint:
//! - Completion status (`done`) — completed tasks are never printed
//! - Labels added/removed (would create a feedback loop)
//! - Assignees, comments, position, or other metadata

use chrono::Local;
use tracing::{info, warn};
use tokio::time::{sleep, Duration};

use vikunja::VikunjaClient;
use vikunja::models::VikunjaTask;

use crate::from_vikunja_task;
use crate::print_ticket;

const NO_PRINT_LABEL: &str = "don't print";

/// Returns true if the task has a "Don't Print" label (case-insensitive).
fn has_no_print_label(task: &VikunjaTask) -> bool {
    task.labels
        .iter()
        .any(|l| l.title.to_ascii_lowercase() == NO_PRINT_LABEL)
}

/// Produces a deterministic string that captures every field visible on a
/// printed ticket.  If this string changes between polls, the ticket is
/// reprinted.
pub(crate) fn content_hash(task: &VikunjaTask) -> String {
    // Sort subtasks by ID so the hash is stable regardless of API return order.
    let mut subtasks: Vec<&VikunjaTask> = task
        .related_tasks
        .get("subtask")
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    subtasks.sort_by_key(|s| s.id);

    let subtasks_str = subtasks
        .iter()
        .map(|s| format!("{}:{}", s.title, s.done))
        .collect::<Vec<_>>()
        .join("|");

    format!(
        "title={};desc={};done={};due={};pri={};subs={}",
        task.title,
        task.description.as_deref().unwrap_or(""),
        task.done,
        // Use epoch seconds so the hash is locale/format independent.
        task.due_date.map(|d| d.timestamp()).unwrap_or(0),
        task.priority,
        subtasks_str,
    )
}

/// Checks all print-labelled tasks once and prints any that are new or changed.
async fn poll() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = VikunjaClient::get()?;
    let (all_tasks, projects) = tokio::join!(client.list_all_tasks(), client.list_projects());
    let all_tasks = all_tasks?;

    let project_map: std::collections::HashMap<i64, String> = match projects {
        Ok(list) => list.into_iter().map(|p| (p.id, p.title)).collect(),
        Err(e) => {
            warn!("monitor poll: list_projects failed, project titles will be missing: {}", e);
            std::collections::HashMap::new()
        }
    };

    // Collect subtask IDs so we never try to print a subtask as a top-level ticket.
    let subtask_ids: std::collections::HashSet<i64> = all_tasks
        .iter()
        .flat_map(|t| {
            t.related_tasks
                .get("subtask")
                .into_iter()
                .flat_map(|subs| subs.iter().map(|s| s.id))
        })
        .collect();

    for task in all_tasks
        .iter()
        .filter(|t| !subtask_ids.contains(&t.id) && !has_no_print_label(t))
    {
        let hash = content_hash(task);
        let stored = db::printed_hash_get(task.id).await.unwrap_or(None);

        if stored.as_deref() == Some(hash.as_str()) {
            // Nothing has changed — skip.
            continue;
        }

        // Don't print a ticket when the task has been completed; just record
        // the new hash so the next poll doesn't trigger again.
        if task.done {
            info!(
                "Task {} \"{}\" marked completed — updating hash, skipping print",
                task.id, task.title
            );
            let now = Local::now();
            if let Err(e) = db::printed_record_set(task.id, now, hash).await {
                warn!("Failed to persist print record for task {}: {}", task.id, e);
            }
            continue;
        }

        let reason = if stored.is_none() { "first print" } else { "content changed" };
        info!(
            "Printing task {} \"{}\" ({})",
            task.id, task.title, reason
        );

        let printed_at_stored = db::printed_at_get(task.id).await.unwrap_or(None);
        let project_title = project_map.get(&task.project_id).cloned();
        let item = from_vikunja_task(task.clone(), printed_at_stored, project_title);

        match print_ticket(&item).await {
            Ok(()) => {
                let now = Local::now();
                if let Err(e) = db::printed_record_set(task.id, now, hash).await {
                    warn!("Failed to persist print record for task {}: {}", task.id, e);
                }
                info!("Ticket printed for task {} \"{}\"", task.id, task.title);
            }
            Err(e) => {
                warn!(
                    "Failed to print task {} \"{}\": {}",
                    task.id, task.title, e
                );
            }
        }
    }

    Ok(())
}

/// Starts the print monitor as a background task.
///
/// Runs an initial poll immediately, then repeats every `interval_secs` seconds.
/// Errors within a poll are logged and do not stop the loop.
pub async fn run(interval_secs: u64) {
    info!(
        "Print monitor started (interval: {}s, suppress label: \"{}\")",
        interval_secs, NO_PRINT_LABEL
    );

    loop {
        if let Err(e) = poll().await {
            warn!("Print monitor poll error: {}", e);
        }
        sleep(Duration::from_secs(interval_secs)).await;
    }
}
