use super::*;

pub(super) fn eval_module(ctx: &mut EvalCtx, env: &mut Env, forms: &[Term]) -> Result<Value, KernelError> {
    let mut last = Value::Data(Term::Nil);
    for form in forms {
        if let Some((name, expr)) = parse_def(form) {
            let v = eval_term(ctx, env, &expr)?;
            env.set_local(name, v);
            last = Value::Data(Term::Nil);
            continue;
        }
        last = eval_term(ctx, env, form)?;
    }
    Ok(last)
}

fn parse_def(t: &Term) -> Option<(String, Term)> {
    let items = t.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

pub(super) fn eval_let_tco(
    ctx: &mut EvalCtx,
    env: &Env,
    items: Vec<&Term>,
) -> Result<EvalOutcome, KernelError> {
    if items.len() < 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(let ((x e) ...) body...) expects bindings and body",
        ));
    }
    let bindings = items[1];
    let Some(bs) = bindings.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(let ...) bindings must be a list",
        ));
    };

    let mut env2 = env.clone();
    for b in bs {
        let Some(pair) = b.as_proper_list() else {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(let ...) binding must be a list (name expr)",
            ));
        };
        if pair.len() != 2 {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(let ...) binding must have exactly 2 forms",
            ));
        }
        let Term::Symbol(name) = pair[0] else {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(let ...) binding name must be symbol",
            ));
        };
        let rhs = eval_term(ctx, &env2, pair[1])?;
        env2 = Env::with_binding(&env2, name.clone(), rhs);
    }

    // Body: single => that term; multi => (begin ...)
    let body_term = if items.len() == 3 {
        items[2].clone()
    } else {
        let mut xs = Vec::with_capacity(items.len() - 1);
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };

    Ok(EvalOutcome::Tail {
        env: env2,
        term: body_term,
    })
}

pub(super) fn eval_fn(_ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    if items.len() < 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn (x) body...) expects params and body",
        ));
    }
    let params = items[1];
    let Some(ps) = params.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) params must be a list",
        ));
    };
    if ps.is_empty() {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) requires at least 1 parameter",
        ));
    }
    for p in &ps {
        if !matches!(p, Term::Symbol(_)) {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(fn ...) params must be symbols",
            ));
        }
    }

    let body_term = if items.len() == 3 {
        items[2].clone()
    } else {
        // multi-body => (begin ...)
        let mut xs = Vec::with_capacity(items.len() - 1);
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };

    // Desugar multi-arg lambda into nested unary closures.
    let mut out = body_term;
    for p in ps.into_iter().rev() {
        let Term::Symbol(name) = p else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "internal fn desugaring expected symbol parameter",
            ));
        };
        out = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol(name.clone())]),
            out,
        ]);
    }

    // Now out is a unary fn; build closure from it.
    let Some(items2) = out.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring failed",
        ));
    };
    if items2.len() != 3 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced unexpected shape",
        ));
    }
    let params2 = &items2[1];
    let Some(ps2) = params2.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced bad params",
        ));
    };
    if ps2.len() != 1 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-unary params",
        ));
    }
    let Term::Symbol(param) = ps2[0] else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-symbol param",
        ));
    };
    Ok(Value::Closure {
        param: param.clone(),
        body: items2[2].clone(),
        env: env.clone(),
    })
}

pub(super) fn eval_seal(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    match items.len() {
        1 => {
            let id = ctx.state.next_seal_id;
            ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
            Ok(Value::SealToken(SealId(id)))
        }
        3 => {
            let v = eval_term(ctx, env, items[1])?;
            let tok = eval_term(ctx, env, items[2])?;
            let Value::SealToken(id) = tok else {
                return type_err(ctx, "seal expects a seal token as second argument");
            };
            Ok(Value::Sealed {
                token: id,
                payload: Box::new(v),
            })
        }
        _ => Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(seal) or (seal v tok)",
        )),
    }
}

pub(super) fn eval_unseal(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    if items.len() != 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(unseal w tok) expects exactly 2 arguments",
        ));
    }
    let w = eval_term(ctx, env, items[1])?;
    let tok = eval_term(ctx, env, items[2])?;
    let Value::SealToken(id) = tok else {
        return type_err(ctx, "unseal expects a seal token as second argument");
    };
    if let Value::Sealed { token, payload } = w
        && token == id
    {
        return Ok(*payload);
    }
    Ok(Value::Data(Term::Nil))
}

pub(super) fn eval_prim(ctx: &mut EvalCtx, env: &Env, items: Vec<&Term>) -> Result<Value, KernelError> {
    if items.len() < 2 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(prim op ...) expects at least an op",
        ));
    }
    let Term::Symbol(op) = items[1] else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(prim ...) op must be a symbol",
        ));
    };
    let mut args = Vec::with_capacity(items.len().saturating_sub(2));
    for a in items.iter().skip(2) {
        args.push(eval_term(ctx, env, a)?);
    }
    prim(ctx, op, args)
}
