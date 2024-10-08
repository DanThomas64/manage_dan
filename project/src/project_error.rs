use crate::project_prelude::*;

pub type ProjectLibResult<T = ()> = Result<T, ProjectLibError>;

#[derive(Error, Debug)]
pub enum ProjectLibError {
    #[error("unable to initialize project system: {0}")]
    CannotInitialize(String),

    #[error("unknown project error")]
    Unknown,
}
