use super::super::*;
use super::apply::{ApplyControl, apply_value_to_arg};
use super::eval::eval_cexpr_runtime;

pub(super) fn eval_metered_app_n(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    mut value: Value,
    args: &[Arc<CExpr>],
) -> Result<ApplyControl, KernelError> {
    for (index, arg_expr) in args.iter().enumerate() {
        let arg = eval_cexpr_runtime(ctx, caller_env.clone(), arg_expr)?;
        let final_argument = index + 1 == args.len();
        value = match apply_value_to_arg(ctx, caller_env, value, arg, final_argument)? {
            ApplyControl::Value(value) => value,
            ApplyControl::Tail { runtime, body } if final_argument => {
                return Ok(ApplyControl::Tail { runtime, body });
            }
            ApplyControl::Tail { .. } => {
                return Err(KernelError::new(
                    KernelErrorKind::Internal,
                    "metered non-final curried application unexpectedly returned tail control",
                ));
            }
        };
    }
    Ok(ApplyControl::Value(value))
}
