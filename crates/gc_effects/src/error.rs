use thiserror::Error;

#[derive(Debug, Error)]
pub enum EffectsError {
    #[error("missing protocol tokens in EvalCtx (did you build the prelude?)")]
    MissingProtocol,

    #[error("expected an effect program value")]
    NotAnEffectProgram,

    #[error("effect request is not sealed with the EFFECT protocol token")]
    BadEffectSeal,

    #[error("effect request payload is malformed: {0}")]
    BadPayload(String),

    #[error("capability denied for op {op}")]
    Denied { op: String },

    #[error("unknown capability op {op}")]
    UnknownOp { op: String },

    #[error("kernel error: {0}")]
    Kernel(#[from] gc_kernel::KernelError),

    #[error("log parse error: {0}")]
    Log(String),

    #[error("replay mismatch: {0}")]
    ReplayMismatch(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
