pub mod notes_error;
pub mod notes_prelude;

use crate::notes_prelude::*;

pub fn init() -> NotesLibResult {
    info!("initializing notes");
    // Err(NotesLibError::CannotInitialize(
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
