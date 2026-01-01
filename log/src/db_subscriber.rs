use db::log_event;
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
    Level, // Explicitly import Level for clarity in filtering
};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};
use tokio::runtime::Handle;
use once_cell::sync::OnceCell;
use anyhow::Result;

/// Global storage for the Tokio runtime handle.
static RUNTIME_HANDLE: OnceCell<Handle> = OnceCell::new();

/// Initializes the global runtime handle for use by the DbLayer.
/// This must be called once during application startup.
pub fn init_runtime_handle(handle: Handle) -> Result<()> {
    RUNTIME_HANDLE
        .set(handle)
        .map_err(|_| anyhow::anyhow!("Tokio runtime handle already initialized"))
}

/// A custom visitor to extract message from tracing events.
struct LogVisitor {
    message: Option<String>,
}

impl LogVisitor {
    fn new() -> Self {
        LogVisitor { message: None }
    }
}

impl Visit for LogVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }
}

/// A custom tracing layer that sends significant events (INFO and above) to the database.
pub struct DbLayer;

impl<S> Layer<S> for DbLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Only log INFO level and above to the database (Application wide significant events)
        match event.metadata().level() {
            &Level::INFO | &Level::WARN | &Level::ERROR => {
                // Proceed with logging
            }
            _ => {
                // Filter out TRACE, DEBUG, and potentially custom levels lower than INFO
                return;
            }
        }

        let mut visitor = LogVisitor::new();
        event.record(&mut visitor);

        let level = event.metadata().level().to_string();
        let target = event.metadata().target().to_string();
        let message = visitor.message.unwrap_or_else(|| "No message".to_string());

        // FIX: Retrieve the globally stored runtime handle.
        if let Some(handle) = RUNTIME_HANDLE.get() {
            handle.spawn(async move {
                if let Err(e) = log_event(&level, &target, &message).await {
                    // If DB logging fails, we should log this failure somewhere else,
                    // but since we are inside the logging system, we'll just print to stderr
                    // or use a standard library print for safety.
                    eprintln!("Failed to log event to database: {:?}", e);
                }
            });
        } else {
            // If we cannot get the runtime handle (because init_runtime_handle wasn't called), we cannot log to DB.
            eprintln!("Warning: Failed to spawn DB logging task (Tokio runtime handle not initialized). Event: {}/{}: {}", level, target, message);
        }
    }
}
