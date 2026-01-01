use thiserror::Error;

pub type TodoLibResult<T = ()> = Result<T, TodoLibError>;

#[derive(Error, Debug)]
pub enum TodoLibError {
    #[error("unable to initialize todo system: {0}")]
    CannotInitialize(String),

    #[error("unknown todo error")]
    Unknown,
}
