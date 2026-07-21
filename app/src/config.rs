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

/// Configuration for the todo subsystem.
#[derive(Debug, Deserialize, Clone)]
pub struct TodoConfig {
    /// nb notebook name todo items are stored in.
    pub nb_notebook: String,
}

/// Configuration for the project subsystem.
#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    /// Base directory project folders are created under. Supports a leading
    /// `~/` for the user's home directory.
    pub base_dir: String,
}

/// File logging configuration.
///
/// Override with `APP_LOGGING__FILE=/path/to/app.log` env var, or set
/// `[logging] file = "..."` in a config TOML file.
#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    /// Path to the log file.  Relative paths are resolved from the process
    /// working directory.  The parent directory is created automatically.
    pub file: String,
}

/// Global application configuration structure.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub environment: String,
    pub printer: PrinterConfig,
    pub todo: TodoConfig,
    pub project: ProjectConfig,
    /// TCP port the HTTP API server listens on. Overridable per-run (e.g. via
    /// `APP_API_PORT`) so a scratch/test instance can run alongside the real
    /// deployed service without a port conflict.
    pub api_port: u16,
    /// How often the background monitor polls `nb` for changed todos/notes (seconds).
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
}

impl AppConfig {
    /// Loads the configuration from environment variables and configuration files.
    ///
    /// Config files are loaded from the directory specified by `APP_CONFIG_DIR`
    /// (default: `"config"`, relative to the working directory).
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

            .set_default("todo.nb_notebook", "todo")?
            .set_default("project.base_dir", "~/projects")?
            .set_default("api_port", 8080u64)?
            .set_default("monitor_interval_secs", 30u64)?
            .set_default("summary_hour", 8u64)?
            .set_default("summary_level", "full")?
            .set_default("completed_summary_hour", 20u64)?
            .set_default("completed_summary_enabled", true)?
            .set_default("logging.file", "data/logs/app.log")?

            // 2. Load default config file
            .add_source(File::with_name(&format!("{}/default", cfg_dir)).required(false))

            // 3. Load environment-specific config
            .add_source(File::with_name(&format!("{}/{}", cfg_dir, env)).required(false))

            // 4. Load local overrides — gitignored, never committed (put secrets here)
            .add_source(File::with_name(&format!("{}/local", cfg_dir)).required(false))

            // 5. Override with environment variables (e.g., APP_PRINTER_MODE,
            // APP_TODO__NB_NOTEBOOK) — "__" (double underscore) is the
            // nesting separator, not "_", since most field names (nb_notebook,
            // api_port, monitor_interval_secs, ...) contain a literal
            // underscore themselves; a single-underscore separator would
            // misparse those and silently fail to override them. The prefix
            // separator (between "APP" and the rest) must be set explicitly
            // to "_" too — `Environment` otherwise defaults it to match
            // `separator` ("__"), which would require "APP__PRINTER_MODE"
            // instead of the documented "APP_PRINTER_MODE".
            .add_source(
                Environment::with_prefix("APP")
                    .prefix_separator("_")
                    .separator("__"),
            );

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

#[cfg(test)]
mod tests {
    use super::*;

    // `cargo test` runs tests in parallel threads within one process, but
    // `std::env::set_var`/`remove_var` mutate process-global state — every
    // test below sets `APP_CONFIG_DIR` (some also set other `APP_*` vars), so
    // without serializing them, one test's env vars can leak into another's
    // concurrently-running `AppConfig::load()` call. Each test acquires this
    // lock for its full duration.
    static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // Regression test for a bug where `summary_hour`/`completed_summary_hour`/
    // `monitor_interval_secs`/`summary_level`/`completed_summary_enabled` were
    // nested under `[printer]` in the TOML files, but read here as top-level
    // `AppConfig` fields — `config`-rs silently drops unrecognized nested
    // keys, so overrides for those 5 settings were never applied. Uses a
    // scratch config dir (via `APP_CONFIG_DIR`) rather than the real repo
    // config, so this doesn't depend on — or mutate — the process cwd.
    #[test]
    fn top_level_keys_load_from_default_and_local_toml() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let scratch = std::env::temp_dir().join(format!("app_config_test_{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("create scratch config dir");

        std::fs::write(
            scratch.join("default.toml"),
            r#"
                environment = "development"
                monitor_interval_secs = 30
                summary_hour = 8
                summary_level = "full"
                completed_summary_enabled = true
                completed_summary_hour = 20

                [printer]
                vendor_id = 4070
                product_id = 33054
                mode = "terminal"
                characters_per_line = 42

                [logging]
                file = "data/logs/app.log"

                [todo]
                nb_notebook = "todo"

                [project]
                base_dir = "~/projects"
            "#,
        )
        .expect("write default.toml");

        std::fs::write(
            scratch.join("local.toml"),
            r#"
                summary_hour = 7
                completed_summary_hour = 22

                [printer]
                mode = "usb"
            "#,
        )
        .expect("write local.toml");

        std::env::set_var("APP_CONFIG_DIR", &scratch);
        let config = AppConfig::load().expect("load config");
        std::env::remove_var("APP_CONFIG_DIR");
        std::fs::remove_dir_all(&scratch).ok();

        assert_eq!(config.summary_hour, 7, "local.toml's top-level summary_hour should override default.toml's");
        assert_eq!(config.completed_summary_hour, 22);
        // Sanity check: [printer]-nested keys still work as before.
        assert_eq!(config.printer.mode, "usb");
    }

    // Regression test for the env var override mechanism itself: most field
    // names contain a literal underscore (nb_notebook, api_port,
    // monitor_interval_secs, ...), so a single "_" separator can't
    // distinguish "nesting" underscores from "part of the field name"
    // underscores — it silently fails to apply the override instead of
    // erroring. "__" (double underscore) is the actual nesting separator.
    #[test]
    fn env_var_override_works_for_underscored_field_names() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let scratch = std::env::temp_dir().join(format!("app_config_env_test_{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("create scratch config dir");
        std::fs::write(
            scratch.join("default.toml"),
            r#"
                environment = "development"
                api_port = 8080
                monitor_interval_secs = 30
                summary_hour = 8
                summary_level = "full"
                completed_summary_enabled = true
                completed_summary_hour = 20

                [printer]
                vendor_id = 4070
                product_id = 33054
                mode = "terminal"
                characters_per_line = 42

                [logging]
                file = "data/logs/app.log"

                [todo]
                nb_notebook = "todo"

                [project]
                base_dir = "~/projects"
            "#,
        )
        .expect("write default.toml");

        std::env::set_var("APP_CONFIG_DIR", &scratch);
        std::env::set_var("APP_API_PORT", "9191");
        std::env::set_var("APP_TODO__NB_NOTEBOOK", "scratch_notebook");
        std::env::set_var("APP_PRINTER__MODE", "usb");
        let config = AppConfig::load().expect("load config");
        std::env::remove_var("APP_CONFIG_DIR");
        std::env::remove_var("APP_API_PORT");
        std::env::remove_var("APP_TODO__NB_NOTEBOOK");
        std::env::remove_var("APP_PRINTER__MODE");
        std::fs::remove_dir_all(&scratch).ok();

        assert_eq!(config.api_port, 9191, "APP_API_PORT should override a top-level underscored field");
        assert_eq!(config.todo.nb_notebook, "scratch_notebook", "APP_TODO__NB_NOTEBOOK should override a nested underscored field");
        assert_eq!(config.printer.mode, "usb", "APP_PRINTER__MODE should override a nested field whose leaf name has no underscore");
    }
}
