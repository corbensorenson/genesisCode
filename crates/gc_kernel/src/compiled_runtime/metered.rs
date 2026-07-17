use super::super::*;
use super::apply::{ApplyControl, apply_value_to_arg};
use super::eval::eval_cexpr_runtime;

pub(super) fn eval_metered_app_n(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    mut value: Value,
    args: &[Arc<CExpr>],
) -> Result<ApplyControl, KernelError> {
    for arg_expr in args {
        let arg = eval_cexpr_runtime(ctx, caller_env.clone(), arg_expr)?;
        value = match apply_value_to_arg(ctx, caller_env, value, arg, false)? {
            ApplyControl::Value(value) => value,
            ApplyControl::Tail { .. } => {
                return Err(KernelError::new(
                    KernelErrorKind::Internal,
                    "metered curried application unexpectedly returned tail control",
                ));
            }
        };
    }
    Ok(ApplyControl::Value(value))
}
