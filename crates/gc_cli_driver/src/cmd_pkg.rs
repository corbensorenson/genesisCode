use super::*;
mod frontend_dispatch;

pub(super) fn cmd_pkg(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &PkgCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;
    if let Some(out) =
        cmd_pkg_local_workspace_ops(cli, cmd, caps, log, &frontend, frontend_info.clone())?
    {
        return Ok(out);
    }

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let (prog, kind, log_op, program_hash) =
        frontend_dispatch::build_pkg_effect_program(cli, cmd, &frontend, &mut ctx, &mut env)?;
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
            && let Some(Term::Str(code)) =
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code")))
        {
            if code == "core/caps/denied" {
                exit_code = EX_CAPS_DENIED;
            } else if matches!(cmd, PkgCmd::Publish { .. })
                && (code.starts_with("core/pkg/")
                    || code.starts_with("core/refs/")
                    || code == "core/store/not-found")
            {
                exit_code = EX_OBLIGATIONS;
            }
        }
    } else if matches!(
        cmd,
        PkgCmd::Install { .. } | PkgCmd::Verify { .. } | PkgCmd::Doctor { .. }
    ) && let Some(false) = extract_pkg_ok_bool(&r.value)
    {
        ok = false;
        exit_code = EX_VERIFY;
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    if !ok
        && exit_code == EX_EVAL
        && matches!(cmd, PkgCmd::Publish { .. })
        && (value.contains("core/pkg/")
            || value.contains("core/refs/")
            || value.contains("core/store/not-found"))
    {
        exit_code = EX_OBLIGATIONS;
    }
    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            PkgCmd::New { .. }
            | PkgCmd::Remove { .. }
            | PkgCmd::Migrate { .. }
            | PkgCmd::Run { .. }
            | PkgCmd::Test { .. }
            | PkgCmd::SelfOptimize { .. }
            | PkgCmd::Env { .. } => extract_pkg_lock_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Init { .. }
            | PkgCmd::Add { .. }
            | PkgCmd::Lock { .. }
            | PkgCmd::Update { .. } => extract_pkg_lock_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Install { .. } | PkgCmd::Verify { .. } => {
                if ok {
                    "ok\n".to_string()
                } else {
                    format!("{value}\n")
                }
            }
            PkgCmd::Doctor { .. } => {
                if ok {
                    "ok\n".to_string()
                } else {
                    format!("{value}\n")
                }
            }
            PkgCmd::List { .. } | PkgCmd::Info { .. } | PkgCmd::Abi { .. } => {
                format!("{value}\n")
            }
            PkgCmd::Snapshot { .. } => extract_pkg_snapshot_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Export { .. } => extract_pkg_export_bundle_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Import { .. } => extract_pkg_import_root(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Publish { .. } => extract_pkg_publish_commit(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| {
                    if ok {
                        "ok\n".to_string()
                    } else {
                        format!("{value}\n")
                    }
                }),
        }
    };

    let doctor_report = if let PkgCmd::Doctor { lock } = cmd {
        Some(pkg_doctor::build_pkg_doctor_report(
            &ctx, &r.value, caps, lock, ok, exit_code,
        ))
    } else {
        None
    };
    if let Some(report) = &doctor_report
        && !report.ok
    {
        ok = false;
        if exit_code == EX_OK {
            exit_code = EX_VERIFY;
        }
    }
    let ai_report = pkg_reports::build_pkg_ai_report(cmd, &r.value, caps);

    let mut data = serde_json::json!({
        "coreform_frontend": frontend_info,
        "caps": caps.display().to_string(),
        "log": log_path.display().to_string(),
        "value": value,
        "value_format": value_format,
    });
    if let Some(report) = doctor_report
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert("doctor".to_string(), report.json);
    }
    if let Some(report) = ai_report
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert("report".to_string(), report);
    }
    if let Some(obj) = data.as_object_mut() {
        let telemetry = pkg_telemetry::build_pkg_telemetry(
            cmd,
            ok,
            exit_code,
            &r.log,
            &r.value,
            obj.get("report"),
            obj.get("doctor"),
        );
        obj.insert("telemetry".to_string(), telemetry);
    }

    let env = JsonEnvelope {
        ok,
        kind,
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

    Ok(CmdOut {
        exit_code,
        stdout,
        json: json_envelope_value(env)?,
    })
}

fn cmd_pkg_local_workspace_ops(
    cli: &Cli,
    cmd: &PkgCmd,
    caps: &Path,
    log: Option<&Path>,
    frontend: &gc_obligations::CoreformFrontend,
    frontend_info: serde_json::Value,
) -> Result<Option<CmdOut>, CliError> {
    match cmd {
        PkgCmd::Run {
            task,
            workspace_file,
        } => {
            let action = pkg_task_runner::resolve_workspace_task(workspace_file, task)
                .map_err(|e| cli_err(EX_PARSE, "pkg/run", e))?;
            let out = match action {
                pkg_task_runner::WorkspaceTaskAction::Test { pkg } => {
                    cmd_test(cli, &pkg, Some(caps))?
                }
                pkg_task_runner::WorkspaceTaskAction::Pack { pkg } => cmd_pack(cli, &pkg)?,
                pkg_task_runner::WorkspaceTaskAction::Typecheck { pkg } => {
                    cmd_typecheck(cli, &pkg)?
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
        PkgCmd::Env {
            profile,
            lock,
            workspace_file,
            out_dir,
        } => Some(
            pkg_workspace_ops::handle_env(profile, lock, workspace_file, out_dir)
                .map_err(|e| cli_err(EX_PARSE, "pkg/env", e))?,
        ),
        _ => None,
    };
    let Some(local) = local else {
        return Ok(None);
    };

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(local.log_op));
    let log_obj = pkg_workspace_ops::empty_log(local.program_hash);
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
