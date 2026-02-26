use super::*;

#[path = "expr_lowering_join.rs"]
mod expr_lowering_join;
#[path = "expr_lowering_value_lowering.rs"]
mod expr_lowering_value_lowering;

pub(super) fn lower_str_join_terms(
    parts_t: &Term,
    sep_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_join::lower_str_join_terms(
        parts_t,
        sep_t,
        env,
        global_env,
        fn_defs,
        local_fn_defs,
        planner,
    )
}

pub(super) fn lower_bytes_join_term(
    parts_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_join::lower_bytes_join_term(
        parts_t,
        env,
        global_env,
        fn_defs,
        local_fn_defs,
        planner,
    )
}

pub(super) fn lower_vec_get_const_pair(
    items: Vec<PExpr>,
    idx: PExpr,
    idx_n: i64,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_vec_get_const_pair(items, idx, idx_n, planner)
}

pub(super) fn lower_str_repeat_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_str_repeat_expr(lhs, rhs, planner)
}

pub(super) fn lower_str_concat_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_str_concat_expr(lhs, rhs, planner)
}

pub(super) fn lower_bytes_concat_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_bytes_concat_expr(lhs, rhs, planner)
}

pub(super) fn lower_bytes_get_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_bytes_get_expr(lhs, rhs, planner)
}

pub(super) fn try_plan_application_chain(
    t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<Option<PExpr>, Stage2CompileError> {
    let Some((head, args)) = flatten_application_chain(t) else {
        return Ok(None);
    };
    if args.is_empty() {
        return Ok(None);
    }

    let Some(mut callable) = resolve_callable_head(&head, env, global_env, fn_defs, local_fn_defs)?
    else {
        return Ok(None);
    };
    let mut pushed_name = None;
    if let Some(name) = callable.def_name.as_ref() {
        if planner.expanding_fn_defs.iter().any(|n| n == name) {
            if let Some(folded) =
                fold_recursive_fn_call_to_scalar(t, env, global_env, fn_defs, planner)?
            {
                return Ok(Some(folded));
            }
            return Ok(None);
        }
        planner.expanding_fn_defs.push(name.clone());
        pushed_name = Some(name.clone());
    }

    let mut bindings: Vec<LetBinding> = Vec::with_capacity(args.len());
    let mut result_expr = None;

    for (i, arg) in args.iter().enumerate() {
        let arg_expr = match plan_expr(arg, env, global_env, fn_defs, local_fn_defs, planner) {
            Ok(v) => v,
            Err(e) => {
                if pushed_name.is_some() {
                    planner.expanding_fn_defs.pop();
                }
                return Err(e);
            }
        };
        let idx = planner.alloc_local(arg_expr.ty())?;
        record_local_const_ids(planner, idx, &arg_expr);
        let mut call_env = callable.base_env.clone();
        call_env.insert(
            callable.param.clone(),
            Local {
                idx,
                ty: arg_expr.ty(),
            },
        );
        bindings.push(LetBinding {
            idx,
            expr: arg_expr,
        });

        let is_last = i + 1 == args.len();
        if is_last {
            result_expr = Some(
                match plan_expr(
                    &callable.body,
                    &call_env,
                    global_env,
                    fn_defs,
                    local_fn_defs,
                    planner,
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        if pushed_name.is_some() {
                            planner.expanding_fn_defs.pop();
                        }
                        return Err(e);
                    }
                },
            );
            break;
        }

        let Some((next_param, next_body)) = desugar_fn_literal_to_unary(&callable.body)? else {
            if pushed_name.is_some() {
                planner.expanding_fn_defs.pop();
            }
            return Err(Stage2CompileError::Unsupported(
                "application chain expects function result at each intermediate step".to_string(),
            ));
        };
        callable = CallableHead {
            param: next_param,
            body: next_body,
            base_env: call_env,
            def_name: None,
        };
    }

    if pushed_name.is_some() {
        planner.expanding_fn_defs.pop();
    }

    let mut out = result_expr.ok_or_else(|| {
        Stage2CompileError::Internal("application chain planning produced no result".to_string())
    })?;
    for binding in bindings.into_iter().rev() {
        let ty = out.ty();
        out = PExpr::Let {
            bindings: vec![binding],
            body: vec![out],
            ty,
        };
    }
    Ok(Some(out))
}

fn local_const_data_term(
    planner: &Planner,
    local: Local,
) -> Result<Option<Term>, Stage2CompileError> {
    let out = match local.ty {
        Ty::NilI32 => Some(Term::Nil),
        Ty::BoolI32 => None,
        Ty::I64 => planner
            .local_const_int_values
            .get(&local.idx)
            .copied()
            .map(|n| Term::Int(n.into())),
        Ty::SymI32 => {
            if let Some(id) = planner.local_const_symbol_ids.get(&local.idx).copied() {
                Some(Term::Symbol(planner_symbol_for_id(planner, id)?))
            } else {
                None
            }
        }
        Ty::StrI32 => {
            if let Some(id) = planner.local_const_string_ids.get(&local.idx).copied() {
                Some(Term::Str(planner_string_for_id(planner, id)?))
            } else {
                None
            }
        }
        Ty::BytesI32 => {
            if let Some(id) = planner.local_const_bytes_ids.get(&local.idx).copied() {
                Some(Term::Bytes(planner_bytes_for_id(planner, id)?.into()))
            } else {
                None
            }
        }
    };
    Ok(out)
}

fn fold_recursive_fn_call_to_scalar(
    t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<Option<PExpr>, Stage2CompileError> {
    let mut scalar_aliases: BTreeMap<String, Term> = BTreeMap::new();
    for (name, local) in env {
        if let Some(term) = local_const_data_term(planner, *local)? {
            scalar_aliases.insert(name.clone(), term);
        }
    }
    for (name, local) in global_env {
        if let Some(term) = local_const_data_term(planner, *local)? {
            scalar_aliases.insert(name.clone(), term);
        }
    }
    let expr = resolve_scalar_aliases_term(t, &scalar_aliases);

    let mut forms: Vec<Term> = Vec::new();
    for (name, local) in global_env {
        if let Some(term) = local_const_data_term(planner, *local)? {
            forms.push(Term::list(vec![
                Term::Symbol("def".to_string()),
                Term::Symbol(name.clone()),
                term,
            ]));
        }
    }
    for (name, fndef) in fn_defs {
        if !matches!(fndef.capture, FnCapture::GlobalFrame) {
            continue;
        }
        let rhs = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol(fndef.param.clone())]),
            fndef.body.clone(),
        ]);
        forms.push(Term::list(vec![
            Term::Symbol("def".to_string()),
            Term::Symbol(name.clone()),
            rhs,
        ]));
    }
    forms.push(expr);

    let mut ctx = EvalCtx::with_step_limit(Some(STAGE2_BASELINE_STEP_LIMIT));
    let prelude = build_prelude(&mut ctx);
    let mut eval_env = prelude.env;
    let value = match eval_module(&mut ctx, &mut eval_env, &forms) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let Value::Data(term) = value else {
        return Ok(None);
    };
    scalar_term_to_pexpr(&term, planner)
}

pub(super) fn is_safe_defs_only_rhs(t: &Term) -> bool {
    match t {
        Term::Nil
        | Term::Bool(_)
        | Term::Int(_)
        | Term::Str(_)
        | Term::Bytes(_)
        | Term::Symbol(_)
        | Term::Vector(_)
        | Term::Map(_)
            if term_const_data_term(t).is_some() =>
        {
            return true;
        }
        _ => {}
    }
    let Some(xs) = t.as_proper_list() else {
        return false;
    };
    if xs.is_empty() {
        return false;
    }
    matches!(
        xs[0],
        Term::Symbol(s) if (s == "fn" && xs.len() == 3) || (s == "quote" && xs.len() == 2)
    )
}
