use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{kind}: {msg}")]
pub struct KernelError {
    pub kind: KernelErrorKind,
    pub msg: String,
}

impl KernelError {
    pub fn new(kind: KernelErrorKind, msg: impl Into<String>) -> Self {
        Self {
            kind,
            msg: msg.into(),
        }
    }
}

#[derive(Debug, Error, Clone)]
pub enum KernelErrorKind {
    #[error("bad form")]
    BadForm,
    #[error("unbound symbol")]
    Unbound,
    #[error("type error")]
    Type,
    #[error("not callable")]
    NotCallable,
    #[error("internal error")]
    Internal,
    #[error("step limit exceeded")]
    StepLimit,
    #[error("memory limit exceeded")]
    MemoryLimit,
}
