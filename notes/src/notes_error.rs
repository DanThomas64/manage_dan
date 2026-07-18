use crate::notes_prelude::*;

pub type NotesLibResult<T = ()> = Result<T, NotesLibError>;

#[derive(Error, Debug)]
pub enum NotesLibError {
    #[error("nb is not installed or not in PATH")]
    NbNotInstalled,

    #[error("nb command failed: {0}")]
    Nb(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("note not found: {0}")]
    NotFound(String),

    #[error("unable to initialize notes system: {0}")]
    CannotInitialize(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("print error: {0}")]
    Print(#[from] printer::printer_error::PrinterLibError),

    #[error("database error: {0}")]
    Db(String),
}
