//! Centralized application error handling.
//!
//! This module defines the top-level `AppError` enum which aggregates errors
//! from various internal libraries (Db, Log, Notes, Project, Printer, Todo).

use crate::prelude::*;

/// A specialized `Result` type for application operations.
pub type AppResult<T = ()> = Result<T, AppError>;

/// The main application error type, wrapping errors from underlying systems.
#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Db(#[from] DbLibError),
    #[error(transparent)]
    Log(#[from] LogLibError),
    #[error(transparent)]
    Notes(#[from] NotesLibError),
    #[error(transparent)]
    Project(#[from] ProjectLibError),
    #[error(transparent)]
    Printer(#[from] PrinterLibError),
    #[error(transparent)]
    Todo(#[from] TodoLibError),

    #[error("systemstatus monitor has failed: {0}")]
    SystemStatusMonitorFail(String),

    #[error("unknown app error")]
    Unknown,
}

impl AppError {
    /// Prints the error details using the application's logging system.
    pub fn print(&self) {
        error!("error details: {:0}", self)
    }
}
