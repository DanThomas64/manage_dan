pub mod project_error;
pub mod project_prelude;

use crate::project_prelude::*;

pub fn init() -> ProjectLibResult {
    info!("initializing project");
    // Err(ProjectLibError::CannotInitialize(
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
