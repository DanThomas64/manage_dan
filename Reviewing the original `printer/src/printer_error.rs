use thiserror::Error;
use escpos; // <-- This imports the crate root, not the Error type

pub type PrinterLibResult<T = ()> = Result<T, PrinterLibError>;

#[derive(Error, Debug)]
pub enum PrinterLibError {
// ...
    #[error(transparent)]
    Escpos(#[from] escpos::Error), // <-- Fails because escpos::Error is not in scope
}
