//! Background print monitor.
//!
//! Polls the active backend on a configurable interval. Every top-level
//! item is printed by default. Adding a `"Don't Print"` label to an item
//! suppresses ticket printing for that item entirely.
//!
//! A content hash of the item's meaningful fields is stored in the local
//! `printed_tasks` table; an item is only reprinted when that hash changes,
//! preventing duplicate tickets for unchanged items.
//!
//! ## Reprint triggers
//! The following changes cause a reprint:
//! - Title or description changed
//! - Due date or priority changed
//! - Any subtask added, removed, renamed, or its done-status toggled
//!
//! Changes that do NOT trigger a reprint:
//! - Completion status (`done`) — completed items are never printed
//! - Labels added/removed (would create a feedback loop)
//! - Assignees, comments, position, or other metadata

use tracing::{info, warn};
use tokio::time::{sleep, Duration};

use crate::models::TodoItem;
use crate::print_ticket;

const NO_PRINT_LABEL: &str = "don't print";

/// Returns true if the item has a "Don't Print" label (case-insensitive).
fn has_no_print_label(item: &TodoItem) -> bool {
    item.labels
        .iter()
        .any(|l| l.to_ascii_lowercase() == NO_PRINT_LABEL)
}

/// Produces a deterministic string that captures every field visible on a
/// printed ticket. If this string changes between polls, the ticket is
/// reprinted.
pub(crate) fn content_hash(item: &TodoItem) -> String {
    // Sort subtasks by id so the hash is stable regardless of listing order.
    let mut subtasks = item.subtasks.clone();
    subtasks.sort_by_key(|s| s.id);

    let subtasks_str = subtasks
        .iter()
        .map(|s| format!("{}:{}", s.title, s.done))
        .collect::<Vec<_>>()
        .join("|");

    format!(
        "title={};desc={};done={};due={};pri={};subs={}",
        item.title,
        item.description,
        item.completed,
        // Use epoch seconds so the hash is locale/format independent.
        item.due_date.map(|d| d.timestamp()).unwrap_or(0),
        item.priority,
        subtasks_str,
    )
}

/// Checks all print-labelled items once and prints any that are new or changed.
async fn poll() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let items = crate::read_items().await?;

    for item in items.iter().filter(|i| !has_no_print_label(i)) {
        let id = item.id.unwrap_or(0);
        let hash = content_hash(item);
        let stored = db::printed_hash_get(id).await.unwrap_or(None);

        if stored.as_deref() == Some(hash.as_str()) {
            // Nothing has changed — skip.
            continue;
        }

        // Don't print a ticket when the item has been completed; just record
        // the new hash so the next poll doesn't trigger again.
        if item.completed {
            info!(
                "Todo {} \"{}\" marked completed — updating hash, skipping print",
                id, item.title
            );
            let _ = db::printed_claim(id, hash).await;
            continue;
        }

        // Claim the print atomically before doing it. The creation path can
        // be mid-flight on the very same task at this instant (e.g. `nb`
        // shelling out takes long enough for this poll to land in between) —
        // whichever of the two claims the hash first is the one that prints.
        match db::printed_claim(id, hash).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                warn!("Failed to claim print for todo {} \"{}\": {}", id, item.title, e);
                continue;
            }
        }

        let reason = if stored.is_none() { "first print" } else { "content changed" };
        info!("Printing todo {} \"{}\" ({})", id, item.title, reason);

        match print_ticket(item).await {
            Ok(()) => {
                info!("Ticket printed for todo {} \"{}\"", id, item.title);
            }
            Err(e) => {
                warn!("Failed to print todo {} \"{}\": {}", id, item.title, e);
                // Undo the claim so the next poll retries instead of silently
                // treating this task as already printed.
                if let Err(e2) = db::printed_at_delete(id).await {
                    warn!("Failed to revert print claim for todo {}: {}", id, e2);
                }
            }
        }
    }

    Ok(())
}

/// Starts the print monitor as a background task.
///
/// Runs an initial sync+poll immediately, then repeats every `interval_secs`
/// seconds. Errors within a pass are logged and do not stop the loop.
///
/// Each iteration syncs `todo_cache` against the live backend *before*
/// polling for print-worthy changes — `poll()` reads via `crate::read_items()`,
/// which (once the cache read path is live) serves `todo_cache` directly, so
/// the sync has to run first each pass or `poll()` would be comparing
/// against data that's already one interval stale relative to itself.
pub async fn run(interval_secs: u64) {
    info!(
        "Print monitor started (interval: {}s, suppress label: \"{}\")",
        interval_secs, NO_PRINT_LABEL
    );

    loop {
        if let Err(e) = crate::sync_cache().await {
            warn!("Todo cache sync error: {}", e);
        }
        if let Err(e) = poll().await {
            warn!("Print monitor poll error: {}", e);
        }
        sleep(Duration::from_secs(interval_secs)).await;
    }
}
