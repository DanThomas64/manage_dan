//! Error types specific to the Printer subsystem.

use thiserror::Error;
use escpos::errors::PrinterError as EscposError;

/// A specialized `Result` type for Printer operations.
pub type PrinterLibResult<T = ()> = Result<T, PrinterLibError>;

/// Errors that can occur during printer communication or initialization.
#[derive(Error, Debug)]
pub enum PrinterLibError {
    #[error("unable to initialize printer system: {0}")]
    CannotInitialize(String),

    #[error("unknown printer error")]
    Unknown,

    #[error(transparent)]
    Escpos(#[from] EscposError),
}
