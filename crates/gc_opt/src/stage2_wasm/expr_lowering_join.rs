use super::*;

pub(super) fn lower_str_join_terms(
    parts_t: &Term,
    sep_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let sep = plan_expr(sep_t, env, global_env, fn_defs, local_fn_defs, planner)?;
    if sep.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "str/join expects (vector-of-strings, string) arguments in stage2".to_string(),
        ));
    }
    lower_str_join_parts_term(
        parts_t,
        sep,
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
    let scope = VecGetScope {
        env,
        global_env,
        fn_defs,
        local_fn_defs,
    };
    let vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
    lower_str_join_parts_term_with_aliases(parts_t, sep, &scope, &vec_aliases, planner)
}

pub(super) fn lower_str_join_parts_term_with_aliases(
    parts_t: &Term,
    sep: PExpr,
    scope: &VecGetScope<'_>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let global_vec_aliases = planner.global_const_vector_aliases.clone();
    if let Some(parts_ids) = term_const_string_vector_ids_with_aliases(
        parts_t,
        vec_aliases,
        &global_vec_aliases,
        planner,
    )? {
        return lower_str_join_sep_expr(parts_ids, sep, planner);
    }

    let Some(xs) = parts_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "str/join currently requires stage2-known vector literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "str/join currently requires stage2-known vector literals".to_string(),
        ));
    }

    if matches!(xs[0], Term::Symbol(s) if s == "begin") {
        if xs.len() < 2 {
            return Err(Stage2CompileError::Unsupported(
                "begin must have at least one expression".to_string(),
            ));
        }
        let mut exprs = Vec::with_capacity(xs.len() - 1);
        for x in xs.iter().skip(1).take(xs.len().saturating_sub(2)) {
            exprs.push(plan_expr(
                x,
                scope.env,
                scope.global_env,
                scope.fn_defs,
                scope.local_fn_defs,
                planner,
            )?);
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("str/join begin had no body".to_string())
        })?;
        exprs.push(lower_str_join_parts_term_with_aliases(
            last,
            sep,
            scope,
            vec_aliases,
            planner,
        )?);
        return Ok(PExpr::Begin {
            exprs,
            ty: Ty::StrI32,
        });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(
            xs[1],
            scope.env,
            scope.global_env,
            scope.fn_defs,
            scope.local_fn_defs,
            planner,
        )?;
        let cond_ty = cond.ty();
        ensure_scalar_cond_ty(cond_ty)?;
        let then_expr = lower_str_join_parts_term_with_aliases(
            xs[2],
            sep.clone(),
            scope,
            vec_aliases,
            planner,
        )?;
        let else_expr =
            lower_str_join_parts_term_with_aliases(xs[3], sep, scope, vec_aliases, planner)?;
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: Ty::StrI32,
        });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "let") {
        if xs.len() < 3 {
            return Err(Stage2CompileError::Unsupported(
                "(let ((x e) ...) body...) expects bindings and body".to_string(),
            ));
        }
        let Some(bs) = xs[1].as_proper_list() else {
            return Err(Stage2CompileError::Unsupported(
                "(let ...) bindings must be a list".to_string(),
            ));
        };
        let mut env2 = scope.env.clone();
        let mut local_fn_defs2 = scope.local_fn_defs.clone();
        let mut vec_aliases2 = vec_aliases.clone();
        let mut bindings = Vec::with_capacity(bs.len());
        for b in bs {
            let Some(pair) = b.as_proper_list() else {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding must be a list (name expr)".to_string(),
                ));
            };
            if pair.len() != 2 {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding must have exactly 2 forms".to_string(),
                ));
            }
            let Term::Symbol(name) = pair[0] else {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding name must be symbol".to_string(),
                ));
            };
            if let Some(items) = term_const_vector_expr_with_aliases(
                pair[1],
                &vec_aliases2,
                &planner.global_const_vector_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                vec_aliases2.insert(name.clone(), items);
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && !local_fn_defs2.contains_key(sym)
            {
                if let Some(items) = vec_aliases2.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
                    continue;
                }
                if let Some(items) = planner.global_const_vector_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
                    continue;
                }
            }
            if let Some((param, body)) = desugar_fn_literal_to_unary(pair[1])? {
                env2.remove(name);
                local_fn_defs2.insert(
                    name.clone(),
                    InlinableFnDef {
                        param,
                        body,
                        capture: FnCapture::Lexical(env2.clone()),
                    },
                );
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && let Some(alias_fn) =
                    resolve_inlinable_symbol(sym, scope.fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                scope.global_env,
                scope.fn_defs,
                &local_fn_defs2,
                planner,
            )?;
            let idx = planner.alloc_local(rhs.ty())?;
            record_local_const_ids(planner, idx, &rhs);
            env2.insert(name.clone(), Local { idx, ty: rhs.ty() });
            local_fn_defs2.remove(name);
            bindings.push(LetBinding { idx, expr: rhs });
        }

        let mut body = Vec::with_capacity(xs.len() - 2);
        if xs.len() > 3 {
            for x in xs.iter().skip(2).take(xs.len() - 3) {
                body.push(plan_expr(
                    x,
                    &env2,
                    scope.global_env,
                    scope.fn_defs,
                    &local_fn_defs2,
                    planner,
                )?);
            }
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("str/join let had empty body".to_string())
        })?;
        let scope2 = VecGetScope {
            env: &env2,
            global_env: scope.global_env,
            fn_defs: scope.fn_defs,
            local_fn_defs: &local_fn_defs2,
        };
        body.push(lower_str_join_parts_term_with_aliases(
            last,
            sep,
            &scope2,
            &vec_aliases2,
            planner,
        )?);
        return Ok(PExpr::Let {
            bindings,
            body,
            ty: Ty::StrI32,
        });
    }

    Err(Stage2CompileError::Unsupported(
        "str/join currently requires stage2-known vector literals".to_string(),
    ))
}

pub(super) fn lower_str_join_sep_expr(
    parts_ids: Vec<i32>,
    sep: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(sep_id) = planner_const_string_id(planner, &sep) {
        return lower_str_join_const_pair(parts_ids, sep, sep_id, planner);
    }
    match sep {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal(
                    "str/join separator begin had no expressions".to_string(),
                )
            })?;
            let lowered = lower_str_join_sep_expr(parts_ids, last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin {
                exprs,
                ty: Ty::StrI32,
            })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/join separator let had empty body".to_string())
            })?;
            let lowered = lower_str_join_sep_expr(parts_ids, last, planner)?;
            body.push(lowered);
            Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::StrI32,
            })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::StrI32,
        } => {
            let then_lowered = lower_str_join_sep_expr(parts_ids.clone(), *then_expr, planner)?;
            let else_lowered = lower_str_join_sep_expr(parts_ids, *else_expr, planner)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "str/join currently requires a stage2-known string separator".to_string(),
        )),
    }
}

pub(super) fn lower_bytes_join_term(
    parts_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    lower_bytes_join_parts_term(parts_t, env, global_env, fn_defs, local_fn_defs, planner)
}

pub(super) fn lower_bytes_join_parts_term(
    parts_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let scope = VecGetScope {
        env,
        global_env,
        fn_defs,
        local_fn_defs,
    };
    let vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
    lower_bytes_join_parts_term_with_aliases(parts_t, &scope, &vec_aliases, planner)
}

pub(super) fn lower_bytes_join_parts_term_with_aliases(
    parts_t: &Term,
    scope: &VecGetScope<'_>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let global_vec_aliases = planner.global_const_vector_aliases.clone();
    if let Some(parts_ids) = term_const_bytes_vector_ids_with_aliases(
        parts_t,
        vec_aliases,
        &global_vec_aliases,
        planner,
    )? {
        return lower_bytes_join_const_parts(parts_ids, planner);
    }

    let Some(xs) = parts_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "bytes/join currently requires stage2-known vector literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "bytes/join currently requires stage2-known vector literals".to_string(),
        ));
    }

    if matches!(xs[0], Term::Symbol(s) if s == "begin") {
        if xs.len() < 2 {
            return Err(Stage2CompileError::Unsupported(
                "begin must have at least one expression".to_string(),
            ));
        }
        let mut exprs = Vec::with_capacity(xs.len() - 1);
        for x in xs.iter().skip(1).take(xs.len().saturating_sub(2)) {
            exprs.push(plan_expr(
                x,
                scope.env,
                scope.global_env,
                scope.fn_defs,
                scope.local_fn_defs,
                planner,
            )?);
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("bytes/join begin had no body".to_string())
        })?;
        exprs.push(lower_bytes_join_parts_term_with_aliases(
            last,
            scope,
            vec_aliases,
            planner,
        )?);
        return Ok(PExpr::Begin {
            exprs,
            ty: Ty::BytesI32,
        });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(
            xs[1],
            scope.env,
            scope.global_env,
            scope.fn_defs,
            scope.local_fn_defs,
            planner,
        )?;
        let cond_ty = cond.ty();
        ensure_scalar_cond_ty(cond_ty)?;
        let then_expr =
            lower_bytes_join_parts_term_with_aliases(xs[2], scope, vec_aliases, planner)?;
        let else_expr =
            lower_bytes_join_parts_term_with_aliases(xs[3], scope, vec_aliases, planner)?;
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: Ty::BytesI32,
        });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "let") {
        if xs.len() < 3 {
            return Err(Stage2CompileError::Unsupported(
                "(let ((x e) ...) body...) expects bindings and body".to_string(),
            ));
        }
        let Some(bs) = xs[1].as_proper_list() else {
            return Err(Stage2CompileError::Unsupported(
                "(let ...) bindings must be a list".to_string(),
            ));
        };
        let mut env2 = scope.env.clone();
        let mut local_fn_defs2 = scope.local_fn_defs.clone();
        let mut vec_aliases2 = vec_aliases.clone();
        let mut bindings = Vec::with_capacity(bs.len());
        for b in bs {
            let Some(pair) = b.as_proper_list() else {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding must be a list (name expr)".to_string(),
                ));
            };
            if pair.len() != 2 {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding must have exactly 2 forms".to_string(),
                ));
            }
            let Term::Symbol(name) = pair[0] else {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding name must be symbol".to_string(),
                ));
            };
            if let Some(items) = term_const_vector_expr_with_aliases(
                pair[1],
                &vec_aliases2,
                &planner.global_const_vector_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                vec_aliases2.insert(name.clone(), items);
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && !local_fn_defs2.contains_key(sym)
            {
                if let Some(items) = vec_aliases2.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
                    continue;
                }
                if let Some(items) = planner.global_const_vector_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
                    continue;
                }
            }
            if let Some((param, body)) = desugar_fn_literal_to_unary(pair[1])? {
                env2.remove(name);
                local_fn_defs2.insert(
                    name.clone(),
                    InlinableFnDef {
                        param,
                        body,
                        capture: FnCapture::Lexical(env2.clone()),
                    },
                );
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && let Some(alias_fn) =
                    resolve_inlinable_symbol(sym, scope.fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                scope.global_env,
                scope.fn_defs,
                &local_fn_defs2,
                planner,
            )?;
            let idx = planner.alloc_local(rhs.ty())?;
            record_local_const_ids(planner, idx, &rhs);
            env2.insert(name.clone(), Local { idx, ty: rhs.ty() });
            local_fn_defs2.remove(name);
            bindings.push(LetBinding { idx, expr: rhs });
        }

        let mut body = Vec::with_capacity(xs.len() - 2);
        if xs.len() > 3 {
            for x in xs.iter().skip(2).take(xs.len() - 3) {
                body.push(plan_expr(
                    x,
                    &env2,
                    scope.global_env,
                    scope.fn_defs,
                    &local_fn_defs2,
                    planner,
                )?);
            }
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("bytes/join let had empty body".to_string())
        })?;
        let scope2 = VecGetScope {
            env: &env2,
            global_env: scope.global_env,
            fn_defs: scope.fn_defs,
            local_fn_defs: &local_fn_defs2,
        };
        body.push(lower_bytes_join_parts_term_with_aliases(
            last,
            &scope2,
            &vec_aliases2,
            planner,
        )?);
        return Ok(PExpr::Let {
            bindings,
            body,
            ty: Ty::BytesI32,
        });
    }

    Err(Stage2CompileError::Unsupported(
        "bytes/join currently requires stage2-known vector literals".to_string(),
    ))
}
