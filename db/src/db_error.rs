use crate::db_prelude::*;
use rusqlite;
use tokio_rusqlite;
use thiserror::Error; // Ensure Error derive is available

pub type DbLibResult<T = ()> = Result<T, DbLibError>;

#[derive(Error, Debug)]
pub enum DbLibError {
    #[error("unable to initialize db system: {0}")]
    CannotInitialize(String),

    #[error("internal database error: {0}")]
    Internal(String), // Added Internal variant

    #[error("unknown database error")]
    Unknown,

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    TokioSqlite(#[from] tokio_rusqlite::Error),
}
