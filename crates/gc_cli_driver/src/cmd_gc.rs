use super::*;

pub(super) fn cmd_gc(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &GcCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let (prog, kind, log_op, program_hash) = match frontend {
        gc_obligations::CoreformFrontend::Rust => {
            let (forms, kind, log_op) = match cmd {
                GcCmd::Plan {
                    lock,
                    pins,
                    depth,
                    no_lock,
                    no_refs,
                } => (
                    mk_gc_plan_program(lock, pins, *depth, !*no_lock, !*no_refs),
                    "genesis/gc-plan-v0.1",
                    "gc-plan",
                ),
                GcCmd::Run {
                    lock,
                    pins,
                    depth,
                    no_lock,
                    no_refs,
                    quarantine,
                    quarantine_dir,
                } => (
                    mk_gc_run_program(
                        lock,
                        pins,
                        *depth,
                        !*no_lock,
                        !*no_refs,
                        *quarantine,
                        quarantine_dir.as_deref(),
                    ),
                    "genesis/gc-run-v0.1",
                    "gc-run",
                ),
                GcCmd::Pin { target, pins } => (
                    mk_gc_pin_program(target, pins),
                    "genesis/gc-pin-v0.1",
                    "gc-pin",
                ),
                GcCmd::Unpin { target, pins } => (
                    mk_gc_unpin_program(target, pins),
                    "genesis/gc-unpin-v0.1",
                    "gc-unpin",
                ),
                GcCmd::Purge {
                    ttl_days,
                    quarantine_dir,
                } => (
                    mk_gc_purge_program(*ttl_days, quarantine_dir.as_deref()),
                    "genesis/gc-purge-v0.1",
                    "gc-purge",
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
                GcCmd::Plan {
                    lock,
                    pins,
                    depth,
                    no_lock,
                    no_refs,
                } => {
                    let f = env.get("core/cli::gc-plan-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-plan-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-plan-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/plan".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-plan-v0.1", "gc-plan", desc)
                }
                GcCmd::Run {
                    lock,
                    pins,
                    depth,
                    no_lock,
                    no_refs,
                    quarantine,
                    quarantine_dir,
                } => {
                    let f = env.get("core/cli::gc-run-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-run-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine")),
                                Term::Bool(*quarantine),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine-dir")),
                                quarantine_dir
                                    .as_deref()
                                    .map(|p| Term::Str(p.display().to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-run-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/run".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":lock")),
                                Term::Str(lock.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":depth")),
                                Term::Int((*depth as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-lock")),
                                Term::Bool(!*no_lock),
                            ),
                            (
                                TermOrdKey(Term::symbol(":include-refs")),
                                Term::Bool(!*no_refs),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine")),
                                Term::Bool(*quarantine),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine-dir")),
                                quarantine_dir
                                    .as_deref()
                                    .map(|p| Term::Str(p.display().to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-run-v0.1", "gc-run", desc)
                }
                GcCmd::Pin { target, pins } => {
                    let f = env.get("core/cli::gc-pin-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-pin-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-pin-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/pin".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-pin-v0.1", "gc-pin", desc)
                }
                GcCmd::Unpin { target, pins } => {
                    let f = env.get("core/cli::gc-unpin-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-unpin-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":pins")),
                                Term::Str(pins.display().to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-unpin-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/unpin".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":target")),
                                Term::Str(target.to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-unpin-v0.1", "gc-unpin", desc)
                }
                GcCmd::Purge {
                    ttl_days,
                    quarantine_dir,
                } => {
                    let f = env.get("core/cli::gc-purge-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::gc-purge-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":ttl-days")),
                                Term::Int((*ttl_days as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":quarantine-dir")),
                                quarantine_dir
                                    .as_deref()
                                    .map(|p| Term::Str(p.display().to_string()))
                                    .unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli gc-purge-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("gc/purge".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":ttl-days")),
                                Term::Int((*ttl_days as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/gc-purge-v0.1", "gc-purge", desc)
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            (prog, kind, log_op, program_hash)
        }
    };

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

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

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    let stdout = if cli.json {
        String::new()
    } else {
        format!("{value}\n")
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
                code: "gc/error",
                message: "gc operation failed".to_string(),
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
