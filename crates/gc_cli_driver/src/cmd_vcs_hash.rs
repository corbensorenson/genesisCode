use super::*;

pub(super) fn handle_vcs_hash(cli: &Cli, cmd: &VcsCmd) -> Result<CmdOut, CliError> {
    let VcsCmd::Hash { input, engine } = cmd else {
        return Err(cli_err(
            EX_INTERNAL,
            "vcs/dispatch-drift",
            "cmd_vcs_hash called for non-hash command",
        ));
    };

    let engine = resolved_engine(cli, "vcs hash", *engine)?;
    let src = std::fs::read_to_string(input)
        .with_context(|| format!("read {}", input.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (hash_hex, hk) = if engine == FmtEngine::Selfhost {
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(resolved_mem_limits(cli));
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_runtime_selfhost_toolchain(cli, &mut ctx, &mut env)?;

        let f = env.get("core/cli::hash-src-with-kind").ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "selfhost/missing",
                "missing binding core/cli::hash-src-with-kind",
            )
        })?;

        ctx.steps = 0;
        ctx.step_limit = resolved_step_limit(cli).resolve();
        let r = f
            .apply(&mut ctx, Value::Data(Term::Str(src.clone())))
            .map_err(|e| {
                cli_err(
                    EX_EVAL,
                    "eval/error",
                    format!("selfhost vcs hash failed: {e}"),
                )
            })?;
        if let Some((code, message, payload)) = extract_protocol_error(&ctx, &r) {
            return Err(CliError {
                exit_code: EX_PARSE,
                json: JsonError {
                    code: "selfhost/error",
                    message: format!("{code}: {message}"),
                    context: payload.map(serde_json::Value::String),
                },
            });
        }
        let (hash_hex, hk) = match r {
            Value::Data(Term::Map(m)) => {
                let hash_hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                    Some(Term::Str(s)) => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :hash string",
                        ));
                    }
                };
                let hk = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                    Some(Term::Str(s)) if s == "term" || s == "module" => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :kind string",
                        ));
                    }
                };
                (hash_hex, hk)
            }
            Value::Map(m) => {
                let hash_hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                    Some(Value::Data(Term::Str(s))) => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :hash string",
                        ));
                    }
                };
                let hk = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                    Some(Value::Data(Term::Str(s))) if s == "term" || s == "module" => s.clone(),
                    _ => {
                        return Err(cli_err(
                            EX_INTERNAL,
                            "selfhost/bad-return",
                            "selfhost vcs hash return missing :kind string",
                        ));
                    }
                };
                (hash_hex, hk)
            }
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "selfhost/bad-return",
                    format!("selfhost vcs hash returned non-map: {}", r.debug_repr()),
                ));
            }
        };
        (hash_hex, hk)
    } else {
        let (h, hk) = match parse_term(&src) {
            Ok(t) => (gc_coreform::hash_term(&t), "term"),
            Err(_) => {
                let forms = parse_module(&src)
                    .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
                let forms = canonicalize_module(forms)
                    .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
                (hash_module(&forms), "module")
            }
        };
        (gc_vcs::bytes32_to_hex(&h), hk.to_string())
    };

    let env = JsonEnvelope {
        ok: true,
        kind: vcs_contract::kind(cmd),
        data: Some(serde_json::json!({
            "in": input.display().to_string(),
            // Keep legacy field for backward-compat while standardizing on `in`.
            "input": input.display().to_string(),
            "hash": hash_hex,
            "hash_kind": hk,
            "hash_format": "hex",
            "engine": if engine == FmtEngine::Selfhost { "selfhost" } else { "rust" },
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{hash_hex}\n")
        },
        json: json_envelope_value(env)?,
    })
}
