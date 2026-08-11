use super::*;

struct ProtocolErrorDetails {
    code: String,
    message: String,
    payload: Term,
}

fn extract_protocol_error_details(ctx: &EvalCtx, v: &Value) -> Option<ProtocolErrorDetails> {
    let tok = ctx.protocol?.error;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != tok {
        return None;
    }

    let payload_term = payload.to_term_for_log(Some(tok));
    let (code, message) = match &payload_term {
        Term::Map(m) => {
            let code = m
                .get(&gc_coreform::TermOrdKey(Term::Symbol(
                    ":error/code".to_string(),
                )))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "core/error".to_string());
            let message = m
                .get(&gc_coreform::TermOrdKey(Term::Symbol(
                    ":error/message".to_string(),
                )))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "error".to_string());
            (code, message)
        }
        _ => ("core/error".to_string(), "error".to_string()),
    };
    Some(ProtocolErrorDetails {
        code,
        message,
        payload: payload_term,
    })
}

pub(super) fn extract_protocol_error(
    ctx: &EvalCtx,
    v: &Value,
) -> Option<(String, String, Option<String>)> {
    extract_protocol_error_details(ctx, v).map(|details| {
        (
            details.code,
            details.message,
            Some(gc_coreform::print_term(&details.payload)),
        )
    })
}

pub(super) fn ensure_no_protocol_error(
    ctx: &EvalCtx,
    value: &Value,
    suppress: bool,
    domain: &'static str,
    operation: &'static str,
    diagnostic_code: &'static str,
) -> Result<(), CliError> {
    if suppress {
        return Ok(());
    }
    let Some((protocol_code, message, payload)) = extract_protocol_error(ctx, value) else {
        return Ok(());
    };
    Err(cli_err_with_context(
        EX_EVAL,
        diagnostic_code,
        message,
        structured_failures::protocol_context(
            domain,
            operation,
            &protocol_code,
            payload.as_deref(),
        ),
    ))
}

pub(super) fn ensure_no_runner_protocol_error(
    ctx: &EvalCtx,
    value: &Value,
    denied: bool,
) -> Result<(), CliError> {
    ensure_no_protocol_error(ctx, value, denied, "policy", "run/result", "effects/run")
}

#[derive(Debug, Clone)]
pub(super) struct SelfhostFrontendModule {
    pub(super) forms: Vec<Term>,
    pub(super) canonical_source: String,
    pub(super) module_hash: [u8; 32],
    pub(super) source_start_byte: u64,
    pub(super) source_end_byte: u64,
}

pub(super) fn selfhost_frontend_module(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    src: &str,
) -> Result<SelfhostFrontendModule, CliError> {
    selfhost_frontend_module_at(ctx, env, src, Path::new("<source>"))
}

pub(super) fn selfhost_frontend_module_at(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    src: &str,
    path: &Path,
) -> Result<SelfhostFrontendModule, CliError> {
    let frontend_fn = env.get("core/cli::frontend-module").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing required production binding core/cli::frontend-module",
        )
    })?;
    let out = frontend_fn
        .apply(ctx, Value::data(Term::Str(src.to_string())))
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("core/cli frontend-module failed: {e}"),
                structured_failures::evaluator_context("parser/frontend-module", &e),
            )
        })?;
    if let Some(details) = extract_protocol_error_details(ctx, &out) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{}: {}", details.code, details.message),
                context: Some(structured_failures::selfhost_parser_context(
                    "parser/frontend-module",
                    path,
                    src,
                    &details.code,
                    &details.payload,
                )),
            },
        });
    }
    let result_term = out.to_term_for_log(ctx.protocol.map(|protocol| protocol.error));
    let Term::Map(result) = &result_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "core/cli frontend-module returned non-map: {}",
                out.debug_repr()
            ),
        ));
    };
    if result.len() != 8 {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            "core/cli frontend-module result must contain exactly 8 fields",
        ));
    }
    let get = |key: &str| result.get(&TermOrdKey(Term::symbol(key)));
    if !matches!(get(":kind"), Some(Term::Str(kind)) if kind == "genesis/frontend-module-v0.1")
        || !matches!(get(":v"), Some(Term::Int(v)) if v == &1.into())
        || !matches!(get(":profile"), Some(Term::Str(profile)) if profile == "genesis/coreform-canon-hash-v0.2")
        || !matches!(get(":span-unit"), Some(Term::Symbol(unit)) if unit == ":utf8-byte")
    {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            "core/cli frontend-module identity or profile mismatch",
        ));
    }
    let forms = match get(":forms") {
        Some(Term::Vector(forms)) => forms.clone(),
        _ => {
            return Err(cli_err(
                EX_INTERNAL,
                "selfhost/bad-return",
                "core/cli frontend-module :forms must be a vector",
            ));
        }
    };
    let canonical_source = match get(":canonical-source") {
        Some(Term::Str(source)) => source.clone(),
        _ => {
            return Err(cli_err(
                EX_INTERNAL,
                "selfhost/bad-return",
                "core/cli frontend-module :canonical-source must be a string",
            ));
        }
    };
    let module_hash = match get(":module-h") {
        Some(Term::Str(hex)) => parse_hex32_for_cli(hex, "core/cli frontend-module :module-h")?,
        _ => {
            return Err(cli_err(
                EX_INTERNAL,
                "selfhost/bad-return",
                "core/cli frontend-module :module-h must be a hex string",
            ));
        }
    };
    let Some(Term::Map(span)) = get(":source-span") else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            "core/cli frontend-module :source-span must be a map",
        ));
    };
    if span.len() != 2 {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            "core/cli frontend-module :source-span must contain exactly 2 fields",
        ));
    }
    let span_u64 = |key: &str| -> Result<u64, CliError> {
        let Some(Term::Int(value)) = span.get(&TermOrdKey(Term::symbol(key))) else {
            return Err(cli_err(
                EX_INTERNAL,
                "selfhost/bad-return",
                format!("core/cli frontend-module :source-span {key} must be an integer"),
            ));
        };
        value.to_string().parse::<u64>().map_err(|_| {
            cli_err(
                EX_INTERNAL,
                "selfhost/bad-return",
                format!("core/cli frontend-module :source-span {key} is out of range"),
            )
        })
    };
    let source_start_byte = span_u64(":start-byte")?;
    let source_end_byte = span_u64(":end-byte")?;
    let expected_end = u64::try_from(src.len()).map_err(|_| {
        cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            "source byte length exceeds the supported frontend transport range",
        )
    })?;
    if source_start_byte != 0 || source_end_byte != expected_end {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            "core/cli frontend-module source span does not cover the exact UTF-8 source bytes",
        ));
    }
    Ok(SelfhostFrontendModule {
        forms,
        canonical_source,
        module_hash,
        source_start_byte,
        source_end_byte,
    })
}

pub(super) fn selfhost_parse_canonicalize_module(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    src: &str,
) -> Result<Vec<Term>, CliError> {
    Ok(selfhost_frontend_module(ctx, env, src)?.forms)
}

fn parse_hex32_for_cli(hex: &str, context: &str) -> Result<[u8; 32], CliError> {
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/hash",
            format!("{context} returned a non-canonical lowercase 64-hex hash"),
        ));
    }
    let mut out = [0u8; 32];
    for (i, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        let hi = (pair[0] as char).to_digit(16).ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "selfhost/hash",
                format!("{context} returned invalid hex hash"),
            )
        })?;
        let lo = (pair[1] as char).to_digit(16).ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "selfhost/hash",
                format!("{context} returned invalid hex hash"),
            )
        })?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}

pub(super) fn selfhost_hash_module_forms(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    forms: &[Term],
) -> Result<[u8; 32], CliError> {
    let hash_forms_fn = env.get("core/cli::hash-module-forms").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing required production binding core/cli::hash-module-forms",
        )
    })?;
    let out = hash_forms_fn
        .apply(ctx, Value::data(Term::Vector(forms.to_vec())))
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("selfhost hash failed: {e}"),
                structured_failures::evaluator_context("build/hash-module", &e),
            )
        })?;
    if let Some((code, message, payload)) = extract_protocol_error(ctx, &out) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: Some(structured_failures::protocol_context(
                    "build",
                    "build/hash-module",
                    &code,
                    payload.as_deref(),
                )),
            },
        });
    }
    let Some(Term::Str(hex)) = out.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "core/cli hash-module-forms returned non-string: {}",
                out.debug_repr()
            ),
        ));
    };
    parse_hex32_for_cli(hex, "core/cli hash-module-forms")
}

pub(super) fn selfhost_stage1_transform_module(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    forms: &[Term],
) -> Result<Vec<Term>, CliError> {
    let stage1_fn = env
        .get("core/cli::stage1-transform-module")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "selfhost/missing",
                "missing binding core/cli::stage1-transform-module",
            )
        })?;
    let out = stage1_fn
        .apply(ctx, Value::data(Term::Vector(forms.to_vec())))
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("selfhost stage1 failed: {e}"),
                structured_failures::evaluator_context("build/stage1-transform", &e),
            )
        })?;
    if let Some((code, message, payload)) = extract_protocol_error(ctx, &out) {
        return Err(CliError {
            exit_code: EX_INTERNAL,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: Some(structured_failures::protocol_context(
                    "build",
                    "build/stage1-transform",
                    &code,
                    payload.as_deref(),
                )),
            },
        });
    }
    let Some(Term::Vector(transformed)) = out.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "core/cli stage1-transform-module returned non-vector: {}",
                out.debug_repr()
            ),
        ));
    };
    Ok(transformed.clone())
}

pub(super) fn selfhost_parse_term(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    src: &str,
    arg_name: &str,
) -> Result<Term, CliError> {
    let parse_fn = env.get("selfhost/parse::parse-term").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding selfhost/parse::parse-term",
        )
    })?;
    let parsed = parse_fn
        .apply(ctx, Value::data(Term::Str(src.to_string())))
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("selfhost parse-term failed for {arg_name}: {e}"),
                structured_failures::evaluator_context("parser/parse-term", &e),
            )
        })?;

    if let Some((code, message, payload)) = extract_protocol_error(ctx, &parsed) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{arg_name}: {code}: {message}"),
                context: Some(structured_failures::protocol_context(
                    "parser",
                    "parser/parse-term",
                    &code,
                    payload.as_deref(),
                )),
            },
        });
    }

    let Some(term) = parsed.to_plain_term() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "selfhost parse-term returned non-data for {arg_name}: {}",
                parsed.debug_repr()
            ),
        ));
    };
    Ok(term)
}

pub(super) fn selfhost_plan_request_map(
    cli: &Cli,
    binding: &str,
    req: Term,
    cmd_name: &str,
) -> Result<std::collections::BTreeMap<TermOrdKey, Term>, CliError> {
    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

    let f = env.get(binding).ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            format!("missing binding {binding}"),
        )
    })?;
    let out = f.apply(&mut ctx, Value::data(req)).map_err(|e| {
        cli_err_with_context(
            EX_EVAL,
            "eval/error",
            format!("{binding} failed for {cmd_name}: {e}"),
            structured_failures::evaluator_context("package/plan-command", &e),
        )
    })?;

    if let Some((code, message, payload)) = extract_protocol_error(&ctx, &out) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{cmd_name}: {code}: {message}"),
                context: Some(structured_failures::protocol_context(
                    "package",
                    "package/plan-command",
                    &code,
                    payload.as_deref(),
                )),
            },
        });
    }

    if let Some(Term::Map(m)) = out.as_data() {
        return Ok(m.clone());
    }
    let fallback = out.to_term_for_log(ctx.protocol.map(|p| p.error));
    if let Term::Map(m) = fallback {
        return Ok(m);
    }
    Err(cli_err(
        EX_INTERNAL,
        "selfhost/bad-return",
        format!(
            "{binding} returned non-map for {cmd_name}: {}",
            out.debug_repr()
        ),
    ))
}

pub(super) fn planned_required_str(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<String, CliError> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        )),
    }
}

pub(super) fn planned_optional_str(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<Option<String>, CliError> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(Term::Nil) | None => Ok(None),
        _ => Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        )),
    }
}

pub(super) fn planned_required_bool(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<bool, CliError> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bool(b)) => Ok(*b),
        _ => Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        )),
    }
}

pub(super) fn planned_required_u64(
    m: &std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
    cmd_name: &str,
) -> Result<u64, CliError> {
    let Some(Term::Int(i)) = m.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned invalid {key}"),
        ));
    };
    i.to_string().parse::<u64>().map_err(|_| {
        cli_err(
            EX_PARSE,
            "selfhost/plan",
            format!("{cmd_name}: planner returned out-of-range {key}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::parse_hex32_for_cli;

    #[test]
    fn frontend_hash_transport_requires_canonical_lowercase_hex() {
        assert!(parse_hex32_for_cli(&"0".repeat(64), "test").is_ok());
        assert!(parse_hex32_for_cli(&"A".repeat(64), "test").is_err());
        assert!(parse_hex32_for_cli(&format!(" {}", "0".repeat(64)), "test").is_err());
    }
}
