use thiserror::Error;
use escpos::errors::PrinterError as EscposError;

pub type PrinterLibResult<T = ()> = Result<T, PrinterLibError>;

#[derive(Error, Debug)]
pub enum PrinterLibError {
    #[error("unable to initialize printer system: {0}")]
    CannotInitialize(String),

    #[error("unknown printer error")]
    Unknown,

    #[error(transparent)]
    Escpos(#[from] EscposError),
}
