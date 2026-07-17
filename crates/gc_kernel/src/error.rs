use thiserror::Error;

#[derive(Debug, Error, Clone)]
#[error("{kind}: {msg}")]
pub struct KernelError {
    pub kind: KernelErrorKind,
    pub msg: String,
    pub resource_limit: Option<ResourceLimit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceLimit {
    pub dimension: &'static str,
    pub observed: u64,
    pub limit: u64,
}

impl KernelError {
    pub fn new(kind: KernelErrorKind, msg: impl Into<String>) -> Self {
        Self {
            kind,
            msg: msg.into(),
            resource_limit: None,
        }
    }

    pub fn memory_limit(dimension: &'static str, observed: u64, limit: u64) -> Self {
        Self {
            kind: KernelErrorKind::MemoryLimit,
            msg: format!("memory limit exceeded: {dimension} (observed={observed}, limit={limit})"),
            resource_limit: Some(ResourceLimit {
                dimension,
                observed,
                limit,
            }),
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
