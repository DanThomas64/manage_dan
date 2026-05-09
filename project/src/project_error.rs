//! Error types specific to the Project subsystem.

use crate::project_prelude::*;

/// A specialized `Result` type for Project operations.
pub type ProjectLibResult<T = ()> = Result<T, ProjectLibError>;

/// Errors that can occur during Project system operations.
#[derive(Error, Debug)]
pub enum ProjectLibError {
    #[error("unable to initialize project system: {0}")]
    CannotInitialize(String),

    #[error("unknown project error")]
    Unknown,
}
