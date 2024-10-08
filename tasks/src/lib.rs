pub mod printer_error;
pub mod printer_prelude;

use crate::printer_prelude::*;

pub fn init() -> PrinterLibResult {
    info!("initializing printer");
    // Err(PrinterLibError::CannotInitialize(
    //     "i am a failure".to_string(),
    // ))
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = init();
        assert!(result.is_ok());
    }
}
