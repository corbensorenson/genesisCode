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
        max_alloc_units: min_opt(cli.max_alloc_units, manifest.limits.max_alloc_units),
        max_live_units: min_opt(cli.max_live_units, manifest.limits.max_live_units),
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

#[derive(Debug, Clone)]
struct SelfhostFrontendModule {
    forms: Vec<Term>,
    module_hash: [u8; 32],
}

fn selfhost_frontend_module(
    ctx: &mut EvalCtx,
    env: &Env,
    src: &str,
) -> Result<SelfhostFrontendModule, ObligationError> {
    let frontend_fn = env.get("core/cli::frontend-module").ok_or_else(|| {
        ObligationError::Module(
            "missing required production binding core/cli::frontend-module".to_string(),
        )
    })?;
    let out = frontend_fn
        .apply(ctx, Value::data(Term::Str(src.to_string())))
        .map_err(|e| ObligationError::Module(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &out) {
        return Err(ObligationError::Module(format!(
            "selfhost core/cli frontend-module failed: {e}"
        )));
    }
    let result_term = out.to_term_for_log(ctx.protocol.map(|protocol| protocol.error));
    let Term::Map(result) = &result_term else {
        return Err(ObligationError::Module(format!(
            "selfhost core/cli frontend-module returned non-map: {}",
            out.debug_repr()
        )));
    };
    if result.len() != 8 {
        return Err(ObligationError::Module(
            "selfhost core/cli frontend-module result must contain exactly 8 fields".to_string(),
        ));
    }
    let get = |key: &str| result.get(&TermOrdKey(Term::symbol(key)));
    if !matches!(get(":kind"), Some(Term::Str(kind)) if kind == "genesis/frontend-module-v0.1")
        || !matches!(get(":v"), Some(Term::Int(v)) if v == &1.into())
        || !matches!(get(":profile"), Some(Term::Str(profile)) if profile == "genesis/coreform-canon-hash-v0.2")
        || !matches!(get(":span-unit"), Some(Term::Symbol(unit)) if unit == ":utf8-byte")
    {
        return Err(ObligationError::Module(
            "selfhost core/cli frontend-module identity or profile mismatch".to_string(),
        ));
    }
    let forms = match get(":forms") {
        Some(Term::Vector(forms)) => forms.clone(),
        _ => {
            return Err(ObligationError::Module(
                "selfhost core/cli frontend-module :forms must be a vector".to_string(),
            ));
        }
    };
    if !matches!(get(":canonical-source"), Some(Term::Str(_))) {
        return Err(ObligationError::Module(
            "selfhost core/cli frontend-module :canonical-source must be a string".to_string(),
        ));
    }
    let module_hash = match get(":module-h") {
        Some(Term::Str(hex)) => {
            parse_hex32_str(hex, "selfhost core/cli frontend-module :module-h")?
        }
        _ => {
            return Err(ObligationError::Module(
                "selfhost core/cli frontend-module :module-h must be a hex string".to_string(),
            ));
        }
    };
    let Some(Term::Map(span)) = get(":source-span") else {
        return Err(ObligationError::Module(
            "selfhost core/cli frontend-module :source-span must be a map".to_string(),
        ));
    };
    let expected_end = u64::try_from(src.len()).map_err(|_| {
        ObligationError::Module(
            "source byte length exceeds the supported frontend transport range".to_string(),
        )
    })?;
    if span.len() != 2
        || !matches!(span.get(&TermOrdKey(Term::symbol(":start-byte"))), Some(Term::Int(v)) if v == &0.into())
        || !matches!(span.get(&TermOrdKey(Term::symbol(":end-byte"))), Some(Term::Int(v)) if v.to_string().parse::<u64>() == Ok(expected_end))
    {
        return Err(ObligationError::Module(
            "selfhost core/cli frontend-module :source-span is invalid".to_string(),
        ));
    }
    Ok(SelfhostFrontendModule { forms, module_hash })
}

fn selfhost_parse_canonicalize_module(
    ctx: &mut EvalCtx,
    env: &Env,
    src: &str,
) -> Result<Vec<Term>, ObligationError> {
    Ok(selfhost_frontend_module(ctx, env, src)?.forms)
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
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ObligationError::Module(format!(
            "{context} returned a non-canonical lowercase 64-hex hash"
        )));
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
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
    let hash_forms_fn = env.get("core/cli::hash-module-forms").ok_or_else(|| {
        ObligationError::Module(
            "missing required production binding core/cli::hash-module-forms".to_string(),
        )
    })?;
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
    parse_hex32_str(hex, "selfhost core/cli hash-module-forms")
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

#[cfg(test)]
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
        .map_err(|error| ObligationError::Typecheck(error.to_string()))?;
    if let Some(error) = extract_protocol_error(ctx, &out) {
        return Err(ObligationError::Typecheck(format!(
            "selfhost core/cli infer-effects failed: {error}"
        )));
    }
    let out_term = out
        .as_data()
        .cloned()
        .unwrap_or_else(|| out.to_term_for_log(ctx.protocol.map(|protocol| protocol.error)));
    let Term::Map(map) = out_term else {
        return Err(ObligationError::Typecheck(format!(
            "selfhost core/cli infer-effects returned non-map: {}",
            out.debug_repr()
        )));
    };

    let Term::Vector(op_terms) = map.get(&TermOrdKey(Term::symbol(":ops"))).ok_or_else(|| {
        ObligationError::Typecheck(
            "selfhost core/cli infer-effects result missing :ops".to_string(),
        )
    })?
    else {
        return Err(ObligationError::Typecheck(
            "selfhost core/cli infer-effects :ops must be vector".to_string(),
        ));
    };
    let mut ops = BTreeSet::new();
    for op in op_terms {
        let Term::Symbol(op) = op else {
            return Err(ObligationError::Typecheck(format!(
                "selfhost core/cli infer-effects :ops must contain symbols, got {}",
                print_term(op)
            )));
        };
        if !ops.insert(op.clone()) {
            return Err(ObligationError::Typecheck(format!(
                "selfhost core/cli infer-effects :ops contains duplicate {op}"
            )));
        }
    }
    let unknown = match map.get(&TermOrdKey(Term::symbol(":unknown"))) {
        Some(Term::Bool(value)) => *value,
        Some(other) => {
            return Err(ObligationError::Typecheck(format!(
                "selfhost core/cli infer-effects :unknown must be bool, got {}",
                print_term(other)
            )));
        }
        None => {
            return Err(ObligationError::Typecheck(
                "selfhost core/cli infer-effects result missing :unknown".to_string(),
            ));
        }
    };
    Ok(gc_types::InferredEffects { ops, unknown })
}

#[cfg(test)]
mod logical_limit_tests {
    use super::*;

    #[test]
    fn logical_memory_limits_resolve_to_the_stricter_policy() {
        let dir = tempfile::tempdir().unwrap();
        let package_path = dir.path().join("package.toml");
        std::fs::write(
            &package_path,
            r#"
schema = 1
name = "limit-resolution"
version = "0.0.1"
modules = []
obligations = []

[limits]
max_alloc_units = 80
max_live_units = 120
"#,
        )
        .unwrap();
        let manifest = PackageManifest::load(&package_path).unwrap().0;

        let limits = effective_mem_limits(
            &manifest,
            MemLimits {
                max_alloc_units: Some(100),
                max_live_units: Some(50),
                ..MemLimits::default()
            },
        );
        assert_eq!(limits.max_alloc_units, Some(80));
        assert_eq!(limits.max_live_units, Some(50));

        let limits = effective_mem_limits(&manifest, MemLimits::default());
        assert_eq!(limits.max_alloc_units, Some(80));
        assert_eq!(limits.max_live_units, Some(120));
    }
}
