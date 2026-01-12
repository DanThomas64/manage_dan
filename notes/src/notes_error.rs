//! Error types specific to the Notes subsystem.

use crate::notes_prelude::*;

/// A specialized `Result` type for Notes operations.
pub type NotesLibResult<T = ()> = Result<T, NotesLibError>;

/// Errors that can occur during Notes system operations.
#[derive(Error, Debug)]
pub enum NotesLibError {
    #[error("unable to initialize notes system: {0}")]
    CannotInitialize(String),

    #[error("unknown notes error")]
    Unknown,
}
