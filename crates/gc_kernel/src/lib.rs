mod compiled;
mod env;
mod error;
mod eval;
mod value;

pub use compiled::{
    CompiledModule, compile_module, decode_compiled_module_blob, encode_compiled_module_blob,
    eval_compiled_module, eval_module_compiled,
};
pub use env::{Env, EnvFrame};
pub use error::{KernelError, KernelErrorKind};
pub use eval::{
    DEFAULT_STEP_LIMIT, EvalCtx, EvalState, MemLimits, ProtocolTokens, StepLimit, eval_module,
    eval_term,
};
pub use value::{
    Apply, Contract, EffectProgram, EffectRequest, NativeFn, SealId, Value, value_hash,
};

pub use gc_coreform::Term;

#[cfg(test)]
mod tests;
