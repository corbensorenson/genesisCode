mod apply;
mod eval;
mod patterns;
mod primitive_forward;

pub(crate) use apply::{
    CompiledClosureCall, eval_compiled_closure_body_scoped as apply_compiled_closure,
};
pub(super) use eval::eval_cexpr_runtime;
pub(crate) use primitive_forward::PrimitiveForwardPlan;

#[cfg(test)]
pub(crate) use apply::{
    appn_native_partial_materializations, reset_appn_native_partial_materializations,
};
#[cfg(test)]
pub(crate) use primitive_forward::{
    primitive_forward_executions, reset_primitive_forward_executions,
};
