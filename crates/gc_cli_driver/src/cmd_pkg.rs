use super::*;
mod frontend_dispatch;
mod local_workspace_ops;

#[derive(Debug, Clone)]
struct StrictSoundCheckResult {
    requested: bool,
    pkg: PathBuf,
    ok: bool,
    report_coreform: Option<String>,
    error: Option<String>,
}

pub(super) fn cmd_pkg(
    cli: &Cli,
    flavor: Flavor,
    caps: &Path,
    log: Option<&Path>,
    cmd: &PkgCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;
    if let Some(out) = cmd_pkg_local_workspace_ops(
        cli,
        flavor,
        cmd,
        caps,
        log,
        &frontend,
        frontend_info.clone(),
    )? {
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
    let contract_value = normalize_pkg_contract_value(cmd, &r.value);

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &contract_value
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
    ) && let Some(false) = extract_pkg_ok_bool(&contract_value)
    {
        ok = false;
        exit_code = EX_VERIFY;
    }

    let strict_sound = run_pkg_strict_sound_check(cli, cmd, &frontend);
    if let Some(strict) = &strict_sound
        && strict.requested
        && !strict.ok
    {
        ok = false;
        if exit_code == EX_OK {
            exit_code = EX_VERIFY;
        }
    }

    let (value, value_format) = render_value_for_cli(&ctx, &contract_value);
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
            | PkgCmd::Scaffold { .. }
            | PkgCmd::Remove { .. }
            | PkgCmd::Migrate { .. }
            | PkgCmd::Run { .. }
            | PkgCmd::Build { .. }
            | PkgCmd::Test { .. }
            | PkgCmd::SelfOptimize { .. }
            | PkgCmd::Trace { .. }
            | PkgCmd::Qualify { .. }
            | PkgCmd::AssurancePack { .. }
            | PkgCmd::Env { .. }
            | PkgCmd::ProfileRuntime { .. } => extract_pkg_lock_hash(&contract_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Init { .. }
            | PkgCmd::Add { .. }
            | PkgCmd::Lock { .. }
            | PkgCmd::Update { .. } => extract_pkg_lock_hash(&contract_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Install { .. } | PkgCmd::Verify { .. } => {
                if ok {
                    "ok\n".to_string()
                } else if let Some(strict) = &strict_sound
                    && strict.requested
                    && !strict.ok
                {
                    strict_sound_stdout(strict)
                } else {
                    format!("{value}\n")
                }
            }
            PkgCmd::Doctor { .. } => {
                if ok {
                    "ok\n".to_string()
                } else if let Some(strict) = &strict_sound
                    && strict.requested
                    && !strict.ok
                {
                    strict_sound_stdout(strict)
                } else {
                    format!("{value}\n")
                }
            }
            PkgCmd::List { .. } | PkgCmd::Info { .. } | PkgCmd::Abi { .. } => {
                format!("{value}\n")
            }
            PkgCmd::Snapshot { .. } => extract_pkg_snapshot_hash(&contract_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Export { .. } => extract_pkg_export_bundle_hash(&contract_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Import { .. } => extract_pkg_import_root(&contract_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Publish { .. } => extract_pkg_publish_commit(&contract_value)
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

    let strict_sound_doctor = strict_sound.as_ref().and_then(|strict| {
        if strict.requested {
            Some(pkg_doctor::StrictSoundDoctorInput {
                pkg: strict.pkg.display().to_string(),
                ok: strict.ok,
                error: strict.error.clone(),
            })
        } else {
            None
        }
    });
    let doctor_report = if let PkgCmd::Doctor { lock, .. } = cmd {
        Some(pkg_doctor::build_pkg_doctor_report(
            &ctx,
            &contract_value,
            caps,
            lock,
            ok,
            exit_code,
            strict_sound_doctor.as_ref(),
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
    let ai_report = pkg_reports::build_pkg_ai_report(cmd, &contract_value, caps);

    let mut data = serde_json::json!({
        "coreform_frontend": frontend_info,
        "caps": caps.display().to_string(),
        "log": log_path.display().to_string(),
        "value": value,
        "value_format": value_format,
    });
    if let Some(strict) = &strict_sound
        && strict.requested
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert(
            "strict_sound".to_string(),
            serde_json::json!({
                "requested": strict.requested,
                "pkg": strict.pkg.display().to_string(),
                "ok": strict.ok,
                "error": strict.error,
                "report_coreform": strict.report_coreform,
            }),
        );
    }
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
            &contract_value,
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

fn normalize_pkg_contract_value(cmd: &PkgCmd, value: &Value) -> Value {
    if !matches!(cmd, PkgCmd::Update { .. }) {
        return value.clone();
    }
    let Value::Data(Term::Map(m)) = value else {
        return value.clone();
    };
    let mut out = m.clone();
    out.entry(gc_coreform::TermOrdKey(Term::symbol(":selected-count")))
        .or_insert(Term::Int(0.into()));
    out.entry(gc_coreform::TermOrdKey(Term::symbol(":rationale-count")))
        .or_insert(Term::Int(0.into()));
    out.entry(gc_coreform::TermOrdKey(Term::symbol(":rationale")))
        .or_insert(Term::Vector(Vec::new()));
    Value::Data(Term::Map(out))
}

fn run_pkg_strict_sound_check(
    cli: &Cli,
    cmd: &PkgCmd,
    frontend: &gc_obligations::CoreformFrontend,
) -> Option<StrictSoundCheckResult> {
    let (pkg, requested) = match cmd {
        PkgCmd::Verify {
            pkg, strict_sound, ..
        }
        | PkgCmd::Doctor {
            pkg, strict_sound, ..
        } => (pkg.clone(), *strict_sound),
        _ => return None,
    };
    if !requested {
        return None;
    }

    match gc_obligations::typecheck_package_with_step_limit_and_frontend(
        &pkg,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend.clone(),
        true,
    ) {
        Ok(report) => Some(StrictSoundCheckResult {
            requested: true,
            pkg,
            ok: report.ok,
            report_coreform: Some(report.report_coreform),
            error: None,
        }),
        Err(e) => Some(StrictSoundCheckResult {
            requested: true,
            pkg,
            ok: false,
            report_coreform: None,
            error: Some(e.to_string()),
        }),
    }
}

fn strict_sound_stdout(strict: &StrictSoundCheckResult) -> String {
    if let Some(report) = &strict.report_coreform {
        format!("{report}\n")
    } else if let Some(error) = &strict.error {
        format!(
            "strict-sound diagnostics failed for {}: {error}\n",
            strict.pkg.display()
        )
    } else {
        format!(
            "strict-sound diagnostics failed for {}\n",
            strict.pkg.display()
        )
    }
}

fn cmd_pkg_local_workspace_ops(
    cli: &Cli,
    flavor: Flavor,
    cmd: &PkgCmd,
    caps: &Path,
    log: Option<&Path>,
    frontend: &gc_obligations::CoreformFrontend,
    frontend_info: serde_json::Value,
) -> Result<Option<CmdOut>, CliError> {
    local_workspace_ops::cmd_pkg_local_workspace_ops(
        cli,
        flavor,
        cmd,
        caps,
        log,
        frontend,
        frontend_info,
    )
}
