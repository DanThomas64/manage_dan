//! Error types specific to the Shopping subsystem.

use thiserror::Error;

pub type ShoppingLibResult<T = ()> = Result<T, ShoppingLibError>;

#[derive(Error, Debug)]
pub enum ShoppingLibError {
    #[error("unable to initialize shopping system: {0}")]
    CannotInitialize(String),

    #[error("category not found: {0}")]
    CategoryNotFound(i64),

    #[error("item not found: {0}")]
    ItemNotFound(i64),

    #[error("database error: {0}")]
    Db(#[from] db::db_error::DbLibError),

    #[error("print error: {0}")]
    Print(#[from] printer::printer_error::PrinterLibError),

    #[error("unknown shopping error")]
    Unknown,
}
