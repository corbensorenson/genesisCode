fn effective_step_limit(
    manifest: &PackageManifest,
    cli: StepLimit,
) -> Result<StepLimit, ObligationError> {
    let pkg = manifest
        .limits
        .step_limit
        .map(StepLimit::Limit)
        .unwrap_or(StepLimit::Default);

    if cli == StepLimit::Unlimited && !manifest.limits.allow_unlimited {
        return Err(ObligationError::Manifest(
            "package policy forbids --no-step-limit (set [limits].allow_unlimited = true to permit)"
                .to_string(),
        ));
    }

    if cli == StepLimit::Unlimited {
        return Ok(StepLimit::Unlimited);
    }

    // Both are expected finite here (Default or explicit Limit), but keep this path
    // non-panicking so malformed/internal states surface as typed errors.
    let cli_n = cli.resolve().ok_or_else(|| {
        ObligationError::Manifest("invalid CLI step limit resolution (expected finite)".to_string())
    })?;
    let pkg_n = pkg.resolve().ok_or_else(|| {
        ObligationError::Manifest(
            "invalid package step limit resolution (expected finite)".to_string(),
        )
    })?;
    Ok(StepLimit::Limit(cli_n.min(pkg_n)))
}

fn effective_mem_limits(manifest: &PackageManifest, cli: MemLimits) -> MemLimits {
    fn min_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
        match (a, b) {
            (None, None) => None,
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (Some(x), Some(y)) => Some(x.min(y)),
        }
    }

    MemLimits {
        max_pair_cells: min_opt(cli.max_pair_cells, manifest.limits.max_pair_cells),
        max_vec_len: min_opt(cli.max_vec_len, manifest.limits.max_vec_len),
        max_map_len: min_opt(cli.max_map_len, manifest.limits.max_map_len),
        max_bytes_len: min_opt(cli.max_bytes_len, manifest.limits.max_bytes_len),
        max_string_len: min_opt(cli.max_string_len, manifest.limits.max_string_len),
    }
}

fn extract_protocol_error(ctx: &EvalCtx, v: &Value) -> Option<String> {
    let tok = ctx.protocol?.error;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != tok {
        return None;
    }
    let payload_term = payload.to_term_for_log(Some(tok));
    match &payload_term {
        Term::Map(m) => {
            let code = m
                .get(&TermOrdKey(Term::symbol(":error/code")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("core/error");
            let msg = m
                .get(&TermOrdKey(Term::symbol(":error/message")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("error");
            Some(format!("{code}: {msg}"))
        }
        _ => Some(print_term(&payload_term)),
    }
}

fn selfhost_parse_canonicalize_module(
    ctx: &mut EvalCtx,
    env: &Env,
    src: &str,
) -> Result<Vec<Term>, ObligationError> {
    if let Some(canon_src_fn) = env.get("core/cli::canonicalize-module-src") {
        let out = canon_src_fn
            .apply(ctx, Value::data(Term::Str(src.to_string())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli canonicalize-module-src failed: {e}"
            )));
        }
        let Some(Term::Vector(forms)) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli canonicalize-module-src returned non-vector: {}",
                out.debug_repr()
            )));
        };
        return Ok(forms.clone());
    }

    let parse_fn = env.get("selfhost/parse::parse-module").ok_or_else(|| {
        ObligationError::Module("missing binding selfhost/parse::parse-module".to_string())
    })?;
    let parsed = parse_fn
        .apply(ctx, Value::data(Term::Str(src.to_string())))
        .map_err(|e| ObligationError::Module(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &parsed) {
        return Err(ObligationError::Module(format!(
            "selfhost parse-module failed: {e}"
        )));
    }
    let Some(Term::Vector(parsed_forms)) = parsed.as_data() else {
        return Err(ObligationError::Module(format!(
            "selfhost parse-module returned non-vector: {}",
            parsed.debug_repr()
        )));
    };

    let canon_fn = env
        .get("selfhost/canon::canonicalize-module")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing binding selfhost/canon::canonicalize-module".to_string(),
            )
        })?;
    let canon = canon_fn
        .apply(ctx, Value::data(Term::Vector(parsed_forms.clone())))
        .map_err(|e| ObligationError::Module(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &canon) {
        return Err(ObligationError::Module(format!(
            "selfhost canonicalize-module failed: {e}"
        )));
    }
    let Some(Term::Vector(forms)) = canon.as_data() else {
        return Err(ObligationError::Module(format!(
            "selfhost canonicalize-module returned non-vector: {}",
            canon.debug_repr()
        )));
    };
    Ok(forms.clone())
}

fn selfhost_extract_module_meta(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<Option<Term>, ObligationError> {
    if let Some(meta_fn) = env.get("core/cli::module-meta") {
        let out = meta_fn
            .apply(ctx, Value::data(Term::Vector(forms.to_vec())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli module-meta failed: {e}"
            )));
        }
        let Some(meta_term) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli module-meta returned non-data: {}",
                out.debug_repr()
            )));
        };
        return match meta_term {
            Term::Map(m) => Ok(Some(Term::Map(m.clone()))),
            Term::Nil => Ok(None),
            _ => Err(ObligationError::Module(format!(
                "selfhost core/cli module-meta returned non-map/non-nil: {}",
                out.debug_repr()
            ))),
        };
    }
    Ok(extract_meta_static(forms))
}

fn parse_hex32_str(hex: &str, context: &str) -> Result<[u8; 32], ObligationError> {
    let s = hex.trim();
    if s.len() != 64 {
        return Err(ObligationError::Module(format!(
            "{context} returned non-64-byte hex hash"
        )));
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16).ok_or_else(|| {
            ObligationError::Module(format!("{context} returned invalid hex hash"))
        })?;
        let lo = (chunk[1] as char).to_digit(16).ok_or_else(|| {
            ObligationError::Module(format!("{context} returned invalid hex hash"))
        })?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}

fn selfhost_hash_module_forms(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<[u8; 32], ObligationError> {
    if let Some(hash_forms_fn) = env.get("core/cli::hash-module-forms") {
        let out = hash_forms_fn
            .apply(ctx, Value::data(Term::Vector(forms.to_vec())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli hash-module-forms failed: {e}"
            )));
        }
        let Some(Term::Str(hex)) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli hash-module-forms returned non-string: {}",
                out.debug_repr()
            )));
        };
        return parse_hex32_str(hex, "selfhost core/cli hash-module-forms");
    }

    if let Some(hash_fn) = env.get("selfhost/hash::hash-module") {
        let out = hash_fn
            .apply(ctx, Value::data(Term::Vector(forms.to_vec())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost hash-module failed: {e}"
            )));
        }
        let Some(Term::Str(hex)) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost hash-module returned non-string: {}",
                out.debug_repr()
            )));
        };
        return parse_hex32_str(hex, "selfhost hash-module");
    }

    Err(ObligationError::Module(
        "missing binding core/cli::hash-module-forms or selfhost/hash::hash-module".to_string(),
    ))
}

fn selfhost_optimize_module_forms(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<Vec<Term>, ObligationError> {
    let optimize_fn = env.get("core/cli::optimize-module").ok_or_else(|| {
        ObligationError::Module("missing binding core/cli::optimize-module".to_string())
    })?;
    let out = optimize_fn
        .apply(ctx, Value::data(Term::Vector(forms.to_vec())))
        .map_err(|e| ObligationError::Opt(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &out) {
        return Err(ObligationError::Opt(format!(
            "selfhost core/cli optimize-module failed: {e}"
        )));
    }
    let Some(Term::Vector(opt_forms)) = out.as_data() else {
        return Err(ObligationError::Opt(format!(
            "selfhost core/cli optimize-module returned non-vector: {}",
            out.debug_repr()
        )));
    };
    Ok(opt_forms.clone())
}

fn selfhost_infer_effects_forms(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<gc_types::InferredEffects, ObligationError> {
    let infer_fn = env.get("core/cli::infer-effects").ok_or_else(|| {
        ObligationError::Typecheck("missing binding core/cli::infer-effects".to_string())
    })?;
    let out = infer_fn
        .apply(ctx, Value::data(Term::Vector(forms.to_vec())))
        .map_err(|e| ObligationError::Typecheck(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &out) {
        return Err(ObligationError::Typecheck(format!(
            "selfhost core/cli infer-effects failed: {e}"
        )));
    }
    let out_term = out
        .as_data()
        .cloned()
        .unwrap_or_else(|| out.to_term_for_log(ctx.protocol.map(|p| p.error)));
    let Term::Map(m) = out_term else {
        return Err(ObligationError::Typecheck(format!(
            "selfhost core/cli infer-effects returned non-map: {}",
            out.debug_repr()
        )));
    };

    let mut ops = BTreeSet::new();
    let ops_term = m
        .get(&TermOrdKey(Term::symbol(":ops")))
        .ok_or_else(|| {
            ObligationError::Typecheck(
                "selfhost core/cli infer-effects result missing :ops".to_string(),
            )
        })?
        .clone();
    let Term::Vector(xs) = ops_term else {
        return Err(ObligationError::Typecheck(
            "selfhost core/cli infer-effects :ops must be vector".to_string(),
        ));
    };
    for x in xs {
        match x {
            Term::Symbol(s) => {
                ops.insert(s);
            }
            other => {
                return Err(ObligationError::Typecheck(format!(
                    "selfhost core/cli infer-effects :ops must contain symbols, got {}",
                    print_term(&other)
                )));
            }
        }
    }

    let unknown = match m.get(&TermOrdKey(Term::symbol(":unknown"))) {
        Some(Term::Bool(b)) => *b,
        Some(other) => {
            return Err(ObligationError::Typecheck(format!(
                "selfhost core/cli infer-effects :unknown must be bool, got {}",
                print_term(other)
            )));
        }
        None => false,
    };

    Ok(gc_types::InferredEffects { ops, unknown })
}

