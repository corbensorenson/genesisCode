use super::super::*;
use super::eval::eval_cexpr_runtime;

pub(super) fn eval_byte_get_or_nil_inline(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    data: Rc<crate::value::CompiledClosureData>,
    args: &[Arc<CExpr>],
) -> Result<Option<Value>, KernelError> {
    if args.len() != 2
        || ctx.step_limit.is_some()
        || ctx.coverage_enabled()
        || caller_env.coverage_run.is_some()
        || !match_byte_get_or_nil(caller_env, &data)
    {
        return Ok(None);
    }

    let bytes_value = eval_cexpr_runtime(ctx, caller_env.clone(), &args[0])?;
    let index_value = eval_cexpr_runtime(ctx, caller_env.clone(), &args[1])?;
    let Value::Data(bytes_term) = bytes_value else {
        return Ok(None);
    };
    let Term::Bytes(bytes) = bytes_term.as_ref() else {
        return Ok(None);
    };
    let Some(index) = value_to_i64(&index_value) else {
        return Ok(None);
    };
    let Ok(index) = usize::try_from(index) else {
        return Ok(None);
    };

    Ok(Some(
        bytes
            .get(index)
            .map(|b| Value::int(i64::from(*b)))
            .unwrap_or_else(|| Value::data(Term::Nil)),
    ))
}

pub(super) fn eval_counted_vec_push_loop_inline(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    data: Rc<crate::value::CompiledClosureData>,
    args: &[Arc<CExpr>],
) -> Result<Option<Value>, KernelError> {
    if args.len() != 3
        || ctx.step_limit.is_some()
        || ctx.coverage_enabled()
        || caller_env.coverage_run.is_some()
        || ctx.mem_limits != crate::eval::MemLimits::default()
    {
        return Ok(None);
    }
    if match_counted_vec_push_loop(caller_env, &data).is_none() {
        return Ok(None);
    }

    let i_value = eval_cexpr_runtime(ctx, caller_env.clone(), &args[0])?;
    let n_value = eval_cexpr_runtime(ctx, caller_env.clone(), &args[1])?;
    let acc_value = eval_cexpr_runtime(ctx, caller_env.clone(), &args[2])?;

    let Some(start) = value_to_i64(&i_value) else {
        return Ok(None);
    };
    let mut cur = start;
    let Some(end) = value_to_i64(&n_value) else {
        return Ok(None);
    };
    if cur == end {
        return Ok(Some(acc_value));
    }
    if cur > end {
        return Ok(None);
    }
    let Value::Vector(xs) = acc_value else {
        return Ok(None);
    };
    let mut out: Vec<Value> = xs.iter().cloned().collect();
    let additional = usize::try_from(end - cur).map_err(|_| {
        KernelError::new(
            KernelErrorKind::MemoryLimit,
            "vector loop range exceeds addressable memory",
        )
    })?;
    out.try_reserve(additional).map_err(|_| {
        KernelError::new(
            KernelErrorKind::MemoryLimit,
            "vector loop allocation exceeds available memory",
        )
    })?;
    while cur < end {
        out.push(Value::int(cur));
        cur = cur.saturating_add(1);
    }
    ctx.steps = ctx.steps.saturating_add((end - start) as u64);
    ctx.mem_observe_vec_len(out.len())?;
    Ok(Some(Value::vector(crate::value::ValueVector::Flat(out))))
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Int(n) => Some(*n),
        Value::Data(t) => match t.as_ref() {
            Term::Int(n) => n.to_i64(),
            _ => None,
        },
        _ => None,
    }
}

fn match_counted_vec_push_loop(
    caller_env: &RuntimeEnv,
    data: &Rc<crate::value::CompiledClosureData>,
) -> Option<()> {
    if !external_prim_wrapper_matches(&caller_env.external, "core/int::eq?", PrimOp::IntEq)
        || !external_prim_wrapper_matches(&caller_env.external, "core/int::add", PrimOp::IntAdd)
        || !external_prim_wrapper_matches(&caller_env.external, "core/vec::push", PrimOp::VecPush)
    {
        return None;
    }
    let self_slot = module_slot_for_compiled_closure(data)?;
    let i_param = data.param.as_ref();
    let CExpr::FnUnary {
        param: n_param,
        body: n_body,
        ..
    } = data.body_c.inner().as_ref()
    else {
        return None;
    };
    let CExpr::FnUnary {
        param: acc_param,
        body: acc_body,
        ..
    } = n_body.as_ref()
    else {
        return None;
    };
    let CExpr::If {
        cond,
        then_expr,
        else_expr,
        ..
    } = acc_body.as_ref()
    else {
        return None;
    };
    if !is_app2_var(cond, "core/int::eq?", i_param, n_param) {
        return None;
    }
    if !is_var_named(then_expr, acc_param) {
        return None;
    }
    let (callee, recur_args) = appn_parts(else_expr)?;
    if !is_module_var_slot(callee, self_slot) || recur_args.len() != 3 {
        return None;
    }
    if !is_int_add_one(&recur_args[0], i_param)
        || !is_var_named(&recur_args[1], n_param)
        || !is_app2_var(&recur_args[2], "core/vec::push", acc_param, i_param)
    {
        return None;
    }
    Some(())
}

fn match_byte_get_or_nil(
    caller_env: &RuntimeEnv,
    data: &Rc<crate::value::CompiledClosureData>,
) -> bool {
    if !external_prim_wrapper_matches(&caller_env.external, "core/int::lt?", PrimOp::IntLt)
        || !external_prim_wrapper_matches(&caller_env.external, "core/bytes::len", PrimOp::BytesLen)
        || !external_prim_wrapper_matches(&caller_env.external, "core/bytes::get", PrimOp::BytesGet)
    {
        return false;
    }
    let bytes_param = data.param.as_ref();
    let CExpr::FnUnary {
        param: index_param,
        body,
        ..
    } = data.body_c.inner().as_ref()
    else {
        return false;
    };
    let CExpr::If {
        cond,
        then_expr,
        else_expr,
        ..
    } = body.as_ref()
    else {
        return false;
    };

    is_app2_expr(
        cond,
        "core/int::lt?",
        |expr| is_var_named(expr, index_param),
        |expr| is_app1_var(expr, "core/bytes::len", bytes_param),
    ) && is_app2_var(then_expr, "core/bytes::get", bytes_param, index_param)
        && matches!(else_expr.as_ref(), CExpr::Atom(Term::Nil))
}

fn module_slot_for_compiled_closure(data: &Rc<crate::value::CompiledClosureData>) -> Option<u32> {
    let module = data.module_env.as_ref()?;
    for (idx, value) in module.0.borrow().iter().enumerate() {
        let Some(Value::CompiledClosure(other)) = value else {
            continue;
        };
        if Rc::ptr_eq(other, data) {
            return u32::try_from(idx).ok();
        }
    }
    None
}

fn external_prim_wrapper_matches(env: &Env, name: &str, op: PrimOp) -> bool {
    let Some(Value::CompiledClosure(data)) = env.get(name) else {
        return false;
    };
    matches!(binary_prim_wrapper_op(&data), Some(actual) if actual == op)
        || matches!(unary_prim_wrapper_op(&data), Some(actual) if actual == op)
}

fn binary_prim_wrapper_op(data: &Rc<crate::value::CompiledClosureData>) -> Option<PrimOp> {
    let first_param = data.param.as_ref();
    match data.body_c.inner().as_ref() {
        CExpr::FnUnary {
            param: second_param,
            body,
            ..
        } => {
            let CExpr::Prim { op, args } = body.as_ref() else {
                return None;
            };
            if args.len() == 2
                && is_var_named(&args[0], first_param)
                && is_var_named(&args[1], second_param)
            {
                Some(*op)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn unary_prim_wrapper_op(data: &Rc<crate::value::CompiledClosureData>) -> Option<PrimOp> {
    let first_param = data.param.as_ref();
    match data.body_c.inner().as_ref() {
        CExpr::Prim { op, args } if args.len() == 1 && is_var_named(&args[0], first_param) => {
            Some(*op)
        }
        _ => None,
    }
}

fn appn_parts(expr: &Arc<CExpr>) -> Option<(&Arc<CExpr>, &[Arc<CExpr>])> {
    match expr.as_ref() {
        CExpr::AppN { callee, args, .. } => Some((callee, args)),
        _ => None,
    }
}

fn is_app2_var(expr: &Arc<CExpr>, callee_name: &str, a: &str, b: &str) -> bool {
    is_app2_expr(
        expr,
        callee_name,
        |expr| is_var_named(expr, a),
        |expr| is_var_named(expr, b),
    )
}

fn is_app2_expr<F, G>(expr: &Arc<CExpr>, callee_name: &str, first: F, second: G) -> bool
where
    F: FnOnce(&Arc<CExpr>) -> bool,
    G: FnOnce(&Arc<CExpr>) -> bool,
{
    let Some((callee, args)) = appn_parts(expr) else {
        return false;
    };
    is_external_var_named(callee, callee_name)
        && args.len() == 2
        && first(&args[0])
        && second(&args[1])
}

fn is_app1_var(expr: &Arc<CExpr>, callee_name: &str, a: &str) -> bool {
    let Some((callee, args)) = appn_parts(expr) else {
        return false;
    };
    is_external_var_named(callee, callee_name) && args.len() == 1 && is_var_named(&args[0], a)
}

fn is_int_add_one(expr: &Arc<CExpr>, var_name: &str) -> bool {
    let Some((callee, args)) = appn_parts(expr) else {
        return false;
    };
    is_external_var_named(callee, "core/int::add")
        && args.len() == 2
        && is_var_named(&args[0], var_name)
        && matches!(args[1].as_ref(), CExpr::Atom(Term::Int(i)) if i == &num_bigint::BigInt::from(1))
}

fn is_var_named(expr: &Arc<CExpr>, expected: &str) -> bool {
    matches!(expr.as_ref(), CExpr::Var { name, .. } if name == expected)
}

fn is_external_var_named(expr: &Arc<CExpr>, expected: &str) -> bool {
    matches!(
        expr.as_ref(),
        CExpr::Var {
            name,
            resolution: VarResolution::External,
            ..
        } if name == expected
    )
}

fn is_module_var_slot(expr: &Arc<CExpr>, expected_slot: u32) -> bool {
    matches!(
        expr.as_ref(),
        CExpr::Var {
            resolution: VarResolution::Module { slot },
            ..
        } if *slot == expected_slot
    )
}
