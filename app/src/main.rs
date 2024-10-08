#![crate_name = "app"]

use db;
use log;
use nogo::SystemsGoNogo;
use notes;
use project;
use tasks;
use todo;

pub mod error;
pub mod macros;
pub mod nogo;
pub mod prelude;
mod test;

use crate::prelude::*;

/// Main Function of the app
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // initialize logging
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_line_number(true)
        .with_ansi(false)
        .with_timer(ChronoLocal::rfc_3339())
        .init();

    // initialize all systems
    let systems = SystemsStatus::new().init();
    // Setup monitoring
    let _ = SystemsGoNogo::new().init(systems).await;
    Ok(())
}
