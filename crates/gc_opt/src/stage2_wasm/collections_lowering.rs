use super::*;

pub(super) fn lower_vec_get_terms(
    vec_t: &Term,
    idx_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let idx = plan_expr(idx_t, env, global_env, fn_defs, local_fn_defs, planner)?;
    if idx.ty() != Ty::I64 {
        return Err(Stage2CompileError::Unsupported(
            "vec/get expects (vector, int) arguments in stage2".to_string(),
        ));
    }
    let scope = VecGetScope {
        env,
        global_env,
        fn_defs,
        local_fn_defs,
    };
    let vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
    lower_vec_get_vec_term_with_aliases(vec_t, idx, &scope, &vec_aliases, planner)
}

pub(super) fn lower_vec_get_vec_term_with_aliases(
    vec_t: &Term,
    idx: PExpr,
    scope: &VecGetScope<'_>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let global_vec_aliases = planner.global_const_vector_aliases.clone();
    if let Some(items) = term_const_scalar_vector_exprs_with_aliases(
        vec_t,
        vec_aliases,
        &global_vec_aliases,
        planner,
    )? {
        return lower_vec_get_index_expr(items, idx, planner);
    }

    let Some(xs) = vec_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "vec/get currently requires stage2-known vector literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "vec/get currently requires stage2-known vector literals".to_string(),
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
        let last = xs
            .last()
            .copied()
            .ok_or_else(|| Stage2CompileError::Internal("vec/get begin had no body".to_string()))?;
        exprs.push(lower_vec_get_vec_term_with_aliases(
            last,
            idx,
            scope,
            vec_aliases,
            planner,
        )?);
        let ty = exprs
            .last()
            .map(PExpr::ty)
            .ok_or_else(|| Stage2CompileError::Internal("vec/get begin had no body".to_string()))?;
        return Ok(PExpr::Begin { exprs, ty });
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
            lower_vec_get_vec_term_with_aliases(xs[2], idx.clone(), scope, vec_aliases, planner)?;
        let else_expr =
            lower_vec_get_vec_term_with_aliases(xs[3], idx, scope, vec_aliases, planner)?;
        if then_expr.ty() != else_expr.ty() {
            return Err(Stage2CompileError::Unsupported(
                "vec/get branch variants must resolve to matching scalar result types".to_string(),
            ));
        }
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr.clone()),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: then_expr.ty(),
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
            let local_idx = planner.alloc_local(rhs.ty())?;
            record_local_const_ids(planner, local_idx, &rhs);
            env2.insert(
                name.clone(),
                Local {
                    idx: local_idx,
                    ty: rhs.ty(),
                },
            );
            local_fn_defs2.remove(name);
            bindings.push(LetBinding {
                idx: local_idx,
                expr: rhs,
            });
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
            Stage2CompileError::Internal("vec/get let had empty body".to_string())
        })?;
        let scope2 = VecGetScope {
            env: &env2,
            global_env: scope.global_env,
            fn_defs: scope.fn_defs,
            local_fn_defs: &local_fn_defs2,
        };
        body.push(lower_vec_get_vec_term_with_aliases(
            last,
            idx,
            &scope2,
            &vec_aliases2,
            planner,
        )?);
        let ty = body.last().map(PExpr::ty).ok_or_else(|| {
            Stage2CompileError::Internal("vec/get let had empty body".to_string())
        })?;
        return Ok(PExpr::Let { bindings, body, ty });
    }

    Err(Stage2CompileError::Unsupported(
        "vec/get currently requires stage2-known vector literals".to_string(),
    ))
}

pub(super) fn lower_vec_get_index_expr(
    items: Vec<PExpr>,
    idx: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(i) = planner_const_int_value(planner, &idx) {
        return lower_vec_get_const_pair(items, idx, i, planner);
    }
    match idx {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("vec/get index begin had no expressions".to_string())
            })?;
            let lowered = lower_vec_get_index_expr(items, last, planner)?;
            let ty = lowered.ty();
            exprs.push(lowered);
            Ok(PExpr::Begin { exprs, ty })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("vec/get index let had empty body".to_string())
            })?;
            let lowered = lower_vec_get_index_expr(items, last, planner)?;
            let ty = lowered.ty();
            body.push(lowered);
            Ok(PExpr::Let { bindings, body, ty })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::I64,
        } => {
            let then_lowered = lower_vec_get_index_expr(items.clone(), *then_expr, planner)?;
            let else_lowered = lower_vec_get_index_expr(items, *else_expr, planner)?;
            if then_lowered.ty() != else_lowered.ty() {
                return Err(Stage2CompileError::Unsupported(
                    "vec/get branch indices must resolve to matching scalar result types"
                        .to_string(),
                ));
            }
            let out_ty = then_lowered.ty();
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: out_ty,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "vec/get currently requires stage2-known int indices".to_string(),
        )),
    }
}

pub(super) fn lower_map_get_terms(
    map_t: &Term,
    key_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let key = plan_expr(key_t, env, global_env, fn_defs, local_fn_defs, planner)?;
    if !matches!(
        key.ty(),
        Ty::NilI32 | Ty::BoolI32 | Ty::I64 | Ty::SymI32 | Ty::StrI32 | Ty::BytesI32
    ) {
        return Err(Stage2CompileError::Unsupported(
            "map/get expects a scalar data key in stage2".to_string(),
        ));
    }
    lower_map_get_map_term(map_t, key, env, global_env, fn_defs, local_fn_defs, planner)
}

pub(super) fn lower_map_get_map_term(
    map_t: &Term,
    key: PExpr,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if !(matches!(map_t, Term::Symbol(sym) if env.contains_key(sym) || local_fn_defs.contains_key(sym)))
    {
        let empty_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> = BTreeMap::new();
        if let Some(entries) = term_const_map_expr_with_aliases(
            map_t,
            &empty_aliases,
            &planner.global_const_map_aliases,
        )? {
            return lower_map_get_key_expr(entries, key, planner);
        }
    }

    let Some(xs) = map_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "map/get currently requires stage2-known map literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "map/get currently requires stage2-known map literals".to_string(),
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
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            )?);
        }
        let last = xs
            .last()
            .copied()
            .ok_or_else(|| Stage2CompileError::Internal("map/get begin had no body".to_string()))?;
        let lowered =
            lower_map_get_map_term(last, key, env, global_env, fn_defs, local_fn_defs, planner)?;
        let ty = lowered.ty();
        exprs.push(lowered);
        return Ok(PExpr::Begin { exprs, ty });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        let cond_ty = cond.ty();
        ensure_scalar_cond_ty(cond_ty)?;
        let then_expr = lower_map_get_map_term(
            xs[2],
            key.clone(),
            env,
            global_env,
            fn_defs,
            local_fn_defs,
            planner,
        )?;
        let else_expr =
            lower_map_get_map_term(xs[3], key, env, global_env, fn_defs, local_fn_defs, planner)?;
        if then_expr.ty() != else_expr.ty() {
            return Err(Stage2CompileError::Unsupported(
                "map/get branch variants must resolve to matching scalar result types".to_string(),
            ));
        }
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr.clone()),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: then_expr.ty(),
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
        let mut env2 = env.clone();
        let mut local_fn_defs2 = local_fn_defs.clone();
        let mut map_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> = BTreeMap::new();
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
            if let Some(items) = term_const_map_expr_with_aliases(
                pair[1],
                &map_aliases,
                &planner.global_const_map_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                map_aliases.insert(name.clone(), items);
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && !local_fn_defs2.contains_key(sym)
            {
                if let Some(items) = map_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    map_aliases.insert(name.clone(), items);
                    continue;
                }
                if let Some(items) = planner.global_const_map_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    map_aliases.insert(name.clone(), items);
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
                && let Some(alias_fn) = resolve_inlinable_symbol(sym, fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                global_env,
                fn_defs,
                &local_fn_defs2,
                planner,
            )?;
            let local_idx = planner.alloc_local(rhs.ty())?;
            record_local_const_ids(planner, local_idx, &rhs);
            env2.insert(
                name.clone(),
                Local {
                    idx: local_idx,
                    ty: rhs.ty(),
                },
            );
            local_fn_defs2.remove(name);
            bindings.push(LetBinding {
                idx: local_idx,
                expr: rhs,
            });
        }

        let mut body = Vec::with_capacity(xs.len() - 2);
        if xs.len() > 3 {
            for x in xs.iter().skip(2).take(xs.len() - 3) {
                body.push(plan_expr(
                    x,
                    &env2,
                    global_env,
                    fn_defs,
                    &local_fn_defs2,
                    planner,
                )?);
            }
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("map/get let had empty body".to_string())
        })?;
        let resolved_last = resolve_map_alias_term(last, &map_aliases);
        let lowered = lower_map_get_map_term(
            &resolved_last,
            key,
            &env2,
            global_env,
            fn_defs,
            &local_fn_defs2,
            planner,
        )?;
        let ty = lowered.ty();
        body.push(lowered);
        return Ok(PExpr::Let { bindings, body, ty });
    }

    Err(Stage2CompileError::Unsupported(
        "map/get currently requires stage2-known map literals".to_string(),
    ))
}

pub(super) fn lower_map_get_key_expr(
    entries: BTreeMap<TermOrdKey, Term>,
    key: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(k) = scalar_term_from_pexpr_const(planner, &key) {
        return lower_map_get_const_pair(entries, key, k, planner);
    }
    match key {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("map/get key begin had no expressions".to_string())
            })?;
            let lowered = lower_map_get_key_expr(entries, last, planner)?;
            let ty = lowered.ty();
            exprs.push(lowered);
            Ok(PExpr::Begin { exprs, ty })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("map/get key let had empty body".to_string())
            })?;
            let lowered = lower_map_get_key_expr(entries, last, planner)?;
            let ty = lowered.ty();
            body.push(lowered);
            Ok(PExpr::Let { bindings, body, ty })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: _,
        } => {
            let then_lowered = lower_map_get_key_expr(entries.clone(), *then_expr, planner)?;
            let else_lowered = lower_map_get_key_expr(entries, *else_expr, planner)?;
            if then_lowered.ty() != else_lowered.ty() {
                return Err(Stage2CompileError::Unsupported(
                    "map/get branch keys must resolve to matching scalar result types".to_string(),
                ));
            }
            let out_ty = then_lowered.ty();
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: out_ty,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "map/get currently requires stage2-known scalar keys".to_string(),
        )),
    }
}

pub(super) fn lower_map_get_const_pair(
    entries: BTreeMap<TermOrdKey, Term>,
    key: PExpr,
    key_term: Term,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let chosen_term = entries
        .get(&TermOrdKey(key_term))
        .cloned()
        .unwrap_or(Term::Nil);
    let chosen = scalar_term_to_pexpr(&chosen_term, planner)?.ok_or_else(|| {
        Stage2CompileError::Unsupported(
            "map/get currently requires selected values to be scalar in stage2".to_string(),
        )
    })?;
    let key_local = planner.alloc_local(key.ty())?;
    let ty = chosen.ty();
    Ok(PExpr::Let {
        bindings: vec![LetBinding {
            idx: key_local,
            expr: key,
        }],
        body: vec![chosen],
        ty,
    })
}

pub(super) fn lower_vec_len_term(
    vec_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
    lower_vec_len_term_with_aliases(
        vec_t,
        env,
        global_env,
        fn_defs,
        local_fn_defs,
        &vec_aliases,
        planner,
    )
}

pub(super) fn lower_vec_len_term_with_aliases(
    vec_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(items) = term_const_vector_expr_with_aliases(
        vec_t,
        vec_aliases,
        &planner.global_const_vector_aliases,
    )? {
        let n = i64::try_from(items.len()).map_err(|_| {
            Stage2CompileError::Internal("vec/len literal length does not fit i64".to_string())
        })?;
        return Ok(PExpr::Int(n));
    }

    let Some(xs) = vec_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "vec/len currently requires stage2-known vector literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "vec/len currently requires stage2-known vector literals".to_string(),
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
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            )?);
        }
        let last = xs
            .last()
            .copied()
            .ok_or_else(|| Stage2CompileError::Internal("vec/len begin had no body".to_string()))?;
        exprs.push(lower_vec_len_term_with_aliases(
            last,
            env,
            global_env,
            fn_defs,
            local_fn_defs,
            vec_aliases,
            planner,
        )?);
        return Ok(PExpr::Begin { exprs, ty: Ty::I64 });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        let cond_ty = cond.ty();
        ensure_scalar_cond_ty(cond_ty)?;
        let then_expr = lower_vec_len_term_with_aliases(
            xs[2],
            env,
            global_env,
            fn_defs,
            local_fn_defs,
            vec_aliases,
            planner,
        )?;
        let else_expr = lower_vec_len_term_with_aliases(
            xs[3],
            env,
            global_env,
            fn_defs,
            local_fn_defs,
            vec_aliases,
            planner,
        )?;
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: Ty::I64,
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
        let mut env2 = env.clone();
        let mut local_fn_defs2 = local_fn_defs.clone();
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
                && let Some(alias_fn) = resolve_inlinable_symbol(sym, fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                global_env,
                fn_defs,
                &local_fn_defs2,
                planner,
            )?;
            let local_idx = planner.alloc_local(rhs.ty())?;
            record_local_const_ids(planner, local_idx, &rhs);
            env2.insert(
                name.clone(),
                Local {
                    idx: local_idx,
                    ty: rhs.ty(),
                },
            );
            local_fn_defs2.remove(name);
            bindings.push(LetBinding {
                idx: local_idx,
                expr: rhs,
            });
        }

        let mut body = Vec::with_capacity(xs.len() - 2);
        if xs.len() > 3 {
            for x in xs.iter().skip(2).take(xs.len() - 3) {
                body.push(plan_expr(
                    x,
                    &env2,
                    global_env,
                    fn_defs,
                    &local_fn_defs2,
                    planner,
                )?);
            }
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("vec/len let had empty body".to_string())
        })?;
        body.push(lower_vec_len_term_with_aliases(
            last,
            &env2,
            global_env,
            fn_defs,
            &local_fn_defs2,
            &vec_aliases2,
            planner,
        )?);
        return Ok(PExpr::Let {
            bindings,
            body,
            ty: Ty::I64,
        });
    }

    Err(Stage2CompileError::Unsupported(
        "vec/len currently requires stage2-known vector literals".to_string(),
    ))
}

pub(super) fn lower_map_len_term(
    map_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if !(matches!(map_t, Term::Symbol(sym) if env.contains_key(sym) || local_fn_defs.contains_key(sym)))
    {
        let empty_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> = BTreeMap::new();
        if let Some(items) = term_const_map_expr_with_aliases(
            map_t,
            &empty_aliases,
            &planner.global_const_map_aliases,
        )? {
            let n = i64::try_from(items.len()).map_err(|_| {
                Stage2CompileError::Internal("map/len literal length does not fit i64".to_string())
            })?;
            return Ok(PExpr::Int(n));
        }
    }

    let Some(xs) = map_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "map/len currently requires stage2-known map literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "map/len currently requires stage2-known map literals".to_string(),
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
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            )?);
        }
        let last = xs
            .last()
            .copied()
            .ok_or_else(|| Stage2CompileError::Internal("map/len begin had no body".to_string()))?;
        exprs.push(lower_map_len_term(
            last,
            env,
            global_env,
            fn_defs,
            local_fn_defs,
            planner,
        )?);
        return Ok(PExpr::Begin { exprs, ty: Ty::I64 });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        let cond_ty = cond.ty();
        ensure_scalar_cond_ty(cond_ty)?;
        let then_expr =
            lower_map_len_term(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        let else_expr =
            lower_map_len_term(xs[3], env, global_env, fn_defs, local_fn_defs, planner)?;
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: Ty::I64,
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
        let mut env2 = env.clone();
        let mut local_fn_defs2 = local_fn_defs.clone();
        let mut map_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> = BTreeMap::new();
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
            if let Some(items) = term_const_map_expr_with_aliases(
                pair[1],
                &map_aliases,
                &planner.global_const_map_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                map_aliases.insert(name.clone(), items);
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && !local_fn_defs2.contains_key(sym)
            {
                if let Some(items) = map_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    map_aliases.insert(name.clone(), items);
                    continue;
                }
                if let Some(items) = planner.global_const_map_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    map_aliases.insert(name.clone(), items);
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
                && let Some(alias_fn) = resolve_inlinable_symbol(sym, fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                global_env,
                fn_defs,
                &local_fn_defs2,
                planner,
            )?;
            let local_idx = planner.alloc_local(rhs.ty())?;
            record_local_const_ids(planner, local_idx, &rhs);
            env2.insert(
                name.clone(),
                Local {
                    idx: local_idx,
                    ty: rhs.ty(),
                },
            );
            local_fn_defs2.remove(name);
            bindings.push(LetBinding {
                idx: local_idx,
                expr: rhs,
            });
        }

        let mut body = Vec::with_capacity(xs.len() - 2);
        if xs.len() > 3 {
            for x in xs.iter().skip(2).take(xs.len() - 3) {
                body.push(plan_expr(
                    x,
                    &env2,
                    global_env,
                    fn_defs,
                    &local_fn_defs2,
                    planner,
                )?);
            }
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("map/len let had empty body".to_string())
        })?;
        let resolved_last = resolve_map_alias_term(last, &map_aliases);
        body.push(lower_map_len_term(
            &resolved_last,
            &env2,
            global_env,
            fn_defs,
            &local_fn_defs2,
            planner,
        )?);
        return Ok(PExpr::Let {
            bindings,
            body,
            ty: Ty::I64,
        });
    }

    Err(Stage2CompileError::Unsupported(
        "map/len currently requires stage2-known map literals".to_string(),
    ))
}
