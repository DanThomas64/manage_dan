pub mod tasks_error;
pub mod tasks_prelude;

use crate::tasks_prelude::*;

pub fn init() -> TasksLibResult {
    info!("initializing tasks");
    // Err(TasksLibError::CannotInitialize(
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
