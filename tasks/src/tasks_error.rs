use crate::tasks_prelude::*;

pub type TasksLibResult<T = ()> = Result<T, TasksLibError>;

#[derive(Error, Debug)]
pub enum TasksLibError {
    #[error("unable to initialize tasks system: {0}")]
    CannotInitialize(String),

    #[error("unknown project error")]
    Unknown,
}
