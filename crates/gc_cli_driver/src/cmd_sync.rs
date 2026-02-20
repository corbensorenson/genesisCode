use super::*;

pub(super) fn cmd_sync(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &SyncCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let kind = sync_contract::kind(cmd);
    let log_op = sync_contract::log_op(cmd);
    let (prog, program_hash) = if frontend_is_rust(&frontend) {
        let forms = match cmd {
            SyncCmd::Pull {
                remote,
                refs,
                roots,
                depth,
                force,
            } => mk_sync_pull_program(remote, refs, roots, *depth, *force),
            SyncCmd::Push {
                remote,
                roots,
                depth,
                set_refs,
            } => {
                let parsed = parse_sync_set_refs(set_refs)?;
                mk_sync_push_program(remote, roots, *depth, &parsed)
            }
        };

        let forms = canonicalize_module(forms)
            .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
        let program_hash = hash_module(&forms);
        let prog = eval_module(&mut ctx, &mut env, &forms)
            .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
        (prog, program_hash)
    } else {
        load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

        let (prog, desc) = match cmd {
            SyncCmd::Pull {
                remote,
                refs,
                roots,
                depth,
                force,
            } => {
                let f = env.get("core/cli::sync-pull-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::sync-pull-program",
                    )
                })?;
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":remote")),
                            Term::Str(remote.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs")),
                            Term::Vector(refs.iter().cloned().map(Term::Str).collect()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":roots")),
                            Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (TermOrdKey(Term::symbol(":force")), Term::Bool(*force)),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli sync-pull-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("sync/pull".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":remote")),
                            Term::Str(remote.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs")),
                            Term::Vector(refs.iter().cloned().map(Term::Str).collect()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":roots")),
                            Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (TermOrdKey(Term::symbol(":force")), Term::Bool(*force)),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, desc)
            }
            SyncCmd::Push {
                remote,
                roots,
                depth,
                set_refs,
            } => {
                let f = env.get("core/cli::sync-push-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::sync-push-program",
                    )
                })?;
                let parsed = parse_sync_set_refs(set_refs)?;

                let mut set_refs_term: Vec<Term> = Vec::new();
                for sr in &parsed {
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":name")),
                        Term::Str(sr.name.clone()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":hash")),
                        Term::Str(sr.hash.clone()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":policy")),
                        Term::Str(sr.policy.clone()),
                    );
                    if let Some(e) = &sr.expected_old {
                        let v = if e == "nil" {
                            Term::Nil
                        } else {
                            Term::Str(e.clone())
                        };
                        mm.insert(TermOrdKey(Term::symbol(":expected-old")), v);
                    }
                    set_refs_term.push(Term::Map(mm));
                }

                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":remote")),
                            Term::Str(remote.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":roots")),
                            Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":set-refs")),
                            Term::Vector(set_refs_term),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli sync-push-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("sync/push".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":remote")),
                            Term::Str(remote.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":roots")),
                            Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":set-refs-len")),
                            Term::Int((parsed.len() as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, desc)
            }
        };
        let program_hash = gc_coreform::hash_term(&desc);
        (prog, program_hash)
    };

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "sync", &r.log)?;

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
                code: "sync/error",
                message: "sync operation failed".to_string(),
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
