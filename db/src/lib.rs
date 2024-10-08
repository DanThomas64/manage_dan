pub mod db_error;
pub mod db_prelude;

use crate::db_prelude::*;

pub fn init() -> DbLibResult {
    info!("initializing db");
    // Err(DbLibError::CannotInitialize("i am a failure".to_string()))
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
