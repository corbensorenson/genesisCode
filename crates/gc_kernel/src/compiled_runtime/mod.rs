mod apply;
mod eval;
mod primitive_forward;
mod tail_loop;

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
#[cfg(test)]
pub(crate) use tail_loop::{reset_tail_loop_executions, tail_loop_executions};
