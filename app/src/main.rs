#![crate_name = "app"]

use db;
use log;
use nogo::{SystemsGoNogo, Status};
use notes;
use project;
use printer;
use todo;
use tokio::time::{sleep, Duration};

pub mod config;
pub mod error;
pub mod macros;
pub mod nogo;
pub mod prelude;
pub mod api; // New API module
// pub mod ui; // Removed TUI module
mod test;

use crate::prelude::*;

/// Main Function of the app
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load configuration
    let config = AppConfig::load()?;
    AppConfig::init(config)?;

    // 3. initialize all systems
    let systems = SystemsStatus::new().init();
    
    // Setup monitoring and get final status
    let mut go_nogo = SystemsGoNogo::new();
    
    // Calculate initial status synchronously
    go_nogo.calculate_initial_status(systems);

    debug!("We are getting here now!");

    // 4. Report initialization status via printer if successful
    // Configuration values (vid/pid) are no longer needed here as the printer is initialized globally
    // during SystemsStatus::init and PrintJob::execute uses the global instance.
    let config = AppConfig::get();
    let vid = config.printer.vendor_id;
    let pid = config.printer.product_id;

    let status_lines: Vec<String> = systems.iter().map(|(name, status)| {
        format!("{}: {:?}", name, status)
    }).collect();

    let (title, lines) = match go_nogo.gono {
        Status::Go => {
            info!("All systems initialized successfully (GO). Printing status report.");
            (
                "SYSTEM INITIALIZED: GO".to_string(),
                status_lines,
            )
        }
        Status::Degraded => {
            warn!("Systems initialized with DEGRADED status. Printing status report.");
            (
                "SYSTEM INITIALIZED: DEGRADED".to_string(),
                status_lines,
            )
        }
        Status::Nogo => {
            error!("Systems failed initialization (NOGO). Printing status report.");
            (
                "SYSTEM INITIALIZED: NOGO".to_string(),
                status_lines,
            )
        }
        _ => {
            error!("Systems initialized with UNKNOWN status. Printing status report.");
            (
                "SYSTEM INITIALIZED: UNKNOWN".to_string(),
                status_lines,
            )
        }
    };

    let job = printer::PrintJob::new(
        "App Initialization".to_string(),
        title,
        lines,
    );

    // Execute the print job asynchronously. We still pass vid/pid to satisfy the signature,
    // but they are ignored internally by printer::PrintJob::execute.
    if let Err(e) = job.execute(vid, pid).await {
        error!("Failed to print initialization status: {}", e);
    }

    // 5. Start monitoring loop in the background
    go_nogo.start_monitoring(systems);

    // 6. Start the HTTP API server
    info!("Application initialized. Starting API server.");
    
    // Note: We pass copies of the initial status structs. 
    // A future improvement would be to use a shared state (Arc<Mutex<...>>) 
    // so the API can report real-time status updates from the monitoring loop.
    api::start_server(systems, go_nogo).await;

    info!("API server shut down. Application exiting.");
    
    Ok(())
}
