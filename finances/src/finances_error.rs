//! Error types specific to the Finances subsystem.

use thiserror::Error;

pub type FinancesLibResult<T = ()> = Result<T, FinancesLibError>;

#[derive(Error, Debug)]
pub enum FinancesLibError {
    #[error("hledger is not installed or not in PATH")]
    HledgerNotInstalled,

    #[error("unable to initialize finances system: {0}")]
    CannotInitialize(String),

    #[error("spending entry not found: {0}")]
    EntryNotFound(String),

    #[error("recurring item not found: {0}")]
    RecurringItemNotFound(String),

    #[error("hledger command failed: {0}")]
    Hledger(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
