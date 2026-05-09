use thiserror::Error;

pub type VikunjaResult<T = ()> = Result<T, VikunjaError>;

#[derive(Error, Debug)]
pub enum VikunjaError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("task not found: {0}")]
    NotFound(i64),

    #[error("Vikunja API error: {0}")]
    Api(String),

    #[error("Vikunja client not initialized")]
    NotInitialized,
}
