//! Error types specific to the Lists subsystem.

use thiserror::Error;

pub type ListsLibResult<T = ()> = Result<T, ListsLibError>;

#[derive(Error, Debug)]
pub enum ListsLibError {
    #[error("unable to initialize lists system: {0}")]
    CannotInitialize(String),

    #[error("list group not found: {0}")]
    GroupNotFound(i64),

    #[error("category not found: {0}")]
    CategoryNotFound(i64),

    #[error("item not found: {0}")]
    ItemNotFound(i64),

    #[error("database error: {0}")]
    Db(#[from] db::db_error::DbLibError),

    #[error("print error: {0}")]
    Print(#[from] printer::printer_error::PrinterLibError),

    #[error("unknown lists error")]
    Unknown,
}
