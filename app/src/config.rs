//! Application configuration management.
//!
//! This module handles loading configuration settings from environment variables
//! and configuration files, providing global access to the settings.

use anyhow::Result;
use config::{Config, Environment, File};
use serde::Deserialize;
use std::sync::OnceLock;

/// Static storage for the application configuration, initialized once.
static APP_CONFIG: OnceLock<AppConfig> = OnceLock::new();

/// Configuration specific to the USB printer device.
#[derive(Debug, Deserialize, Clone)]
pub struct PrinterConfig {
    pub vendor_id: u16,
    pub product_id: u16,
    /// Backend mode: `"usb"` for physical printer, `"terminal"` for stdout rendering.
    pub mode: String,
    /// Number of characters that fit on one line of the physical receipt.
    /// Common values: 42 (default ESC/POS) or 48.  Check your printer's spec sheet.
    pub characters_per_line: u8,
}

/// Configuration for the Vikunja task management backend.
#[derive(Debug, Deserialize, Clone)]
pub struct VikunjaConfig {
    pub base_url: String,
    pub api_token: String,
    pub project_id: i64,
}

/// File logging configuration.
///
/// Override with `APP_LOGGING_FILE=/path/to/app.log` env var, or set
/// `[logging] file = "..."` in a config TOML file.
#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    /// Path to the log file.  Relative paths are resolved from the process
    /// working directory.  The parent directory is created automatically.
    pub file: String,
}

/// Notes subsystem configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct NotesConfig {
    /// Directory where .md note files are stored.
    pub dir: String,
}

/// Global application configuration structure.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub environment: String,
    pub printer: PrinterConfig,
    pub vikunja: VikunjaConfig,
    /// How often the print monitor polls Vikunja for labelled tasks (seconds).
    pub monitor_interval_secs: u64,
    /// Local hour (0–23) at which the daily summary is printed.
    pub summary_hour: u32,
    /// Detail level for the daily summary: "minimal", "standard", or "full".
    pub summary_level: String,
    /// Local hour (0–23) at which the completed-task summary is printed.
    pub completed_summary_hour: u32,
    /// Whether the end-of-day completed-task summary is enabled.
    pub completed_summary_enabled: bool,
    /// File logging settings.
    pub logging: LoggingConfig,
    /// Notes subsystem settings.
    pub notes: NotesConfig,
}

impl AppConfig {
    /// Loads the configuration from environment variables and configuration files.
    ///
    /// Config files are loaded from the directory specified by `APP_CONFIG_DIR`
    /// (default: `"config"`).  This lets Docker bind-mount the project's
    /// `config/` directory at an absolute path without creating any structure
    /// inside `./data/`.
    pub fn load() -> Result<AppConfig> {
        // Read config directory from env directly (bypasses the crate separator logic).
        let cfg_dir = std::env::var("APP_CONFIG_DIR").unwrap_or_else(|_| "config".to_string());
        let env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());

        let config_builder = Config::builder()
            // 1. Load defaults
            .set_default("environment", "development")?
            .set_default("printer.vendor_id", 0x0fe6)?
            .set_default("printer.product_id", 0x811e)?
            .set_default("printer.mode", "terminal")?
            .set_default("printer.characters_per_line", 42u64)?

            // Vikunja defaults
            .set_default("vikunja.base_url", "http://localhost:3456")?
            .set_default("vikunja.api_token", "")?
            .set_default("vikunja.project_id", 1i64)?
            .set_default("monitor_interval_secs", 30u64)?
            .set_default("summary_hour", 8u64)?
            .set_default("summary_level", "full")?
            .set_default("completed_summary_hour", 20u64)?
            .set_default("completed_summary_enabled", true)?
            .set_default("logging.file", "data/logs/app.log")?
            .set_default("notes.dir", "notes")?

            // 2. Load default config file
            .add_source(File::with_name(&format!("{}/default", cfg_dir)).required(false))

            // 3. Load environment-specific config
            .add_source(File::with_name(&format!("{}/{}", cfg_dir, env)).required(false))

            // 4. Load local overrides — gitignored, never committed (put secrets here)
            .add_source(File::with_name(&format!("{}/local", cfg_dir)).required(false))

            // 5. Override with environment variables (e.g., APP_VIKUNJA_API_TOKEN)
            .add_source(Environment::with_prefix("APP").separator("_"));

        let settings = config_builder.build()?;
        let app_config: AppConfig = settings.try_deserialize()?;
        
        Ok(app_config)
    }

    /// Gets the globally initialized application configuration.
    /// Panics if called before initialization in main.
    pub fn get() -> &'static AppConfig {
        APP_CONFIG.get().expect("Configuration is not initialized")
    }

    /// Initializes the global application configuration.
    ///
    /// This function should only be called once during application startup.
    pub fn init(config: AppConfig) -> Result<()> {
        APP_CONFIG
            .set(config)
            .map_err(|_| anyhow::anyhow!("Configuration already initialized"))
    }
}
