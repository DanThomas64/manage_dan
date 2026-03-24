//! Terminal User Interface (TUI) client for interacting with the application API.
//!
//! This client provides a text-based interface for monitoring system status and
//! managing Todo items.
//!
//! # Configuration
//!
//! The API server URL is read from the `MANAGE_API_URL` environment variable.
//! If unset it defaults to `http://127.0.0.1:8080`.
//!
//! | Scenario                   | Command                                              |
//! |----------------------------|------------------------------------------------------|
//! | Local `cargo run -p app`   | `cargo run -p tui`                                   |
//! | Docker Compose (port 80)   | `MANAGE_API_URL=http://localhost cargo run -p tui`   |
//! | Docker Compose (port 8080) | `MANAGE_API_URL=http://localhost:8080 cargo run -p tui` |

use anyhow::Result;
mod ui;
mod api;

#[tokio::main]
async fn main() -> Result<()> {
    let api_url = std::env::var("MANAGE_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());

    println!("Starting TUI — connecting to {api_url}");

    ui::run_tui().await
}
