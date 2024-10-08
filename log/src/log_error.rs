use crate::log_prelude::*;

pub type LogLibResult<T = ()> = Result<T, LogLibError>;

#[derive(Error, Debug)]
pub enum LogLibError {
    #[error("unable to initialize log system: {0}")]
    CannotInitialize(String),

    #[error("unknown log error")]
    Unknown,
}
