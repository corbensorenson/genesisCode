mod compiled;
mod cycle;
mod env;
mod error;
mod eval;
mod fallible_alloc;
mod free_vars;
mod logical_heap;
mod value;

pub use compiled::{
    CompiledModule, CoverageSiteManifest, compile_module, compile_module_with_site_namespace,
    compiled_module_coverage_manifest, compiled_module_coverage_manifest_from_compiled,
    decode_compiled_module_blob, encode_compiled_module_blob, eval_compiled_module,
    eval_module_compiled,
};
pub use cycle::Shared;
pub use env::{Env, EnvFrame};
pub use error::{KernelError, KernelErrorKind, ResourceLimit};
pub use eval::{
    DEFAULT_STEP_LIMIT, DecisionCoverageCounters, DecisionSample, EvalCtx, EvalObservedCounters,
    EvalState, MemLimits, MemObservedCounters, ProtocolTokens, StepLimit, eval_module, eval_term,
};
pub use value::{
    Apply, Contract, EffectProgram, EffectRequest, NativeFn, SealId, Sym,
    VALUE_EFFECT_HASH_PROFILE_ID, Value, ValueMap, ValueVector, value_hash,
};

pub use gc_coreform::Term;

#[cfg(test)]
mod tests;
