//! Error types specific to Todo operations within the database layer.

use thiserror::Error;

/// A specialized `Result` type for Todo database operations.
pub type TodoLibResult<T = ()> = Result<T, TodoLibError>;

/// Errors that can occur during Todo item manipulation in the database.
#[derive(Error, Debug)]
pub enum TodoLibError {
    #[error("unable to initialize todo system: {0}")]
    CannotInitialize(String),

    #[error("unknown todo error")]
    Unknown,
}
