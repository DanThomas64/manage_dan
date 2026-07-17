//! Error types specific to the Project subsystem.

use crate::project_prelude::*;

/// A specialized `Result` type for Project operations.
pub type ProjectLibResult<T = ()> = Result<T, ProjectLibError>;

/// Errors that can occur during Project system operations.
#[derive(Error, Debug)]
pub enum ProjectLibError {
    #[error("unable to initialize project system: {0}")]
    CannotInitialize(String),

    #[error("project not found: {0}")]
    NotFound(i64),

    #[error("a project named '{0}' already exists")]
    DuplicateName(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("archiving project failed: {0}")]
    ArchiveFailed(String),

    #[error("database error: {0}")]
    Db(#[from] db::db_error::DbLibError),

    #[error("lists error: {0}")]
    Lists(#[from] lists::lists_error::ListsLibError),

    #[error("notes error: {0}")]
    Notes(#[from] notes::notes_error::NotesLibError),

    #[error("todo error: {0}")]
    Todo(#[from] todo::todo_error::TodoLibError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unknown project error")]
    Unknown,
}
