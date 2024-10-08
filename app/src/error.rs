use crate::prelude::*;

pub type AppResult<T = ()> = Result<T, AppError>;

#[derive(Error, Debug)]
pub enum AppError {
    #[error(transparent)]
    Db(#[from] DbLibError),
    #[error(transparent)]
    Log(#[from] LogLibError),
    #[error(transparent)]
    Notes(#[from] NotesLibError),
    #[error(transparent)]
    Project(#[from] ProjectLibError),
    #[error(transparent)]
    Tasks(#[from] TasksLibError),
    #[error(transparent)]
    Todo(#[from] TodoLibError),

    #[error("systemstatus monitor has failed: {0}")]
    SystemStatusMonitorFail(String),

    #[error("unknown app error")]
    Unknown,
}

impl AppError {
    pub fn print(&self) {
        error!("error details: {:0}", self)
    }
}
