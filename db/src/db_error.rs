use crate::db_prelude::*;
use rusqlite;
use tokio_rusqlite;

pub type DbLibResult<T = ()> = Result<T, DbLibError>;

#[derive(Error, Debug)]
pub enum DbLibError {
    #[error("unable to initialize db system: {0}")]
    CannotInitialize(String),

    #[error("unknown database error")]
    Unknown,

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    TokioSqlite(#[from] tokio_rusqlite::Error),
}
