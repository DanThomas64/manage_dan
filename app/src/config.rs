use anyhow::Result;
use config::{Config, Environment, File};
use serde::Deserialize;
use std::sync::OnceLock;

/// Static storage for the application configuration, initialized once.
static APP_CONFIG: OnceLock<AppConfig> = OnceLock::new();

#[derive(Debug, Deserialize, Clone)]
pub struct PrinterConfig {
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub environment: String,
    pub printer: PrinterConfig,
    // Add other configuration sections here as needed
}

impl AppConfig {
    /// Loads the configuration from environment variables and configuration files.
    pub fn load() -> Result<AppConfig> {
        let config_builder = Config::builder()
            // 1. Load defaults
            .set_default("environment", "development")?
            .set_default("printer.vendor_id", 0x0fe6)?
            .set_default("printer.product_id", 0x811e)?
            
            // 2. Load configuration file (e.g., config/default.toml)
            .add_source(File::with_name("config/default").required(false))
            
            // 3. Load environment-specific configuration (e.g., config/production.toml)
            .add_source(File::with_name(&format!("config/{}", std::env::var("APP_ENV").unwrap_or_else(|_| "development".into()))).required(false))
            
            // 4. Override with environment variables (e.g., APP_PRINTER_VENDOR_ID)
            // Environment loading should still work without explicit 'env' feature in 0.15.x
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
    pub fn init(config: AppConfig) -> Result<()> {
        APP_CONFIG
            .set(config)
            .map_err(|_| anyhow::anyhow!("Configuration already initialized"))
    }
}
