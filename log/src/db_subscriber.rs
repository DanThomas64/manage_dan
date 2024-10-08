use db::log_event;
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

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
        if event.metadata().level() < &tracing::Level::INFO {
            return;
        }

        let mut visitor = LogVisitor::new();
        event.record(&mut visitor);

        let level = event.metadata().level().to_string();
        let target = event.metadata().target().to_string();
        let message = visitor.message.unwrap_or_else(|| "No message".to_string());

        // We need to spawn an async task to handle the DB insertion
        // Since this is running inside a tracing subscriber hook, we must ensure
        // we have a runtime available. `tokio::spawn` is appropriate here.
        tokio::spawn(async move {
            if let Err(e) = log_event(&level, &target, &message).await {
                // If DB logging fails, we should log this failure somewhere else,
                // but since we are inside the logging system, we'll just print to stderr
                // or use a standard library print for safety.
                eprintln!("Failed to log event to database: {:?}", e);
            }
        });
    }
}
