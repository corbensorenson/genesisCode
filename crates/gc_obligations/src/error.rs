use thiserror::Error;

#[derive(Debug, Error)]
pub enum ObligationError {
    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("module error: {0}")]
    Module(String),

    #[error("test error: {0}")]
    Test(String),

    #[error("typecheck error: {0}")]
    Typecheck(String),

    #[error("opt error: {0}")]
    Opt(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
