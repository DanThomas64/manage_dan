//! Background cache-sync monitor.
//!
//! Periodically reconciles `note_cache` against the live `nb` notebooks —
//! see [`crate::sync_cache`] for what a pass actually does (mtime-skip for
//! unchanged notes, deletion detection for notes removed externally).
//! Notes has no other background task, so unlike `todo::monitor` (which
//! layers a print-check on top of its own sync pass), this loop only syncs.

use tracing::{info, warn};
use tokio::time::{sleep, Duration};

/// Starts the cache-sync monitor as a background task.
///
/// Runs an initial sync immediately, then repeats every `interval_secs`
/// seconds. Errors within a pass are logged and do not stop the loop.
pub async fn run(interval_secs: u64) {
    info!("Notes cache sync monitor started (interval: {}s)", interval_secs);

    loop {
        if let Err(e) = crate::sync_cache().await {
            warn!("Notes cache sync error: {}", e);
        }
        sleep(Duration::from_secs(interval_secs)).await;
    }
}
