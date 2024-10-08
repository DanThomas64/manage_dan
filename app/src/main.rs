#![crate_name = "app"]

use db;
use log;
use nogo::SystemsGoNogo;
use notes;
use project;
use printer;
use todo;

pub mod config;
pub mod error;
pub mod macros;
pub mod nogo;
pub mod prelude;
mod test;

use crate::prelude::*;

/// Main Function of the app
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load configuration
    let config = AppConfig::load()?;
    AppConfig::init(config)?;

    // 2. Initialize logging (which may use config for levels/targets, but primarily relies on tracing setup)
    log::init()?;

    // 3. initialize all systems
    let systems = SystemsStatus::new().init();
    // Setup monitoring
    let _ = SystemsGoNogo::new().init(systems).await;
    Ok(())
}
