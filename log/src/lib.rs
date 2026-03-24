//! Application logging system initialization and configuration.
//!
//! This crate sets up structured logging using `tracing`, configuring output
//! to both rotating files and the application database.
//!
//! # File logging
//!
//! Log files are written as newline-delimited JSON to `<log_dir>/app.log.<date>`.
//! A new file is started each day (`Rotation::DAILY`).  The directory is created
//! automatically if it does not exist.
//!
//! The `WorkerGuard` returned by `tracing_appender::non_blocking` is leaked so
//! it lives for the entire process lifetime — this keeps the background I/O
//! thread alive and ensures buffered log lines are flushed on shutdown.
//!
//! # Environment overrides
//!
//! * `RUST_LOG` — standard `EnvFilter` directive (e.g. `debug`, `my_crate=trace`)
//! * `LOG_STDOUT=false` — suppress the stdout layer (useful in production / Docker)

pub mod db_subscriber;
pub mod log_error;
pub mod log_prelude;

use crate::db_subscriber::DbLayer;
use crate::log_error::LogLibError;
use crate::log_prelude::*;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::{time::ChronoLocal, Layer},
    layer::{Layer as LayerExt, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter, Registry,
};
use tokio::runtime::Handle;
use tracing::Level;
use std::sync::Once;

static LOG_INIT: Once = Once::new();

/// Initialises the logging system.
///
/// * `log_dir` — directory where rotating log files are written.
///   Created automatically if it does not exist.
///
/// Must be called from within a Tokio runtime (i.e. after `#[tokio::main]`).
pub fn init(log_dir: &str) -> LogLibResult {
    let mut result = Ok(());

    // log_dir must be captured by value for the `call_once` closure.
    let log_dir = log_dir.to_string();

    LOG_INIT.call_once(|| {
        // ── 0. Ensure the log directory exists ───────────────────────────────
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            result = Err(LogLibError::CannotInitialize(format!(
                "Failed to create log directory '{}': {}",
                log_dir, e
            )));
            return;
        }

        // ── 1. File logging layer (JSON, daily rotation) ─────────────────────
        //
        // IMPORTANT: `non_blocking` returns a `WorkerGuard` that keeps the
        // background flushing thread alive.  If the guard is dropped, the thread
        // exits immediately and all buffered writes are silently lost.  We call
        // `Box::leak` to promote the guard to `'static` so it is never dropped.
        let file_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "app.log");
        let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);
        // Leak the guard — intentional, keeps the I/O thread alive for the
        // entire process lifetime.
        Box::leak(Box::new(guard));

        let file_filter = EnvFilter::new("debug")
            .add_directive("hyper::proto::io=info".parse().unwrap())
            .add_directive("hyper::proto::h1::io=warn".parse().unwrap())
            .add_directive("hyper::proto::h1::conn=warn".parse().unwrap())
            .add_directive("tokio_util::codec::framed_impl=info".parse().unwrap());

        let file_layer = Layer::new()
            .with_writer(non_blocking_appender)
            .with_ansi(false)
            .with_timer(ChronoLocal::rfc_3339())
            .with_line_number(true)
            .with_target(true)
            .with_thread_ids(true)
            .with_level(true)
            .json()
            .with_filter(file_filter);

        // ── 2. Database logging layer ─────────────────────────────────────────
        let db_layer = DbLayer;

        let handle = match Handle::try_current() {
            Ok(h) => h,
            Err(e) => {
                result = Err(LogLibError::CannotInitialize(format!(
                    "Failed to get Tokio runtime handle: {}",
                    e
                )));
                return;
            }
        };
        if let Err(e) = db_subscriber::init_runtime_handle(handle) {
            result = Err(LogLibError::CannotInitialize(format!(
                "Failed to initialize DB subscriber runtime handle: {}",
                e
            )));
            return;
        }

        // ── 3. Stdout layer (suppressible via LOG_STDOUT=false) ───────────────
        let show_stdout = std::env::var("LOG_STDOUT")
            .map(|v| v.to_ascii_lowercase() != "false")
            .unwrap_or(true);

        // ── 4. Combine layers and initialise the global subscriber ────────────
        //
        // `tracing_subscriber` requires layers to be the same type when passed
        // to `.with()`.  We use `Option` boxing to conditionally include the
        // stdout layer without resorting to an `if`-branch that changes the
        // subscriber type.
        let stdout_layer = if show_stdout {
            let stdout_filter = EnvFilter::from_default_env()
                .add_directive(Level::INFO.into())
                .add_directive("hyper::proto::io=warn".parse().unwrap())
                .add_directive("hyper::proto::h1::io=warn".parse().unwrap())
                .add_directive("hyper::proto::h1::conn=warn".parse().unwrap())
                .add_directive("tokio_util::codec::framed_impl=warn".parse().unwrap());

            Some(
                Layer::new()
                    .with_writer(std::io::stdout)
                    .with_ansi(true)
                    .with_timer(ChronoLocal::rfc_3339())
                    .with_line_number(true)
                    .with_target(true)
                    .with_filter(stdout_filter),
            )
        } else {
            None
        };

        let subscriber = Registry::default()
            .with(file_layer)
            .with(db_layer)
            .with(stdout_layer);

        if let Err(e) = subscriber.try_init() {
            eprintln!("Warning: Failed to set global tracing subscriber: {}", e);
        }
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let result = init("/tmp/manage_dan_test_logs");
        assert!(result.is_ok());
    }
}
