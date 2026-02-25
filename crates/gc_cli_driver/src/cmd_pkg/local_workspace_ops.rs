use super::*;

pub(super) fn cmd_pkg_local_workspace_ops(
    cli: &Cli,
    flavor: Flavor,
    cmd: &PkgCmd,
    caps: &Path,
    log: Option<&Path>,
    frontend: &gc_obligations::CoreformFrontend,
    frontend_info: serde_json::Value,
) -> Result<Option<CmdOut>, CliError> {
    let mut env_hydrate_log: Option<EffectLog> = None;

    match cmd {
        PkgCmd::Run {
            task,
            workspace_file,
        } => {
            pkg_workspace_ops::validate_workspace_runtime_backend_for_run(workspace_file)
                .map_err(|e| cli_err(EX_PARSE, "pkg/run", e))?;
            let action = pkg_task_runner::resolve_workspace_task(workspace_file, task)
                .map_err(|e| cli_err(EX_PARSE, "pkg/run", e))?;
            let out = match action {
                pkg_task_runner::WorkspaceTaskAction::Test { pkg, caps: tcaps } => {
                    cmd_test(cli, &pkg, tcaps.as_deref().or(Some(caps)))?
                }
                pkg_task_runner::WorkspaceTaskAction::Pack { pkg } => cmd_pack(cli, &pkg)?,
                pkg_task_runner::WorkspaceTaskAction::Typecheck { pkg } => {
                    cmd_typecheck(cli, &pkg, false)?
                }
                pkg_task_runner::WorkspaceTaskAction::Run {
                    file,
                    caps: rcaps,
                    log: rlog,
                    engine,
                } => {
                    let parsed_engine = parse_task_engine(engine)?;
                    cmd_run(
                        cli,
                        flavor,
                        &file,
                        parsed_engine,
                        rcaps.as_deref().unwrap_or(caps),
                        rlog.as_deref(),
                    )?
                }
                pkg_task_runner::WorkspaceTaskAction::Contract {
                    file,
                    caps: rcaps,
                    log: rlog,
                    engine,
                    contract_hash_hex,
                } => {
                    pkg_task_runner::verify_contract_task_file_hash(&file, &contract_hash_hex)
                        .map_err(|e| cli_err(EX_VERIFY, "pkg/run-contract", e))?;
                    let parsed_engine = parse_task_engine(engine)?;
                    cmd_run(
                        cli,
                        flavor,
                        &file,
                        parsed_engine,
                        rcaps.as_deref().unwrap_or(caps),
                        rlog.as_deref(),
                    )?
                }
                pkg_task_runner::WorkspaceTaskAction::Eval {
                    file,
                    engine,
                    stage1_pipeline,
                    stage1_gate,
                    stage2_gate,
                } => {
                    let parsed_engine = parse_task_engine(engine)?;
                    cmd_eval(
                        cli,
                        &file,
                        parsed_engine,
                        stage1_pipeline,
                        stage1_gate,
                        stage2_gate,
                    )?
                }
                pkg_task_runner::WorkspaceTaskAction::Fmt {
                    file,
                    check,
                    engine,
                } => {
                    let parsed_engine = parse_task_engine(engine)?;
                    cmd_fmt(cli, &file, check, parsed_engine)?
                }
                pkg_task_runner::WorkspaceTaskAction::Optimize {
                    file,
                    out,
                    emit_wasm,
                    engine,
                    stage1_gate,
                    stage2_gate,
                } => {
                    let parsed_engine = parse_task_engine(engine)?;
                    cmd_optimize(
                        cli,
                        &file,
                        out.as_ref(),
                        emit_wasm.as_ref(),
                        parsed_engine,
                        stage1_gate,
                        stage2_gate,
                    )?
                }
            };
            return Ok(Some(out));
        }
        PkgCmd::Test { pkg, caps: pcaps } => {
            let out = cmd_test(cli, pkg, pcaps.as_deref().or(Some(caps)))?;
            return Ok(Some(out));
        }
        PkgCmd::SelfOptimize {
            pkg,
            caps: pcaps,
            dry_run,
        } => {
            let local = pkg_self_opt::handle_self_optimize(
                pkg,
                pcaps.as_deref(),
                frontend,
                resolved_step_limit(cli),
                resolved_mem_limits(cli),
                *dry_run,
            )
            .map_err(|e| cli_err(EX_OBLIGATIONS, "pkg/self-optimize", e))?;

            let log_path = log
                .map(PathBuf::from)
                .unwrap_or_else(|| default_log_path(local.log_op));
            let log_obj = pkg_workspace_ops::empty_log(local.program_hash);
            std::fs::write(&log_path, log_obj.to_string_canonical() + "\n")
                .with_context(|| format!("write {}", log_path.display()))
                .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

            let value_v = Value::Data(local.value.clone());
            let ok = extract_pkg_ok_bool(&value_v).unwrap_or(true);
            let exit_code = if ok { EX_OK } else { EX_OBLIGATIONS };
            let value = gc_coreform::print_term(&local.value);
            let mut data = serde_json::json!({
                "coreform_frontend": frontend_info,
                "caps": caps.display().to_string(),
                "log": log_path.display().to_string(),
                "value": value,
                "value_format": "coreform",
            });
            if let Some(report) = pkg_reports::build_pkg_ai_report(cmd, &value_v, caps)
                && let Some(obj) = data.as_object_mut()
            {
                obj.insert("report".to_string(), report);
            }
            if let Some(obj) = data.as_object_mut() {
                obj.insert(
                    "telemetry".to_string(),
                    pkg_telemetry::build_pkg_telemetry(
                        cmd,
                        ok,
                        exit_code,
                        &log_obj,
                        &value_v,
                        obj.get("report"),
                        None,
                    ),
                );
            }

            let stdout = if cli.json {
                String::new()
            } else {
                format!("{value}\n")
            };
            let env = JsonEnvelope {
                ok,
                kind: local.kind,
                data: Some(data),
                error: if ok {
                    None
                } else {
                    Some(JsonError {
                        code: "pkg/self-optimize",
                        message: "self-optimization promotion failed".to_string(),
                        context: None,
                    })
                },
            };
            return Ok(Some(CmdOut {
                exit_code,
                stdout,
                json: json_envelope_value(env)?,
            }));
        }
        _ => {}
    }

    let local = match cmd {
        PkgCmd::New {
            workspace,
            lock,
            workspace_file,
            policy,
            registry_default,
            members,
        } => Some(
            pkg_workspace_ops::handle_new(
                workspace,
                lock,
                workspace_file,
                policy,
                registry_default.as_deref(),
                members,
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/new", e))?,
        ),
        PkgCmd::Scaffold {
            archetype,
            name,
            root,
            force,
            runtime_backend,
            policy,
            registry_default,
        } => Some(
            pkg_scaffold::handle_scaffold(pkg_scaffold::PkgScaffoldArgs {
                archetype,
                name,
                root,
                force: *force,
                runtime_backend: runtime_backend.as_deref(),
                policy,
                registry_default: registry_default.as_deref(),
            })
            .map_err(|e| cli_err(EX_PARSE, "pkg/scaffold", e))?,
        ),
        PkgCmd::Build {
            pkg,
            target,
            out_dir,
        } => Some(
            pkg_workspace_ops::handle_build(pkg, target, out_dir, frontend.clone())
                .map_err(|e| cli_err(EX_PARSE, "pkg/build", e))?,
        ),
        PkgCmd::Remove { name, lock } => Some(
            pkg_workspace_ops::handle_remove(name, lock)
                .map_err(|e| cli_err(EX_PARSE, "pkg/remove", e))?,
        ),
        PkgCmd::Migrate {
            pkg,
            lock,
            workspace_file,
            workspace,
            registry_default,
        } => Some(
            pkg_workspace_ops::handle_migrate(
                pkg,
                lock,
                workspace_file,
                workspace.as_deref(),
                registry_default.as_deref(),
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/migrate", e))?,
        ),
        PkgCmd::Abi { pkg } => Some(
            pkg_abi::handle_abi(
                pkg,
                frontend,
                resolved_step_limit(cli),
                resolved_mem_limits(cli),
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/abi", e))?,
        ),
        PkgCmd::Trace {
            pkg,
            requirements,
            commit,
            snapshot,
            policy,
            out,
            no_store,
        } => Some(
            pkg_assurance_ops::handle_trace(
                pkg,
                requirements,
                commit.as_deref(),
                Some(snapshot.as_str()),
                policy.as_deref(),
                out,
                *no_store,
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/trace", e))?,
        ),
        PkgCmd::Qualify {
            commit,
            snapshot,
            policy,
            profile,
            requirements,
            test_artifacts,
            tools,
            out,
            no_store,
        } => Some(
            pkg_assurance_ops::handle_tool_qualification(
                pkg_assurance_ops::ToolQualificationArgs {
                    commit: commit.as_deref(),
                    snapshot,
                    policy: policy.as_deref(),
                    profile,
                    requirement_ids: requirements,
                    test_artifacts,
                    tools,
                    out,
                    no_store: *no_store,
                },
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/qualify", e))?,
        ),
        PkgCmd::AssurancePack {
            pkg,
            assurance_profile,
            commit,
            snapshot,
            policy,
            trace,
            qualification,
            coverage,
            object_equivalence,
            independence_attestations,
            independent_verifier_runs,
            out,
            bundle_dir,
            no_store,
        } => Some(
            pkg_assurance_pack_ops::handle_assurance_pack(
                pkg_assurance_pack_ops::AssurancePackArgs {
                    pkg,
                    assurance_profile,
                    commit: commit.as_deref(),
                    snapshot,
                    policy: policy.as_deref(),
                    trace_spec: trace,
                    qualification_spec: qualification,
                    coverage_specs: coverage,
                    object_equivalence_spec: object_equivalence.as_deref(),
                    independence_attestations,
                    independent_verifier_run_specs: independent_verifier_runs,
                    out,
                    bundle_dir: bundle_dir.as_deref(),
                    no_store: *no_store,
                },
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/assurance-pack", e))?,
        ),
        PkgCmd::Env {
            profile,
            runtime_backend,
            lock,
            workspace_file,
            out_dir,
            hydrate,
        } => Some({
            if *hydrate {
                let missing =
                    pkg_workspace_ops::collect_missing_locked_hashes(workspace_file, lock)
                        .map_err(|e| cli_err(EX_PARSE, "pkg/env", e))?;
                if !missing.is_empty() {
                    let policy = CapsPolicy::load(caps)
                        .with_context(|| format!("read {}", caps.display()))
                        .map_err(caps_parse_cli_err)?;
                    let mut ctx = mk_ctx(cli);
                    let prelude = build_prelude(&mut ctx);
                    let mut env = prelude.env;
                    let forms = canonicalize_module(mk_pkg_env_hydrate_program(&missing))
                        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
                    let program_hash = hash_module(&forms);
                    let prog = eval_module(&mut ctx, &mut env, &forms)
                        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
                    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
                    let run = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
                        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;
                    if let Some(proto) = ctx.protocol
                        && let Value::Sealed { token, payload } = &run.value
                        && *token == proto.error
                    {
                        let (code, message) = match payload.as_ref() {
                            Value::Data(Term::Map(m)) => {
                                let code = m
                                    .get(&gc_coreform::TermOrdKey(Term::symbol(":error/code")))
                                    .and_then(|t| match t {
                                        Term::Str(s) => Some(s.as_str()),
                                        _ => None,
                                    })
                                    .unwrap_or("core/effect/error");
                                let message = m
                                    .get(&gc_coreform::TermOrdKey(Term::symbol(":error/message")))
                                    .and_then(|t| match t {
                                        Term::Str(s) => Some(s.as_str()),
                                        _ => None,
                                    })
                                    .unwrap_or("env hydration failed");
                                (code.to_string(), message.to_string())
                            }
                            _ => (
                                "core/effect/error".to_string(),
                                "env hydration failed".to_string(),
                            ),
                        };
                        let exit_code = if code == "core/caps/denied" {
                            EX_CAPS_DENIED
                        } else {
                            EX_EVAL
                        };
                        return Err(cli_err(
                            exit_code,
                            "pkg/env-hydrate",
                            format!("{code}: {message}"),
                        ));
                    }
                    env_hydrate_log = Some(run.log);
                }
            }
            pkg_workspace_ops::handle_env(
                profile,
                runtime_backend.as_deref(),
                lock,
                workspace_file,
                out_dir,
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/env", e))?
        }),
        PkgCmd::ProfileRuntime {
            out,
            history,
            min_history,
            max_regression_percent,
            no_history_append,
            task_budget_us,
            io_budget_us,
            memory_budget_us,
        } => Some(
            pkg_runtime_profile::handle_runtime_profile(
                out,
                history,
                (*min_history).try_into().map_err(|_| {
                    cli_err(
                        EX_PARSE,
                        "pkg/profile-runtime",
                        format!("--min-history too large for this platform: {min_history}"),
                    )
                })?,
                *max_regression_percent,
                !*no_history_append,
                pkg_runtime_profile::RuntimeProfileBudgets {
                    task_budget_us: *task_budget_us,
                    io_budget_us: *io_budget_us,
                    memory_budget_us: *memory_budget_us,
                },
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/profile-runtime", e))?,
        ),
        _ => None,
    };
    let Some(local) = local else {
        return Ok(None);
    };

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(local.log_op));
    let log_obj =
        env_hydrate_log.unwrap_or_else(|| pkg_workspace_ops::empty_log(local.program_hash));
    std::fs::write(&log_path, log_obj.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let value_v = Value::Data(local.value.clone());
    let ok = extract_pkg_ok_bool(&value_v).unwrap_or(true);
    let exit_code = if ok { EX_OK } else { EX_VERIFY };
    let value = gc_coreform::print_term(&local.value);
    let value_format = "coreform";

    let mut data = serde_json::json!({
        "coreform_frontend": frontend_info,
        "caps": caps.display().to_string(),
        "log": log_path.display().to_string(),
        "value": value,
        "value_format": value_format,
    });
    if let Some(report) = pkg_reports::build_pkg_ai_report(cmd, &value_v, caps)
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert("report".to_string(), report);
    }
    if let Some(obj) = data.as_object_mut() {
        obj.insert(
            "telemetry".to_string(),
            pkg_telemetry::build_pkg_telemetry(
                cmd,
                ok,
                exit_code,
                &log_obj,
                &value_v,
                obj.get("report"),
                None,
            ),
        );
    }

    let stdout = if cli.json {
        String::new()
    } else {
        extract_pkg_lock_hash(&value_v)
            .or_else(|| extract_pkg_export_bundle_hash(&value_v))
            .map(|h| format!("{h}\n"))
            .unwrap_or_else(|| format!("{value}\n"))
    };
    let env = JsonEnvelope {
        ok,
        kind: local.kind,
        data: Some(data),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "pkg/error",
                message: "pkg operation failed".to_string(),
                context: None,
            })
        },
    };

    Ok(Some(CmdOut {
        exit_code,
        stdout,
        json: json_envelope_value(env)?,
    }))
}

fn parse_task_engine(engine: Option<String>) -> Result<Option<FmtEngine>, CliError> {
    match engine {
        None => Ok(None),
        Some(raw) => raw
            .parse::<FmtEngine>()
            .map(Some)
            .map_err(|e| cli_err(EX_PARSE, "pkg/run", e)),
    }
}
