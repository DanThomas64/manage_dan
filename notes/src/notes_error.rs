use crate::notes_prelude::*;

pub type NotesLibResult<T = ()> = Result<T, NotesLibError>;

#[derive(Error, Debug)]
pub enum NotesLibError {
    #[error("unable to initialize notes system: {0}")]
    CannotInitialize(String),

    #[error("note not found: {0}")]
    NotFound(String),

    #[error("invalid frontmatter in file: {0}")]
    InvalidFrontmatter(String),

    #[error("database error: {0}")]
    Db(#[from] db::db_error::DbLibError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("print error: {0}")]
    Print(#[from] printer::printer_error::PrinterLibError),

    #[error("unknown notes error")]
    Unknown,
}
