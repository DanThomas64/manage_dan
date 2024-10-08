use crate::db_prelude::*;

pub type DbLibResult<T = ()> = Result<T, DbLibError>;

#[derive(Error, Debug)]
pub enum DbLibError {
    #[error("unable to initialize db system: {0}")]
    CannotInitialize(String),

    #[error("unknown database error")]
    Unknown,
}
