pub mod log_error;
pub mod log_prelude;

use crate::log_prelude::*;

pub fn init() -> LogLibResult {
    info!("initializing log");
    // Err(LogLibError::CannotInitialize("i am a failure".to_string()))
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
