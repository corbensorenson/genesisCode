use super::super::*;
use super::eval::eval_cexpr_runtime;
use super::primitive_forward::eval_primitive_forward_inline;
use super::tail_loop::eval_tail_loop_inline;
use crate::Shared;

pub(crate) struct CompiledClosureCall {
    pub(crate) external_env: Env,
    pub(crate) lexical_env: Option<CompiledLexicalEnv>,
    pub(crate) module_env: Option<CompiledModuleCells>,
    pub(crate) coverage_sites: Arc<CompiledCoverageSites>,
    pub(crate) param: crate::value::Sym,
    pub(crate) bind_external_param: bool,
    pub(crate) body: Arc<CExpr>,
    pub(crate) arg: Value,
}

pub(crate) fn eval_compiled_closure_body_scoped(
    ctx: &mut EvalCtx,
    call: CompiledClosureCall,
) -> Result<Value, KernelError> {
    let coverage_run = ctx.coverage_begin_indexed_run(
        call.coverage_sites.statement_sites().len(),
        call.coverage_sites.decision_sites().len(),
    );
    let runtime = RuntimeEnv {
        lexical: call.lexical_env.unwrap_or_else(CompiledLexicalEnv::empty),
        inline_slots: Shared::new(Vec::new()),
        module: call.module_env.unwrap_or_else(CompiledModuleCells::empty),
        external: call.external_env,
        coverage_sites: call.coverage_sites.clone(),
        coverage_run,
    };
    let runtime = if call.bind_external_param {
        runtime.with_slot_and_external(call.param.as_ref(), call.arg)
    } else {
        runtime.with_slot(call.param.as_ref(), call.arg)
    };
    let result = ctx.run_panic_guarded_always("compiled closure application", |ctx| {
        eval_cexpr_runtime(ctx, runtime, &call.body)
    });
    if let Some(run_id) = coverage_run {
        ctx.coverage_flush_indexed_run(
            run_id,
            call.coverage_sites.statement_sites(),
            call.coverage_sites.decision_sites(),
        )?;
    }
    result
}

pub(super) enum ApplyControl {
    Value(Value),
    Tail {
        runtime: RuntimeEnv,
        body: Arc<CExpr>,
    },
}

pub(super) enum AppNCallable {
    Value(Value),
    Compiled {
        runtime: RuntimeEnv,
        param: String,
        body: Arc<CExpr>,
    },
    Native {
        native: Shared<NativeFn>,
        appended: Vec<Value>,
    },
}

pub(super) fn appn_callable_from_value(value: Value) -> AppNCallable {
    match value {
        Value::NativeFn(native) => AppNCallable::Native {
            native,
            appended: Vec::new(),
        },
        other => AppNCallable::Value(other),
    }
}

pub(super) fn can_inline_compiled_coverage(
    caller_env: &RuntimeEnv,
    closure_coverage: &Arc<CompiledCoverageSites>,
) -> bool {
    caller_env.coverage_run.is_none()
        || CompiledCoverageSites::same_table(&caller_env.coverage_sites, closure_coverage)
}

pub(super) fn apply_value_to_arg(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    fv: Value,
    arg: Value,
    allow_tail: bool,
) -> Result<ApplyControl, KernelError> {
    match fv {
        Value::Closure(data) => {
            let compiled_body = compiled_compile::compile_term(&data.body).map_err(|e| {
                KernelError::new(
                    e.kind.clone(),
                    format!("failed to compile legacy closure body in compiled mode: {e}"),
                )
            })?;
            let value = eval_compiled_closure_body_scoped(
                ctx,
                CompiledClosureCall {
                    external_env: data.env.clone(),
                    lexical_env: None,
                    module_env: Some(CompiledModuleCells::empty()),
                    coverage_sites: compiled_body.coverage_sites,
                    param: data.param.clone(),
                    bind_external_param: true,
                    body: compiled_body.expr,
                    arg,
                },
            )?;
            Ok(ApplyControl::Value(value))
        }
        Value::CompiledClosure(data) => {
            let closure_coverage = data.body_c.coverage_sites().clone();
            if allow_tail && can_inline_compiled_coverage(caller_env, &closure_coverage) {
                return Ok(ApplyControl::Tail {
                    runtime: RuntimeEnv {
                        lexical: data
                            .compiled_env
                            .clone()
                            .unwrap_or_else(CompiledLexicalEnv::empty),
                        inline_slots: Shared::new(Vec::new()),
                        module: data
                            .module_env
                            .clone()
                            .unwrap_or_else(CompiledModuleCells::empty),
                        external: data.env.clone(),
                        coverage_sites: closure_coverage,
                        coverage_run: caller_env.coverage_run,
                    }
                    .with_slot(data.param.as_ref(), arg),
                    body: data.body_c.inner().clone(),
                });
            }
            let value = eval_compiled_closure_body_scoped(
                ctx,
                CompiledClosureCall {
                    external_env: data.env.clone(),
                    lexical_env: data.compiled_env.clone(),
                    module_env: data.module_env.clone(),
                    coverage_sites: closure_coverage,
                    param: data.param.clone(),
                    bind_external_param: false,
                    body: data.body_c.inner().clone(),
                    arg,
                },
            )?;
            Ok(ApplyControl::Value(value))
        }
        Value::NativeFn(nf) => Ok(ApplyControl::Value(nf.apply(ctx, arg)?)),
        other => Err(KernelError::new(
            KernelErrorKind::NotCallable,
            format!("value is not callable: {}", other.debug_repr()),
        )),
    }
}

pub(super) fn eval_app_n_runtime(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    callee: &Arc<CExpr>,
    args: &[Arc<CExpr>],
    extra_app_ticks: u32,
) -> Result<ApplyControl, KernelError> {
    for _ in 0..extra_app_ticks {
        ctx.tick()?;
    }
    let callee_value = eval_cexpr_runtime(ctx, caller_env.clone(), callee)?;
    if let Value::CompiledClosure(data) = callee_value {
        if let Some(control) = eval_primitive_forward_inline(ctx, caller_env, data.clone(), args)? {
            return Ok(control);
        }
        if let Some(value) = eval_tail_loop_inline(ctx, caller_env, data.clone(), args)? {
            return Ok(ApplyControl::Value(value));
        }
        if let Some(control) =
            eval_compiled_closure_appn_inline(ctx, caller_env, data.clone(), args)?
        {
            return Ok(control);
        }
        return eval_app_n_callable_runtime(
            ctx,
            caller_env,
            appn_callable_from_value(Value::CompiledClosure(data)),
            args,
        );
    }
    eval_app_n_callable_runtime(
        ctx,
        caller_env,
        appn_callable_from_value(callee_value),
        args,
    )
}

pub(super) fn eval_app_n_callable_runtime(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    mut callable: AppNCallable,
    args: &[Arc<CExpr>],
) -> Result<ApplyControl, KernelError> {
    for (idx, arg_expr) in args.iter().enumerate() {
        let last = idx + 1 == args.len();
        let arg = eval_cexpr_runtime(ctx, caller_env.clone(), arg_expr)?;
        callable = match callable {
            AppNCallable::Value(value) => match value {
                Value::CompiledClosure(data) => {
                    let closure_coverage = data.body_c.coverage_sites().clone();
                    if can_inline_compiled_coverage(caller_env, &closure_coverage) {
                        let runtime = RuntimeEnv {
                            lexical: data
                                .compiled_env
                                .clone()
                                .unwrap_or_else(CompiledLexicalEnv::empty),
                            inline_slots: Shared::new(Vec::new()),
                            module: data
                                .module_env
                                .clone()
                                .unwrap_or_else(CompiledModuleCells::empty),
                            external: data.env.clone(),
                            coverage_sites: closure_coverage,
                            coverage_run: caller_env.coverage_run,
                        }
                        .with_slot(data.param.as_ref(), arg);
                        if last {
                            return Ok(ApplyControl::Tail {
                                runtime,
                                body: data.body_c.inner().clone(),
                            });
                        }
                        match data.body_c.inner().as_ref() {
                            CExpr::FnUnary {
                                param: next_param,
                                body: next_body,
                                ..
                            } => {
                                // Preserve the step charged by evaluating the intermediate
                                // FnUnary while skipping the transient closure allocation.
                                ctx.tick()?;
                                AppNCallable::Compiled {
                                    runtime,
                                    param: next_param.clone(),
                                    body: next_body.clone(),
                                }
                            }
                            _ => {
                                let value = eval_cexpr_runtime(ctx, runtime, data.body_c.inner())?;
                                appn_callable_from_value(value)
                            }
                        }
                    } else {
                        let value = eval_compiled_closure_body_scoped(
                            ctx,
                            CompiledClosureCall {
                                external_env: data.env.clone(),
                                lexical_env: data.compiled_env.clone(),
                                module_env: data.module_env.clone(),
                                coverage_sites: closure_coverage,
                                param: data.param.clone(),
                                bind_external_param: false,
                                body: data.body_c.inner().clone(),
                                arg,
                            },
                        )?;
                        if last {
                            return Ok(ApplyControl::Value(value));
                        }
                        appn_callable_from_value(value)
                    }
                }
                other => match apply_value_to_arg(ctx, caller_env, other, arg, false)? {
                    ApplyControl::Value(value) if last => return Ok(ApplyControl::Value(value)),
                    ApplyControl::Value(value) => appn_callable_from_value(value),
                    ApplyControl::Tail { runtime, body } => {
                        return Ok(ApplyControl::Tail { runtime, body });
                    }
                },
            },
            AppNCallable::Compiled {
                runtime,
                param,
                body,
            } => {
                let runtime = runtime.with_slot(&param, arg);
                if last {
                    return Ok(ApplyControl::Tail { runtime, body });
                }
                match body.as_ref() {
                    CExpr::FnUnary {
                        param: next_param,
                        body: next_body,
                        ..
                    } => {
                        ctx.tick()?;
                        AppNCallable::Compiled {
                            runtime,
                            param: next_param.clone(),
                            body: next_body.clone(),
                        }
                    }
                    _ => {
                        let value = eval_cexpr_runtime(ctx, runtime, &body)?;
                        appn_callable_from_value(value)
                    }
                }
            }
            AppNCallable::Native {
                native,
                mut appended,
            } => {
                appended.push(arg);
                let total = native.collected.len().saturating_add(appended.len());
                if total < native.arity {
                    if last {
                        return Ok(ApplyControl::Value(materialize_native_partial(
                            &native, appended,
                        )));
                    }
                    AppNCallable::Native { native, appended }
                } else {
                    let value = native.apply_collected(ctx, native_args(&native, appended))?;
                    if last {
                        return Ok(ApplyControl::Value(value));
                    }
                    appn_callable_from_value(value)
                }
            }
        };
    }
    match callable {
        AppNCallable::Value(value) => Ok(ApplyControl::Value(value)),
        AppNCallable::Native { native, appended } => Ok(ApplyControl::Value(
            materialize_native_partial(&native, appended),
        )),
        AppNCallable::Compiled {
            runtime: _,
            param,
            body: _,
        } => Err(KernelError::new(
            KernelErrorKind::Internal,
            format!("compiled AppN ended with pending parameter: {param}"),
        )),
    }
}

fn native_args(native: &NativeFn, appended: Vec<Value>) -> Vec<Value> {
    let mut collected = Vec::with_capacity(native.collected.len().saturating_add(appended.len()));
    collected.extend(native.collected.iter().cloned());
    collected.extend(appended);
    collected
}

fn materialize_native_partial(native: &NativeFn, appended: Vec<Value>) -> Value {
    #[cfg(test)]
    APPN_NATIVE_PARTIAL_MATERIALIZATIONS.with(|count| count.set(count.get().saturating_add(1)));
    Value::native_fn(NativeFn {
        name: native.name,
        arity: native.arity,
        collected: native_args(native, appended),
        func: native.func,
    })
}

#[cfg(test)]
thread_local! {
    static APPN_NATIVE_PARTIAL_MATERIALIZATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_appn_native_partial_materializations() {
    APPN_NATIVE_PARTIAL_MATERIALIZATIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn appn_native_partial_materializations() -> usize {
    APPN_NATIVE_PARTIAL_MATERIALIZATIONS.with(std::cell::Cell::get)
}

fn eval_compiled_closure_appn_inline(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    data: Shared<crate::value::CompiledClosureData>,
    args: &[Arc<CExpr>],
) -> Result<Option<ApplyControl>, KernelError> {
    let closure_coverage = data.body_c.coverage_sites().clone();
    if !can_inline_compiled_coverage(caller_env, &closure_coverage) {
        return Ok(None);
    }

    let mut runtime = RuntimeEnv {
        lexical: data
            .compiled_env
            .clone()
            .unwrap_or_else(CompiledLexicalEnv::empty),
        inline_slots: Shared::new(Vec::new()),
        module: data
            .module_env
            .clone()
            .unwrap_or_else(CompiledModuleCells::empty),
        external: data.env.clone(),
        coverage_sites: closure_coverage,
        coverage_run: caller_env.coverage_run,
    };
    let mut body = data.body_c.inner().clone();
    let mut values = Vec::new();

    for (idx, arg_expr) in args.iter().enumerate() {
        let last = idx + 1 == args.len();
        values.push(eval_cexpr_runtime(ctx, caller_env.clone(), arg_expr)?);
        if last {
            runtime = runtime.with_slots(values);
            return Ok(Some(ApplyControl::Tail { runtime, body }));
        }
        match body.as_ref() {
            CExpr::FnUnary {
                body: next_body, ..
            } => {
                // Preserve the step charged by evaluating the intermediate FnUnary while
                // packing the corresponding lexical frame into a single segment.
                ctx.tick()?;
                body = next_body.clone();
            }
            _ => {
                runtime = runtime.with_slots(values);
                let value = eval_cexpr_runtime(ctx, runtime, &body)?;
                let control = eval_app_n_callable_runtime(
                    ctx,
                    caller_env,
                    appn_callable_from_value(value),
                    &args[(idx + 1)..],
                )?;
                return Ok(Some(control));
            }
        }
    }

    Ok(Some(ApplyControl::Value(Value::CompiledClosure(data))))
}
