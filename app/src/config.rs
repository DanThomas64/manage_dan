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
}

/// Configuration for the Vikunja task management backend.
#[derive(Debug, Deserialize, Clone)]
pub struct VikunjaConfig {
    pub base_url: String,
    pub api_token: String,
    pub project_id: i64,
}

/// Global application configuration structure.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub environment: String,
    pub printer: PrinterConfig,
    pub vikunja: VikunjaConfig,
    /// How often the print monitor polls Vikunja for labelled tasks (seconds).
    pub monitor_interval_secs: u64,
}

impl AppConfig {
    /// Loads the configuration from environment variables and configuration files.
    pub fn load() -> Result<AppConfig> {
        let config_builder = Config::builder()
            // 1. Load defaults
            .set_default("environment", "development")?
            .set_default("printer.vendor_id", 0x0fe6)?
            .set_default("printer.product_id", 0x811e)?
            .set_default("printer.mode", "terminal")?
            
            // Vikunja defaults
            .set_default("vikunja.base_url", "http://localhost:3456")?
            .set_default("vikunja.api_token", "")?
            .set_default("vikunja.project_id", 1i64)?
            .set_default("monitor_interval_secs", 30u64)?

            // 2. Load configuration file (e.g., config/default.toml)
            .add_source(File::with_name("config/default").required(false))
            
            // 3. Load environment-specific configuration (e.g., config/development.toml)
            .add_source(File::with_name(&format!("config/{}", std::env::var("APP_ENV").unwrap_or_else(|_| "development".into()))).required(false))

            // 4. Load local overrides — gitignored, never committed (put secrets here)
            .add_source(File::with_name("config/local").required(false))

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
