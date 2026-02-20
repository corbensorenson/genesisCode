use super::*;

pub(super) fn cmd_store(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &StoreCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let (prog, kind, log_op, program_hash) = match &frontend {
        gc_obligations::CoreformFrontend::Rust => {
            let (forms, kind, log_op) = match cmd {
                StoreCmd::Put { input } => {
                    let src = std::fs::read_to_string(input)
                        .with_context(|| format!("read {}", input.display()))
                        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
                    let art = parse_term(&src)
                        .map_err(|e| cli_err(EX_PARSE, "parse/term", e.to_string()))?;
                    (
                        mk_store_put_program(&art),
                        "genesis/store-put-v0.2",
                        "store-put",
                    )
                }
                StoreCmd::Get { hash, .. } => (
                    mk_store_get_program(hash),
                    "genesis/store-get-v0.2",
                    "store-get",
                ),
                StoreCmd::Has { hash } => (
                    mk_store_has_program(hash),
                    "genesis/store-has-v0.2",
                    "store-has",
                ),
                StoreCmd::Verify { hash } => (
                    mk_store_verify_program(hash.as_deref()),
                    "genesis/store-verify-v0.2",
                    "store-verify",
                ),
            };
            eval_store_program_forms(&mut ctx, &mut env, forms, kind, log_op)?
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            if let StoreCmd::Verify { hash } = cmd {
                eval_store_program_forms(
                    &mut ctx,
                    &mut env,
                    mk_store_verify_program(hash.as_deref()),
                    "genesis/store-verify-v0.2",
                    "store-verify",
                )
            } else {
                load_selfhost_toolchain(cli, &mut ctx, &mut env)?;
                let (prog, kind, log_op, desc) = match cmd {
                    StoreCmd::Put { input } => {
                        let src = std::fs::read_to_string(input)
                            .with_context(|| format!("read {}", input.display()))
                            .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
                        let art = selfhost_parse_term(&mut ctx, &env, &src, "store put input")?;
                        let f = env.get("core/cli::store-put-program").ok_or_else(|| {
                            cli_err(
                                EX_INTERNAL,
                                "selfhost/missing",
                                "missing binding core/cli::store-put-program",
                            )
                        })?;
                        let prog = f.apply(&mut ctx, Value::Data(art.clone())).map_err(|e| {
                            cli_err(
                                EX_EVAL,
                                "eval/error",
                                format!("core/cli store-put-program failed: {e}"),
                            )
                        })?;
                        let desc = Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":cmd")),
                                    Term::Str("store/put".to_string()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":artifact-h")),
                                    Term::Bytes(gc_coreform::hash_term(&art).to_vec().into()),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        );
                        (prog, "genesis/store-put-v0.2", "store-put", desc)
                    }
                    StoreCmd::Get { hash, .. } => {
                        let f = env.get("core/cli::store-get-program").ok_or_else(|| {
                            cli_err(
                                EX_INTERNAL,
                                "selfhost/missing",
                                "missing binding core/cli::store-get-program",
                            )
                        })?;
                        let prog = f
                            .apply(&mut ctx, Value::Data(Term::Str(hash.to_string())))
                            .map_err(|e| {
                                cli_err(
                                    EX_EVAL,
                                    "eval/error",
                                    format!("core/cli store-get-program failed: {e}"),
                                )
                            })?;
                        let desc = Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":cmd")),
                                    Term::Str("store/get".to_string()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":hash")),
                                    Term::Str(hash.to_string()),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        );
                        (prog, "genesis/store-get-v0.2", "store-get", desc)
                    }
                    StoreCmd::Has { hash } => {
                        let f = env.get("core/cli::store-has-program").ok_or_else(|| {
                            cli_err(
                                EX_INTERNAL,
                                "selfhost/missing",
                                "missing binding core/cli::store-has-program",
                            )
                        })?;
                        let prog = f
                            .apply(&mut ctx, Value::Data(Term::Str(hash.to_string())))
                            .map_err(|e| {
                                cli_err(
                                    EX_EVAL,
                                    "eval/error",
                                    format!("core/cli store-has-program failed: {e}"),
                                )
                            })?;
                        let desc = Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":cmd")),
                                    Term::Str("store/has".to_string()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":hash")),
                                    Term::Str(hash.to_string()),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        );
                        (prog, "genesis/store-has-v0.2", "store-has", desc)
                    }
                    StoreCmd::Verify { .. } => unreachable!("handled above"),
                };
                let program_hash = gc_coreform::hash_term(&desc);
                Ok((prog, kind, log_op, program_hash))
            }?
        }
    };

    finish_store_command(
        cli,
        caps,
        log,
        cmd,
        frontend_info,
        &mut ctx,
        &policy,
        prog,
        kind,
        log_op,
        program_hash,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "store command finishing keeps explicit runtime/log/front-end context visible"
)]
fn finish_store_command(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &StoreCmd,
    frontend_info: serde_json::Value,
    ctx: &mut EvalCtx,
    policy: &CapsPolicy,
    prog: Value,
    kind: &'static str,
    log_op: &'static str,
    program_hash: [u8; 32],
) -> Result<CmdOut, CliError> {
    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(ctx, policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "store", &r.log)?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    let mut error_code = "store/error";
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && let Some(Term::Str(s)) = m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code")))
        {
            error_code = store_error_json_code(s);
            if s == "core/caps/denied" {
                exit_code = EX_CAPS_DENIED;
            } else if matches!(cmd, StoreCmd::Verify { .. })
                && matches!(
                    s.as_str(),
                    "core/store/corruption"
                        | "core/store/not-found"
                        | "core/store/bad-hash"
                        | "core/store/io-error"
                )
            {
                exit_code = EX_VERIFY;
            }
        }
    }

    // Extract a stable stdout payload.
    let (value, value_format) = render_value_for_cli(ctx, &r.value);
    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            StoreCmd::Put { .. } => extract_store_put_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            StoreCmd::Has { .. } => extract_store_has_present(&r.value)
                .map(|b| format!("{}\n", if b { "true" } else { "false" }))
                .unwrap_or_else(|| format!("{value}\n")),
            StoreCmd::Get { out, .. } => {
                if !ok {
                    format!("{value}\n")
                } else if let Some(p) = out {
                    let art = extract_store_get_artifact(&r.value).ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "store/error",
                            "store get returned unexpected value",
                        )
                    })?;
                    std::fs::write(p, gc_coreform::print_term(&art) + "\n")
                        .with_context(|| format!("write {}", p.display()))
                        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
                    String::new()
                } else {
                    extract_store_get_artifact(&r.value)
                        .map(|t| format!("{}\n", gc_coreform::print_term(&t)))
                        .unwrap_or_else(|| format!("{value}\n"))
                }
            }
            StoreCmd::Verify { .. } => {
                if !ok {
                    format!("{value}\n")
                } else {
                    extract_store_verify_checked(&r.value)
                        .map(|n| format!("ok {n}\n"))
                        .unwrap_or_else(|| "ok\n".to_string())
                }
            }
        }
    };

    let env = JsonEnvelope {
        ok,
        kind,
        data: Some(serde_json::json!({
            "coreform_frontend": frontend_info,
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "value": value,
            "value_format": value_format,
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: error_code,
                message: "store operation failed".to_string(),
                context: None,
            })
        },
    };

    Ok(CmdOut {
        exit_code,
        stdout,
        json: json_envelope_value(env)?,
    })
}

fn eval_store_program_forms(
    ctx: &mut EvalCtx,
    env: &mut gc_kernel::Env,
    forms: Vec<Term>,
    kind: &'static str,
    log_op: &'static str,
) -> Result<(Value, &'static str, &'static str, [u8; 32]), CliError> {
    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let program_hash = hash_module(&forms);
    let prog = eval_module(ctx, env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
    Ok((prog, kind, log_op, program_hash))
}

fn store_error_json_code(code: &str) -> &'static str {
    match code {
        "core/caps/denied" => "core/caps/denied",
        "core/store/corruption" => "core/store/corruption",
        "core/store/not-found" => "core/store/not-found",
        "core/store/bad-hash" => "core/store/bad-hash",
        "core/store/io-error" => "core/store/io-error",
        "core/store/bad-payload" => "core/store/bad-payload",
        "core/store/remote-auth" => "core/store/remote-auth",
        "core/store/remote-error" => "core/store/remote-error",
        "core/store/hash-mismatch" => "core/store/hash-mismatch",
        "core/store/bad-artifact" => "core/store/bad-artifact",
        "core/caps/policy-error" => "core/caps/policy-error",
        _ => "store/error",
    }
}
