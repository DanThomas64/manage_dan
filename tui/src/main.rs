//! Terminal User Interface (TUI) client for interacting with the application API.
//!
//! # Configuration
//!
//! | Env var           | Default              | Purpose                        |
//! |-------------------|----------------------|--------------------------------|
//! | `MANAGE_API_URL`  | `http://localhost`   | Base URL of the backend API    |
//! | `TUI_LOG_FILE`    | `tui.log`            | Path to the JSON log file      |
//! | `LOG_STDOUT`      | `false`              | Set to `true` to echo to stdout|

use anyhow::Result;
use tracing::info;
use tracing_subscriber::{
    fmt::{time::ChronoLocal, Layer},
    layer::{Layer as LayerExt, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter, Registry,
};
use std::fs::OpenOptions;
use tracing::Level;

mod api;
mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    let log_file = std::env::var("TUI_LOG_FILE").unwrap_or_else(|_| "tui.log".to_string());
    init_logging(&log_file);

    let api_url = std::env::var("MANAGE_API_URL")
        .unwrap_or_else(|_| "http://localhost".to_string());

    info!(api_url, "TUI starting");

    ui::run_tui().await
}

fn init_logging(log_file: &str) {
    // Ensure parent directory exists.
    let path = std::path::Path::new(log_file);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    let file = match OpenOptions::new().append(true).create(true).open(log_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open TUI log file '{}': {}", log_file, e);
            return;
        }
    };

    let (non_blocking, guard) = tracing_appender::non_blocking(file);
    // Leak the guard so the background I/O thread stays alive for the whole process.
    Box::leak(Box::new(guard));

    let file_filter = EnvFilter::new("debug")
        .add_directive("hyper_util::client::legacy::pool=info".parse().unwrap())
        .add_directive("hyper::proto::h1::io=warn".parse().unwrap())
        .add_directive("hyper::proto::h1::conn=warn".parse().unwrap());

    let file_layer = Layer::new()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_timer(ChronoLocal::rfc_3339())
        .with_line_number(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_level(true)
        .json()
        .with_filter(file_filter);

    // Stdout logging is off by default — the TUI owns the terminal.
    // Set LOG_STDOUT=true to echo INFO+ events to stdout (useful before the TUI starts).
    let show_stdout = std::env::var("LOG_STDOUT")
        .map(|v| v.to_ascii_lowercase() == "true")
        .unwrap_or(false);

    let stdout_layer = if show_stdout {
        let stdout_filter = EnvFilter::from_default_env()
            .add_directive(Level::INFO.into());
        Some(
            Layer::new()
                .with_writer(std::io::stdout)
                .with_ansi(true)
                .with_timer(ChronoLocal::rfc_3339())
                .with_target(true)
                .with_filter(stdout_filter),
        )
    } else {
        None
    };

    let subscriber = Registry::default()
        .with(file_layer)
        .with(stdout_layer);

    if let Err(e) = subscriber.try_init() {
        eprintln!("Warning: failed to set tracing subscriber: {}", e);
    }
}
