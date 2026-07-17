use super::super::*;
use super::apply::{
    ApplyControl, appn_callable_from_value, can_inline_compiled_coverage,
    eval_app_n_callable_runtime,
};
use super::eval::eval_cexpr_runtime;
use crate::Shared;

#[derive(Clone, Debug)]
pub(crate) struct PrimitiveForwardPlan {
    op: PrimOp,
    arity: usize,
    args: Box<[PrimitiveForwardArg]>,
}

#[derive(Clone, Debug)]
struct PrimitiveForwardArg {
    supplied_index: usize,
    name: String,
    statement_site: u32,
}

impl PrimitiveForwardPlan {
    pub(crate) fn derive(first_body: &Arc<CExpr>) -> Option<Self> {
        let mut arity = 1usize;
        let mut body = first_body;
        while let CExpr::FnUnary {
            body: next_body, ..
        } = body.as_ref()
        {
            arity = arity.checked_add(1)?;
            body = next_body;
        }
        let CExpr::Prim { op, args } = body.as_ref() else {
            return None;
        };
        let mut forwarded = Vec::with_capacity(args.len());
        for arg in args {
            let CExpr::Var {
                name,
                resolution: VarResolution::Local { depth, slot: 0 },
                statement_site,
                ..
            } = arg.as_ref()
            else {
                return None;
            };
            let depth = usize::from(*depth);
            if depth >= arity {
                return None;
            }
            forwarded.push(PrimitiveForwardArg {
                supplied_index: arity - 1 - depth,
                name: name.clone(),
                statement_site: *statement_site,
            });
        }
        Some(Self {
            op: *op,
            arity,
            args: forwarded.into_boxed_slice(),
        })
    }

    pub(super) fn op(&self) -> PrimOp {
        self.op
    }

    pub(super) fn arity(&self) -> usize {
        self.arity
    }

    pub(super) fn supplied_indices(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.args.iter().map(|arg| arg.supplied_index)
    }
}

pub(super) fn eval_primitive_forward_inline(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    data: Shared<crate::value::CompiledClosureData>,
    args: &[Arc<CExpr>],
) -> Result<Option<ApplyControl>, KernelError> {
    let Some(plan) = data.primitive_forward_plan.as_ref() else {
        return Ok(None);
    };
    if ctx.mem_limits != crate::eval::MemLimits::default()
        || args.len() < plan.arity
        || !can_inline_compiled_coverage(caller_env, data.body_c.coverage_sites())
    {
        return Ok(None);
    }

    #[cfg(test)]
    PRIMITIVE_FORWARD_EXECUTIONS.with(|count| count.set(count.get().saturating_add(1)));

    let mut supplied = Vec::with_capacity(plan.arity);
    for (index, arg) in args[..plan.arity].iter().enumerate() {
        supplied.push(eval_cexpr_runtime(ctx, caller_env.clone(), arg)?);
        if index + 1 < plan.arity {
            // Ordinary curried evaluation observes each intermediate unary function.
            ctx.tick()?;
        }
    }

    // Replay the final primitive expression and each forwarded variable exactly.
    ctx.tick()?;
    let value = if let [left, right] = plan.args.as_ref() {
        let left = replay_forwarded_var(ctx, caller_env, &supplied, left)?;
        let right = replay_forwarded_var(ctx, caller_env, &supplied, right)?;
        prim_op2(ctx, plan.op, left, right)?
    } else {
        let mut prim_args = Vec::with_capacity(plan.args.len());
        for forwarded in &plan.args {
            prim_args.push(replay_forwarded_var(ctx, caller_env, &supplied, forwarded)?);
        }
        prim_op(ctx, plan.op, prim_args)?
    };

    if args.len() == plan.arity {
        return Ok(Some(ApplyControl::Value(value)));
    }
    let control = eval_app_n_callable_runtime(
        ctx,
        caller_env,
        appn_callable_from_value(value),
        &args[plan.arity..],
    )?;
    Ok(Some(control))
}

fn replay_forwarded_var(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    supplied: &[Value],
    forwarded: &PrimitiveForwardArg,
) -> Result<Value, KernelError> {
    ctx.tick()?;
    let value = supplied[forwarded.supplied_index].clone();
    if let Some(run_id) = caller_env.coverage_run {
        ctx.coverage_statement_site_index(run_id, forwarded.statement_site)?;
    }
    if ctx.coverage_enabled() {
        ctx.coverage_hit(&forwarded.name, &value);
    }
    Ok(value)
}

#[cfg(test)]
thread_local! {
    static PRIMITIVE_FORWARD_EXECUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_primitive_forward_executions() {
    PRIMITIVE_FORWARD_EXECUTIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn primitive_forward_executions() -> usize {
    PRIMITIVE_FORWARD_EXECUTIONS.with(std::cell::Cell::get)
}
