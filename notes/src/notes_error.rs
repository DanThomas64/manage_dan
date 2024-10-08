use crate::notes_prelude::*;

pub type NotesLibResult<T = ()> = Result<T, NotesLibError>;

#[derive(Error, Debug)]
pub enum NotesLibError {
    #[error("unable to initialize notes system: {0}")]
    CannotInitialize(String),

    #[error("unknown notes error")]
    Unknown,
}
