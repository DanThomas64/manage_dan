pub mod db_subscriber;
pub mod log_error;
pub mod log_prelude;

use crate::db_subscriber::DbLayer; // <-- Removed 'self'
use crate::log_error::LogLibError;
use crate::log_prelude::*;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::{time::ChronoLocal, Layer},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Registry,
};
use tokio::runtime::Handle; // Required for capturing the runtime handle
use tracing::Level; // Required for Level::DEBUG.into()

/// Initializes the logging system: file rotation and database logging.
pub fn init() -> LogLibResult {
    // 1. File Logging Setup (app.log, rotating)
    let file_appender = RollingFileAppender::new(Rotation::NEVER, ".", "app.log");
    let (non_blocking_appender, _guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = Layer::new()
        .with_writer(non_blocking_appender)
        .with_ansi(false)
        .with_timer(ChronoLocal::rfc_3339())
        .with_line_number(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_level(true)
        .json(); // Use JSON format for structured logging in files

    // 2. Database Logging Layer
    let db_layer = DbLayer;

    // 3. Initialize global runtime handle for DbLayer
    // We must be running inside a Tokio runtime context when log::init() is called.
    let handle = Handle::try_current().map_err(|e| {
        LogLibError::CannotInitialize(format!("Failed to get Tokio runtime handle: {}", e))
    })?;
    db_subscriber::init_runtime_handle(handle).map_err(|e| {
        LogLibError::CannotInitialize(format!("Failed to initialize DB subscriber runtime handle: {}", e))
    })?;


    // 4. Console/Stdout Layer (for immediate feedback during development/debugging)
    let stdout_layer = Layer::new()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_timer(ChronoLocal::rfc_3339())
        .with_line_number(true)
        .with_target(true);

    // 5. Combine layers and initialize tracing subscriber
    // Use EnvFilter to allow configuration via RUST_LOG environment variable, defaulting to DEBUG
    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env().add_directive(Level::DEBUG.into()))
        .with(file_layer)
        .with(db_layer)
        .with(stdout_layer);

    // Initialize the global default subscriber using try_init() to handle errors gracefully.
    // try_init() returns Result<(), SetGlobalDefaultError>
    if let Err(e) = subscriber.try_init() {
        // If setting fails, it usually means it was already set.
        eprintln!("Warning: Failed to set global default tracing subscriber: {}", e);
    }
    
    info!("initializing log system");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level; // Need Level for test setup

    #[test]
    fn it_works() {
        // Note: This test now requires a Tokio runtime to be running.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        
        let result = init();
        assert!(result.is_ok());
    }
}
