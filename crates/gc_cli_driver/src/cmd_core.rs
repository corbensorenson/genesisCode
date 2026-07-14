use super::*;

pub(super) fn cmd_eval(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "eval", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;

    let (mut ctx, mut env, mut forms) = match engine {
        #[cfg(feature = "parity-harness")]
        FmtEngine::Rust => {
            let forms = parse_module(&src).map_err(|e| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/coreform",
                    e.to_string(),
                    structured_failures::parser_context("eval/parse", file, &src, &e),
                )
            })?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;

            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms)
        }
        FmtEngine::Selfhost => {
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so closure/request hashing matches the rust frontend path.
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms)
        }
    };

    let stage1 = if stage1_pipeline || stage1_gate {
        let out = gc_opt::stage1_pipeline(&forms)
            .map_err(|e| cli_err(EX_INTERNAL, "stage1/error", format!("{e}")))?;
        if stage1_gate && !out.gate_report.ok {
            return Err(CliError {
                exit_code: EX_OBLIGATIONS,
                json: JsonError {
                    code: "obligation/stage1-validation",
                    message: "core/obligation::stage1-validation failed".to_string(),
                    context: Some(gc_opt::stage1_pipeline_json(&out)),
                },
            });
        }
        forms = out.transformed_forms.clone();
        Some(out)
    } else {
        None
    };

    let stage1_for_stage2 = if stage2_gate && stage1.is_none() {
        Some(
            gc_opt::stage1_pipeline(&forms)
                .map_err(|e| cli_err(EX_INTERNAL, "stage1/error", format!("{e}")))?,
        )
    } else {
        None
    };
    let stage2_input: &[Term] = if let Some(out) = stage1.as_ref() {
        &out.transformed_forms
    } else if let Some(out) = stage1_for_stage2.as_ref() {
        &out.transformed_forms
    } else {
        &forms
    };
    let stage2 = if stage2_gate {
        Some(gc_opt::stage2_validation_report(stage2_input))
    } else {
        None
    };
    if stage2_gate {
        let Some(s2) = stage2.as_ref() else {
            return Err(cli_err(
                EX_INTERNAL,
                "stage2/error",
                "stage2 gate enabled but no stage2 report was produced",
            ));
        };
        if !s2.supported || !s2.ok {
            return Err(CliError {
                exit_code: EX_OBLIGATIONS,
                json: JsonError {
                    code: "obligation/translation-validation",
                    message:
                        "core/obligation::translation-validation (stage2 CoreForm->WASM) failed"
                            .to_string(),
                    context: Some(gc_opt::stage2_report_json(s2)),
                },
            });
        }
    }

    let (v, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms).map_err(|e| {
        cli_err_with_context(
            EX_EVAL,
            "eval/error",
            format!("{e}"),
            structured_failures::evaluator_context("eval/module", &e),
        )
    })?;

    ensure_no_protocol_error(&ctx, &v, false, "evaluator", "eval/result", "eval/error")?;

    let (value, value_format) = render_value_for_cli(&ctx, &v);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/eval-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
            "kernel_eval_backend": eval_backend.as_str(),
            "stage1": stage1.as_ref().map(gc_opt::stage1_pipeline_json),
            "stage2": stage2.as_ref().map(gc_opt::stage2_report_json),
            "value": value,
            "value_format": value_format,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{value}\n")
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_explain(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    contract_src: &str,
    msg_src: &str,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "explain", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (mut ctx, mut env, forms, contract_term, msg_term) = match engine {
        #[cfg(feature = "parity-harness")]
        FmtEngine::Rust => {
            let forms = parse_module(&src).map_err(|e| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/coreform",
                    e.to_string(),
                    structured_failures::parser_context("explain/parse", file, &src, &e),
                )
            })?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let contract_term = parse_term(contract_src).map_err(|e| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/term",
                    format!("--contract: {e}"),
                    structured_failures::parser_context(
                        "explain/parse-contract",
                        Path::new("<contract>"),
                        contract_src,
                        &e,
                    ),
                )
            })?;
            let msg_term = parse_term(msg_src).map_err(|e| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/term",
                    format!("--msg: {e}"),
                    structured_failures::parser_context(
                        "explain/parse-message",
                        Path::new("<message>"),
                        msg_src,
                        &e,
                    ),
                )
            })?;
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms, contract_term, msg_term)
        }
        FmtEngine::Selfhost => {
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so contract closure hashing matches the rust frontend path.
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;
            let contract_term =
                selfhost_parse_term(&mut parse_ctx, &parse_env, contract_src, "--contract")?;
            let msg_term = selfhost_parse_term(&mut parse_ctx, &parse_env, msg_src, "--msg")?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms, contract_term, msg_term)
        }
    };

    let (_, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms).map_err(|e| {
        cli_err_with_context(
            EX_EVAL,
            "eval/error",
            format!("{e}"),
            structured_failures::evaluator_context("explain/module", &e),
        )
    })?;

    let contract = eval_term(&mut ctx, &env, &contract_term).map_err(|e| {
        cli_err_with_context(
            EX_EVAL,
            "eval/error",
            format!("--contract: {e}"),
            structured_failures::evaluator_context("explain/contract", &e),
        )
    })?;

    let msg_val = Value::data(msg_term);

    let explain = env.get("core/contract::explain").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "prelude/missing",
            "missing prelude binding core/contract::explain",
        )
    })?;
    let r = explain
        .apply(&mut ctx, contract)
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("apply contract: {e}"),
                structured_failures::evaluator_context("explain/apply-contract", &e),
            )
        })?
        .apply(&mut ctx, msg_val)
        .map_err(|e| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("explain failed: {e}"),
                structured_failures::evaluator_context("explain/dispatch", &e),
            )
        })?;

    let (value, value_format) = render_value_for_cli(&ctx, &r);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/explain-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
            "kernel_eval_backend": eval_backend.as_str(),
            "contract": contract_src,
            "msg": msg_src,
            "trace": value,
            "trace_format": value_format,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{value}\n")
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_run(
    cli: &Cli,
    flavor: Flavor,
    file: &Path,
    engine: Option<FmtEngine>,
    caps: &Path,
    log: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "run", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (mut ctx, mut env, forms) = match engine {
        #[cfg(feature = "parity-harness")]
        FmtEngine::Rust => {
            let forms = parse_module(&src).map_err(|e| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/coreform",
                    e.to_string(),
                    structured_failures::parser_context("run/parse", file, &src, &e),
                )
            })?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms)
        }
        FmtEngine::Selfhost => {
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so closure/request hashing matches the rust frontend path.
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms)
        }
    };
    let program_hash = hash_module(&forms);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;

    let (prog, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms).map_err(|e| {
        cli_err_with_context(
            EX_EVAL,
            "eval/error",
            format!("{e}"),
            structured_failures::evaluator_context("run/evaluate", &e),
        )
    })?;

    let toolchain = match flavor {
        Flavor::Native => format!("genesis/{} (native)", env!("CARGO_PKG_VERSION")),
        Flavor::Wasi => format!("genesis_wasi/{} (wasi)", env!("CARGO_PKG_VERSION")),
    };
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain).map_err(|e| {
        cli_err_with_context(
            EX_EVAL,
            "effects/run",
            format!("{e}"),
            structured_failures::effects_context("run/effects", &e),
        )
    })?;
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "run", &r.log)?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| file.with_extension("gclog"));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let denied_op = r
        .log
        .entries
        .iter()
        .find(|entry| entry.decision == Decision::Deny)
        .map(|entry| entry.op.clone());
    let denied = denied_op.is_some();
    ensure_no_runner_protocol_error(&ctx, &r.value, denied)?;
    let exit_code = if denied { EX_CAPS_DENIED } else { EX_OK };
    let error = denied_op.as_ref().map(|op| JsonError {
        code: "caps/denied",
        message: format!("capability denied for effect operation {op}"),
        context: Some(
            structured_failures::FailureContext::new("policy", "capability-denied", "run/effects")
                .fact("effect_op", op.clone())
                .fact("blocking_capability", op.clone())
                .into_value(),
        ),
    });

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    let env = JsonEnvelope {
        ok: !denied,
        kind: "genesis/run-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
            "kernel_eval_backend": eval_backend.as_str(),
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "program_hash_hex": hex32(program_hash),
            "denied": denied,
            "entries": r.log.entries.len(),
            "value": value,
            "value_format": value_format,
        })),
        error,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{value}\n")
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn default_log_path(op: &str) -> PathBuf {
    let dir = PathBuf::from(".genesis").join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    dir.join(format!("{op}-{stamp}.gclog"))
}

pub(super) fn cmd_replay(
    cli: &Cli,
    file: &PathBuf,
    engine: Option<FmtEngine>,
    log_path: &PathBuf,
    store_dir: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "replay", engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let (mut ctx, mut env, forms) = match engine {
        #[cfg(feature = "parity-harness")]
        FmtEngine::Rust => {
            let forms = parse_module(&src).map_err(|e| {
                cli_err_with_context(
                    EX_PARSE,
                    "parse/coreform",
                    e.to_string(),
                    structured_failures::parser_context("replay/parse-program", file, &src, &e),
                )
            })?;
            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let mut ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut ctx);
            (ctx, prelude.env, forms)
        }
        FmtEngine::Selfhost => {
            // Parse/canonicalize with selfhost bindings loaded, then evaluate in a fresh
            // prelude-only env so closure/request hashing matches the rust frontend path.
            let mut parse_ctx = EvalCtx::with_step_limit(None);
            parse_ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut parse_ctx);
            let mut parse_env = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut parse_ctx, &mut parse_env)?;

            parse_ctx.steps = 0;
            parse_ctx.step_limit = None;
            let forms = selfhost_parse_canonicalize_module(&mut parse_ctx, &parse_env, &src)?;

            let mut eval_ctx = mk_ctx(cli);
            let prelude = build_prelude(&mut eval_ctx);
            (eval_ctx, prelude.env, forms)
        }
    };
    let program_hash = hash_module(&forms);

    let log_src = std::fs::read_to_string(log_path)
        .with_context(|| format!("read {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let log_term = parse_term(&log_src).map_err(|e| {
        cli_err_with_context(
            EX_PARSE,
            "parse/log",
            e.to_string(),
            structured_failures::parser_context("replay/parse-log", log_path, &log_src, &e),
        )
    })?;
    let log = EffectLog::from_term(&log_term).map_err(|e| {
        cli_err_with_context(
            EX_PARSE,
            "parse/log",
            format!("{e}"),
            structured_failures::effects_context("replay/decode-log", &e),
        )
    })?;
    if log.program_hash != program_hash {
        return Err(cli_err_with_context(
            EX_REPLAY_MISMATCH,
            "replay/program-hash-mismatch",
            "program hash mismatch: log is for different program",
            structured_failures::FailureContext::new(
                "replay",
                "program-hash-mismatch",
                "replay/verify-program",
            )
            .fact("expected_program_hash", hex32(program_hash))
            .fact("logged_program_hash", hex32(log.program_hash))
            .into_value(),
        ));
    }

    let (prog, eval_backend) = eval_module_default(&mut ctx, &mut env, &forms).map_err(|e| {
        cli_err_with_context(
            EX_EVAL,
            "eval/error",
            format!("{e}"),
            structured_failures::evaluator_context("replay/evaluate", &e),
        )
    })?;
    let store = match store_dir {
        Some(p) => Some(
            gc_effects::ArtifactStore::open(p)
                .map_err(|e| cli_err(EX_IO, "io/store", format!("{e}")))?,
        ),
        None => None,
    };
    let v = gc_effects::replay_with_store(&mut ctx, prog, &log, store.as_ref()).map_err(|e| {
        let code = match e {
            gc_effects::EffectsError::ReplayMismatch(_) => "replay/mismatch",
            _ => "replay/error",
        };
        let context = structured_failures::effects_context("replay/execute", &e);
        cli_err_with_context(EX_REPLAY_MISMATCH, code, format!("{e}"), context)
    })?;
    let (value, value_format) = render_value_for_cli(&ctx, &v);
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/replay-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "engine": engine.as_str(),
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
            "kernel_eval_backend": eval_backend.as_str(),
            "log": log_path.display().to_string(),
            "store": store_dir.map(|p| p.display().to_string()),
            "value": value,
            "value_format": value_format,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{value}\n")
        },
        json: json_envelope_value(env)?,
    })
}
