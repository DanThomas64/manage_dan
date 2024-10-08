#![crate_name = "app"]

use db;
use log;
use notes;
use project;
use tasks;
use todo;

pub mod error;
pub mod macros;
pub mod nogo;
pub mod prelude;

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
    let _ = SystemsStatus::init().await;
    Ok(())
}
