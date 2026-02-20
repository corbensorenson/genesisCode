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

pub(super) fn lower_str_join_parts_term(
    parts_t: &Term,
    sep: PExpr,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_join::lower_str_join_parts_term(
        parts_t,
        sep,
        env,
        global_env,
        fn_defs,
        local_fn_defs,
        planner,
    )
}

pub(super) fn lower_str_join_parts_term_with_aliases(
    parts_t: &Term,
    sep: PExpr,
    scope: &VecGetScope<'_>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_join::lower_str_join_parts_term_with_aliases(
        parts_t,
        sep,
        scope,
        vec_aliases,
        planner,
    )
}

pub(super) fn lower_str_join_sep_expr(
    parts_ids: Vec<i32>,
    sep: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_join::lower_str_join_sep_expr(parts_ids, sep, planner)
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

pub(super) fn lower_bytes_join_parts_term(
    parts_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_join::lower_bytes_join_parts_term(
        parts_t,
        env,
        global_env,
        fn_defs,
        local_fn_defs,
        planner,
    )
}

pub(super) fn lower_bytes_join_parts_term_with_aliases(
    parts_t: &Term,
    scope: &VecGetScope<'_>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_join::lower_bytes_join_parts_term_with_aliases(
        parts_t,
        scope,
        vec_aliases,
        planner,
    )
}

pub(super) fn lower_str_concat_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_id: i32,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_str_concat_const_pair(lhs, rhs, lhs_id, rhs_id, planner)
}

pub(super) fn lower_str_join_const_pair(
    parts_ids: Vec<i32>,
    sep: PExpr,
    sep_id: i32,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_str_join_const_pair(parts_ids, sep, sep_id, planner)
}

pub(super) fn lower_str_repeat_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_n: i64,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_str_repeat_const_pair(lhs, rhs, lhs_id, rhs_n, planner)
}

pub(super) fn lower_bytes_join_const_parts(
    parts_ids: Vec<i32>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_bytes_join_const_parts(parts_ids, planner)
}

pub(super) fn lower_bytes_concat_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_id: i32,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_bytes_concat_const_pair(lhs, rhs, lhs_id, rhs_id, planner)
}

pub(super) fn lower_bytes_get_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_n: i64,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    expr_lowering_value_lowering::lower_bytes_get_const_pair(lhs, rhs, lhs_id, rhs_n, planner)
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
            return Err(Stage2CompileError::Unsupported(format!(
                "recursive function call is unsupported in stage2: {name}"
            )));
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
