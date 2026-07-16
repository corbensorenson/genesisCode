use super::super::*;
use super::apply::{ApplyControl, apply_value_to_arg, eval_app_n_runtime};

pub(in super::super) fn eval_cexpr_runtime(
    ctx: &mut EvalCtx,
    runtime: RuntimeEnv,
    expr: &Arc<CExpr>,
) -> Result<Value, KernelError> {
    // Like eval_term, implement tail-call optimization for:
    // - (if ...) branches
    // - (begin ...) last form
    // - application where the callee is a closure
    let mut cur_env = runtime;
    let mut cur = expr.clone();
    loop {
        ctx.tick()?;
        match cur.as_ref() {
            CExpr::Atom(t) => {
                // Mirror eval_term's memory observations for strings/bytes.
                match t {
                    Term::Str(s) => ctx.mem_observe_string_len(s.len())?,
                    Term::Bytes(b) => ctx.mem_observe_bytes_len(b.len())?,
                    _ => {}
                }
                return Ok(Value::data(t.clone()));
            }
            CExpr::Var {
                name,
                sym,
                resolution,
                statement_site,
            } => {
                let _sym = *sym;
                let value = match *resolution {
                    VarResolution::Local { depth, slot } => {
                        cur_env.local_get(depth, slot).ok_or_else(|| {
                            KernelError::new(
                                KernelErrorKind::Unbound,
                                format!("unbound symbol: {name}"),
                            )
                        })?
                    }
                    VarResolution::Module { slot } => {
                        cur_env.module.get(slot).ok_or_else(|| {
                            KernelError::new(
                                KernelErrorKind::Unbound,
                                format!("unbound symbol: {name}"),
                            )
                        })?
                    }
                    VarResolution::External => cur_env.external.get(name).ok_or_else(|| {
                        KernelError::new(
                            KernelErrorKind::Unbound,
                            format!("unbound symbol: {name}"),
                        )
                    })?,
                };
                if let Some(run_id) = cur_env.coverage_run {
                    ctx.coverage_statement_site_index(run_id, *statement_site)?;
                }
                if ctx.coverage_enabled() {
                    ctx.coverage_hit(name, &value);
                }
                return Ok(value);
            }
            CExpr::Vector(xs) => {
                ctx.mem_observe_vec_len(xs.len())?;
                for x in xs {
                    ctx.mem_observe_data_term(x)?;
                }
                return Ok(Value::vector(xs.iter().cloned().map(Value::data).collect()));
            }
            CExpr::Map(entries) => {
                ctx.mem_observe_map_len(entries.len())?;
                for (k, _v) in entries {
                    ctx.mem_observe_data_term(&k.0)?;
                }
                let mut out = crate::value::ValueMap::new();
                for (k, v) in entries {
                    let vv = eval_cexpr_runtime(ctx, cur_env.clone(), v)?;
                    out.insert_mut(k.clone(), vv);
                }
                return Ok(Value::map(out));
            }
            CExpr::Quote(d) => {
                ctx.mem_observe_data_term(d)?;
                return Ok(Value::data(d.clone()));
            }
            CExpr::If {
                cond,
                then_expr,
                else_expr,
                decision_site,
            } => {
                if let Some(run_id) = cur_env.coverage_run {
                    ctx.coverage_begin_decision_site_index(run_id, *decision_site)?;
                }
                let cv = match eval_cexpr_runtime(ctx, cur_env.clone(), cond) {
                    Ok(v) => v,
                    Err(e) => {
                        if cur_env.coverage_run.is_some() {
                            ctx.coverage_abort_decision_site();
                        }
                        return Err(e);
                    }
                };
                let cond_truthy = cv.truthy();
                if let Some(run_id) = cur_env.coverage_run {
                    ctx.coverage_finish_decision_site_index(run_id, cond_truthy)?;
                }
                cur = if cond_truthy {
                    then_expr.clone()
                } else {
                    else_expr.clone()
                };
                continue;
            }
            CExpr::Begin(xs) => {
                if xs.is_empty() {
                    return Ok(Value::data(Term::Nil));
                }
                if xs.len() == 1 {
                    cur = xs[0].clone();
                    continue;
                }
                for x in xs.iter().take(xs.len() - 1) {
                    let _ = eval_cexpr_runtime(ctx, cur_env.clone(), x)?;
                }
                cur = xs[xs.len() - 1].clone();
                continue;
            }
            CExpr::Let(bs, body) => {
                let mut env2 = cur_env.clone();
                for (name, rhs) in bs {
                    let v = eval_cexpr_runtime(ctx, env2.clone(), rhs)?;
                    env2 = env2.with_slot(name, v);
                }
                cur_env = env2;
                cur = body.clone();
                continue;
            }
            CExpr::FnUnary {
                param,
                body_term,
                body,
                capture_plan,
            } => {
                let plan = capture_plan.get_or_init(|| ClosureCapturePlan::for_body(body));
                return Ok(Value::compiled_closure(
                    param.clone(),
                    body_term.clone(),
                    crate::value::CompiledExpr::new(body.clone(), cur_env.coverage_sites.clone()),
                    cur_env.external_for_capture(plan),
                    Some(cur_env.lexical_for_capture(plan)?),
                    Some(cur_env.module.clone()),
                ));
            }
            CExpr::Prim { op, args } => {
                if args.len() == 2 {
                    let a = eval_cexpr_runtime(ctx, cur_env.clone(), &args[0])?;
                    let b = eval_cexpr_runtime(ctx, cur_env.clone(), &args[1])?;
                    return prim_op2(ctx, *op, a, b);
                }
                let mut vs = Vec::with_capacity(args.len());
                for a in args {
                    vs.push(eval_cexpr_runtime(ctx, cur_env.clone(), a)?);
                }
                return prim_op(ctx, *op, vs);
            }
            CExpr::PrimUnknown { op, args } => {
                let mut vs = Vec::with_capacity(args.len());
                for a in args {
                    vs.push(eval_cexpr_runtime(ctx, cur_env.clone(), a)?);
                }
                return prim(ctx, op, vs);
            }
            CExpr::SealNew => {
                let id = ctx.state.next_seal_id;
                ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
                return Ok(Value::SealToken(crate::value::SealId(id)));
            }
            CExpr::Seal(v, tok) => {
                let vv = eval_cexpr_runtime(ctx, cur_env.clone(), v)?;
                let tv = eval_cexpr_runtime(ctx, cur_env.clone(), tok)?;
                let Value::SealToken(id) = tv else {
                    return type_err(ctx, "seal expects a seal token as second argument");
                };
                return Ok(Value::Sealed {
                    token: id,
                    payload: Box::new(vv),
                });
            }
            CExpr::Unseal(w, tok) => {
                let wv = eval_cexpr_runtime(ctx, cur_env.clone(), w)?;
                let tv = eval_cexpr_runtime(ctx, cur_env.clone(), tok)?;
                let Value::SealToken(id) = tv else {
                    return type_err(ctx, "unseal expects a seal token as second argument");
                };
                if let Value::Sealed { token, payload } = wv
                    && token == id
                {
                    return Ok(*payload);
                }
                return Ok(Value::data(Term::Nil));
            }
            CExpr::App(f, x) => {
                let fv = eval_cexpr_runtime(ctx, cur_env.clone(), f)?;
                let xv = eval_cexpr_runtime(ctx, cur_env.clone(), x)?;
                match apply_value_to_arg(ctx, &cur_env, fv, xv, true)? {
                    ApplyControl::Tail { runtime, body } => {
                        cur_env = runtime;
                        cur = body;
                        continue;
                    }
                    ApplyControl::Value(value) => return Ok(value),
                }
            }
            CExpr::AppN {
                callee,
                args,
                extra_app_ticks,
            } => match eval_app_n_runtime(ctx, &cur_env, callee, args, *extra_app_ticks)? {
                ApplyControl::Tail { runtime, body } => {
                    cur_env = runtime;
                    cur = body;
                    continue;
                }
                ApplyControl::Value(value) => return Ok(value),
            },
        }
    }
}
