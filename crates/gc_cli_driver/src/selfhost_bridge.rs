use super::*;

pub(super) fn extract_protocol_error(
    ctx: &EvalCtx,
    v: &Value,
) -> Option<(String, String, Option<String>)> {
    let tok = ctx.protocol?.error;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != tok {
        return None;
    }

    let payload_term = payload.to_term_for_log(Some(tok));
    let (code, msg) = match &payload_term {
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
            let msg = m
                .get(&gc_coreform::TermOrdKey(Term::Symbol(
                    ":error/message".to_string(),
                )))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "error".to_string());
            (code, msg)
        }
        _ => ("core/error".to_string(), "error".to_string()),
    };
    Some((code, msg, Some(gc_coreform::print_term(&payload_term))))
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

pub(super) fn selfhost_parse_canonicalize_module(
    ctx: &mut EvalCtx,
    env: &gc_kernel::Env,
    src: &str,
) -> Result<Vec<Term>, CliError> {
    if let Some(canon_src_fn) = env.get("core/cli::canonicalize-module-src") {
        let canon = canon_src_fn
            .apply(ctx, Value::data(Term::Str(src.to_string())))
            .map_err(|e| {
                cli_err_with_context(
                    EX_EVAL,
                    "eval/error",
                    format!("core/cli canonicalize-module-src failed: {e}"),
                    structured_failures::evaluator_context("parser/canonicalize-module", &e),
                )
            })?;

        if let Some((code, message, payload)) = extract_protocol_error(ctx, &canon) {
            return Err(CliError {
                exit_code: EX_PARSE,
                json: JsonError {
                    code: "selfhost/error",
                    message: format!("{code}: {message}"),
                    context: Some(structured_failures::protocol_context(
                        "parser",
                        "parser/canonicalize-module",
                        &code,
                        payload.as_deref(),
                    )),
                },
            });
        }

        let Some(Term::Vector(forms)) = canon.as_data() else {
            return Err(cli_err(
                EX_INTERNAL,
                "selfhost/bad-return",
                format!(
                    "core/cli canonicalize-module-src returned non-vector: {}",
                    canon.debug_repr()
                ),
            ));
        };
        return Ok(forms.clone());
    }

    let parse_fn = env.get("selfhost/parse::parse-module").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding selfhost/parse::parse-module",
        )
    })?;
    let parsed = parse_fn
        .apply(ctx, Value::data(Term::Str(src.to_string())))
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("selfhost parse-module failed: {e}"),
                structured_failures::evaluator_context("parser/parse-module", &e),
            )
        })?;

    if let Some((code, message, payload)) = extract_protocol_error(ctx, &parsed) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: Some(structured_failures::protocol_context(
                    "parser",
                    "parser/parse-module",
                    &code,
                    payload.as_deref(),
                )),
            },
        });
    }

    let Some(Term::Vector(parsed_forms)) = parsed.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "selfhost parse-module returned non-vector: {}",
                parsed.debug_repr()
            ),
        ));
    };

    let canon_fn = env
        .get("selfhost/canon::canonicalize-module")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "selfhost/missing",
                "missing binding selfhost/canon::canonicalize-module",
            )
        })?;
    let canon = canon_fn
        .apply(ctx, Value::data(Term::Vector(parsed_forms.clone())))
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("selfhost canonicalize-module failed: {e}"),
                structured_failures::evaluator_context("parser/canonicalize-module", &e),
            )
        })?;

    if let Some((code, message, payload)) = extract_protocol_error(ctx, &canon) {
        return Err(CliError {
            exit_code: EX_PARSE,
            json: JsonError {
                code: "selfhost/error",
                message: format!("{code}: {message}"),
                context: Some(structured_failures::protocol_context(
                    "parser",
                    "parser/canonicalize-module",
                    &code,
                    payload.as_deref(),
                )),
            },
        });
    }

    let Some(Term::Vector(forms)) = canon.as_data() else {
        return Err(cli_err(
            EX_INTERNAL,
            "selfhost/bad-return",
            format!(
                "selfhost canonicalize-module returned non-vector: {}",
                canon.debug_repr()
            ),
        ));
    };
    Ok(forms.clone())
}

fn parse_hex32_for_cli(hex: &str, context: &str) -> Result<[u8; 32], CliError> {
    let s = hex.trim();
    if s.len() != 64 {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/hash",
            format!("{context} returned non-64-byte hex hash"),
        ));
    }
    let mut out = [0u8; 32];
    for (i, pair) in s.as_bytes().chunks_exact(2).enumerate() {
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
    if let Some(hash_forms_fn) = env.get("core/cli::hash-module-forms") {
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
        return parse_hex32_for_cli(hex, "core/cli hash-module-forms");
    }

    let hash_fn = env.get("selfhost/hash::hash-module").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding selfhost/hash::hash-module",
        )
    })?;
    let out = hash_fn
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
                "selfhost hash-module returned non-string: {}",
                out.debug_repr()
            ),
        ));
    };
    parse_hex32_for_cli(hex, "selfhost hash-module")
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
