use super::*;

pub(super) fn cmd_refs(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &RefsCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let kind = refs_contract::kind(cmd);
    let log_op = refs_contract::log_op(cmd);
    let (prog, program_hash) = if frontend_is_rust(&frontend) {
        let forms = match cmd {
            RefsCmd::Get { name } => mk_refs_get_program(name),
            RefsCmd::List { prefix } => mk_refs_list_program(prefix.as_deref()),
            RefsCmd::Set {
                name,
                hash,
                policy,
                expected_old,
            } => mk_refs_set_program(name, hash, policy, expected_old.as_deref()),
            RefsCmd::Delete {
                name,
                policy,
                expected_old,
            } => mk_refs_delete_program(name, policy, expected_old.as_deref()),
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
            RefsCmd::Get { name } => {
                let f = env.get("core/cli::refs-get-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::refs-get-program",
                    )
                })?;
                let prog = f
                    .apply(&mut ctx, Value::data(Term::Str(name.to_string())))
                    .map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli refs-get-program failed: {e}"),
                        )
                    })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("refs/get".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(name.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, desc)
            }
            RefsCmd::List { prefix } => {
                let f = env.get("core/cli::refs-list-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::refs-list-program",
                    )
                })?;
                let prefix_term = prefix
                    .as_deref()
                    .map(|s| Term::Str(s.to_string()))
                    .unwrap_or(Term::Nil);
                let prog = f
                    .apply(&mut ctx, Value::data(prefix_term.clone()))
                    .map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli refs-list-program failed: {e}"),
                        )
                    })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("refs/list".to_string()),
                        ),
                        (TermOrdKey(Term::symbol(":prefix")), prefix_term),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, desc)
            }
            RefsCmd::Set {
                name,
                hash,
                policy,
                expected_old,
            } => {
                let f = env.get("core/cli::refs-set-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::refs-set-program",
                    )
                })?;

                let (present, expected_old_term) = match expected_old.as_deref() {
                    None => (false, Term::Nil),
                    Some("nil") => (true, Term::Nil),
                    Some(s) => (true, Term::Str(s.to_string())),
                };
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(name.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(hash.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(policy.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":expected-old-present")),
                            Term::Bool(present),
                        ),
                        (TermOrdKey(Term::symbol(":expected-old")), expected_old_term),
                    ]
                    .into_iter()
                    .collect(),
                );

                let prog = f.apply(&mut ctx, Value::data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli refs-set-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("refs/set".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(name.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(hash.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(policy.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":expected-old")),
                            expected_old
                                .as_deref()
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, desc)
            }
            RefsCmd::Delete {
                name,
                policy,
                expected_old,
            } => {
                let f = env.get("core/cli::refs-delete-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::refs-delete-program",
                    )
                })?;

                let (present, expected_old_term) = match expected_old.as_deref() {
                    None => (false, Term::Nil),
                    Some("nil") => (true, Term::Nil),
                    Some(s) => (true, Term::Str(s.to_string())),
                };
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(name.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(policy.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":expected-old-present")),
                            Term::Bool(present),
                        ),
                        (TermOrdKey(Term::symbol(":expected-old")), expected_old_term),
                    ]
                    .into_iter()
                    .collect(),
                );

                let prog = f.apply(&mut ctx, Value::data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli refs-delete-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("refs/delete".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(name.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(policy.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":expected-old")),
                            expected_old
                                .as_deref()
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
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
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "refs", &r.log)?;

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
        if let Some(Term::Map(m)) = payload.as_ref().as_data()
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
        match cmd {
            RefsCmd::Get { .. } => extract_refs_get_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            RefsCmd::List { .. } => extract_refs_list_pairs(&r.value)
                .map(|pairs| {
                    let mut s = String::new();
                    for (n, h) in pairs {
                        s.push_str(&n);
                        s.push(' ');
                        s.push_str(&h);
                        s.push('\n');
                    }
                    s
                })
                .unwrap_or_else(|| format!("{value}\n")),
            RefsCmd::Set { .. } => {
                if ok {
                    extract_refs_set_hash(&r.value)
                        .map(|h| format!("{h}\n"))
                        .unwrap_or_else(|| "ok\n".to_string())
                } else {
                    format!("{value}\n")
                }
            }
            RefsCmd::Delete { .. } => {
                if ok {
                    "ok\n".to_string()
                } else {
                    format!("{value}\n")
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
                code: "refs/error",
                message: "refs operation failed".to_string(),
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
