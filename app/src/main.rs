//! The main application server executable.
//!
//! This application initializes all necessary subsystems (database, logging, printer, etc.),
//! monitors their status, and starts the HTTP API server.

use nogo::{SystemsGoNogo, Status};

pub mod config;
pub mod error;
pub mod macros;
pub mod nogo;
pub mod prelude;
pub mod api;
mod test;

use crate::prelude::*;

/// Main Function of the app
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load configuration
    let config = AppConfig::load()?;
    AppConfig::init(config)?;

    // 2. Initialize all systems, including logging and database setup in the correct order.
    let mut systems = SystemsStatus::new();
    let systems = systems.init();

    info!("Application starting up...");
    let cfg_dir = std::env::var("APP_CONFIG_DIR").unwrap_or_else(|_| "config".to_string());
    info!("Local config: {}/local.toml", cfg_dir);

    // Setup monitoring and get final status
    let mut go_nogo = SystemsGoNogo::new();

    // Calculate initial status synchronously
    go_nogo.calculate_initial_status(systems);

    debug!("We are getting here now!");

    // 4. Gather stats and report initialization status via printer.
    let config = AppConfig::get();
    let vid = config.printer.vendor_id;
    let pid = config.printer.product_id;

    // Fetch stats concurrently; failures produce placeholder text rather than
    // blocking or crashing the startup report.
    let (todo_summary, list_stats) = tokio::join!(
        todo::get_summary(),
        lists::stats(),
    );
    let recurring_total = todo::recurring::load_config().len();
    let recurring_today = todo::recurring::due_today().len();

    let version = env!("CARGO_PKG_VERSION");
    let now = chrono::Local::now();

    // --- Subsystem status lines ---
    let mut lines: Vec<String> = systems.iter()
        .map(|(name, status)| format!("  {:<9} {:?}", format!("{}:", name), status))
        .collect();

    // --- Stats section ---
    lines.push(String::new());
    lines.push("-".repeat(printer::line_width()));

    match &todo_summary {
        Ok(s) => {
            lines.push(format!(
                "  Todo:      {} pending ({} overdue, {} today)",
                s.total_pending, s.overdue, s.due_today
            ));
        }
        Err(_) => lines.push("  Todo:      unavailable".to_string()),
    }

    match &list_stats {
        Ok(s) => {
            lines.push(format!(
                "  Lists:     {} lists across {} groups, {} items ({} pending)",
                s.lists, s.groups, s.items, s.items_pending
            ));
        }
        Err(_) => lines.push("  Lists:     unavailable".to_string()),
    }

    lines.push(format!(
        "  Recurring: {} configured ({} due today)",
        recurring_total, recurring_today
    ));

    lines.push(String::new());
    lines.push("-".repeat(printer::line_width()));
    lines.push(format!(
        "  v{}  |  {}",
        version,
        now.format("%a %d %b %Y  %H:%M")
    ));

    let title = match go_nogo.gono {
        Status::Go => {
            info!("All systems initialized successfully (GO). Printing status report.");
            "SYSTEM INITIALIZED: GO".to_string()
        }
        Status::Degraded => {
            warn!("Systems initialized with DEGRADED status. Printing status report.");
            "SYSTEM INITIALIZED: DEGRADED".to_string()
        }
        Status::Nogo => {
            error!("Systems failed initialization (NOGO). Printing status report.");
            "SYSTEM INITIALIZED: NOGO".to_string()
        }
        _ => {
            error!("Systems initialized with UNKNOWN status. Printing status report.");
            "SYSTEM INITIALIZED: UNKNOWN".to_string()
        }
    };

    let job = printer::PrintJob::new("App Initialization".to_string(), title, lines);
    if let Err(e) = job.execute(vid, pid).await {
        error!("Failed to print initialization status: {}", e);
    }

    // 5. Start system status monitoring loop in the background
    go_nogo.start_monitoring(systems);

    // 5b. Start the print monitor — polls all Vikunja projects for "print"-labelled tasks
    // (and, as of the todo_cache/note_cache read-cache, reconciles that cache
    // against the live backend each pass too — see todo::monitor's doc comment).
    let interval = AppConfig::get().monitor_interval_secs;
    tokio::spawn(todo::monitor::run(interval));
    tokio::spawn(notes::monitor::run(interval));

    // 5c. Print recurring task tickets and the daily summary at startup
    //     (each skipped if already printed today), then schedule the daily run.
    todo::recurring::print_due_today_if_not_printed().await;
    let summary_level = todo::daily_summary::SummaryLevel::from_config_str(&AppConfig::get().summary_level);
    todo::daily_summary::print_summary_if_not_today(summary_level).await;
    let summary_hour = AppConfig::get().summary_hour;
    tokio::spawn(todo::daily_summary::run(summary_hour, summary_level));

    if AppConfig::get().completed_summary_enabled {
        let completed_hour = AppConfig::get().completed_summary_hour;
        tokio::spawn(todo::completed_summary::run(completed_hour));
    }

    // 6. Start the HTTP API server
    info!("Application initialized. Starting API server.");

    // Note: We pass copies of the initial status structs.
    // A future improvement would be to use a shared state (Arc<Mutex<...>>)
    // so the API can report real-time status updates from the monitoring loop.
    api::start_server(systems, go_nogo).await;

    info!("API server shut down. Application exiting.");

    Ok(())
}
