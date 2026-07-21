use thiserror::Error;

pub type TodoLibResult<T = ()> = Result<T, TodoLibError>;

#[derive(Error, Debug)]
pub enum TodoLibError {
    #[error("unable to initialize todo system: {0}")]
    CannotInitialize(String),

    #[error("todo item not found: {0}")]
    NotFound(i64),

    #[error("nb error: {0}")]
    Nb(String),

    #[error("database error: {0}")]
    Db(String),

    #[error("unknown todo error")]
    Unknown,
}
