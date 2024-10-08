pub mod todo_error;
pub mod todo_prelude;

use crate::todo_prelude::*;

pub fn init() -> TodoLibResult {
    info!("initializing todo");
    // Err(TodoLibError::CannotInitialize("i am a failure".to_string()))
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
