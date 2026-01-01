use anyhow::Result;
mod ui;
mod api;

#[tokio::main]
async fn main() -> Result<()> {
    // Note: TUI application does not initialize subsystems (db, log, printer, etc.)
    // It relies entirely on the main 'app' executable running the API server.
    
    println!("Starting TUI client. Ensure the main application server is running on 127.0.0.1:8080...");
    
    ui::run_tui().await
}
