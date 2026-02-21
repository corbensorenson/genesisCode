use super::*;

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
