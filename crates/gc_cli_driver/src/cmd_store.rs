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
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

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
            };
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let program_hash = hash_module(&forms);
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            (prog, kind, log_op, program_hash)
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
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
            };
            let program_hash = gc_coreform::hash_term(&desc);
            (prog, kind, log_op, program_hash)
        }
    };

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "pkg", &r.log)?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
        }
    }

    // Extract a stable stdout payload.
    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
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
                code: "store/error",
                message: "store operation failed".to_string(),
                context: None,
            })
        },
    };

    Ok(CmdOut {
        exit_code,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}
