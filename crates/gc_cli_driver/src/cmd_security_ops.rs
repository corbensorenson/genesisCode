use super::*;

pub(super) fn cmd_keygen(cli: &Cli, out: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let out_buf = if frontend_is_rust(&frontend) {
        out.to_path_buf()
    } else {
        let req = Term::Map(
            [(
                TermOrdKey(Term::symbol(":out")),
                Term::Str(out.display().to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let planned = selfhost_plan_request_map(cli, "core/cli::keygen-request", req, "keygen")?;
        PathBuf::from(planned_required_str(&planned, ":out", "keygen")?)
    };
    let out = out_buf.as_path();

    let k = gc_obligations::KeyFile::generate_ed25519();
    k.write_secure(out)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/keygen-v0.2",
        data: Some(serde_json::json!({
            "out": out.display().to_string(),
            "alg": k.alg,
            "pk_b64": k.pk_b64,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", out.display())
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_sign(
    cli: &Cli,
    pkg: &Path,
    key_path: &Path,
    acceptance: Option<&str>,
    signatures: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let (pkg_buf, key_path_buf, acceptance_buf, signatures_buf) = if frontend_is_rust(&frontend) {
        (
            pkg.to_path_buf(),
            key_path.to_path_buf(),
            acceptance.map(|s| s.to_string()),
            signatures.map(Path::to_path_buf),
        )
    } else {
        let req = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":pkg")),
                    Term::Str(pkg.display().to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":key")),
                    Term::Str(key_path.display().to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":acceptance")),
                    acceptance
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":signatures")),
                    signatures
                        .map(|p| Term::Str(p.display().to_string()))
                        .unwrap_or(Term::Nil),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let planned = selfhost_plan_request_map(cli, "core/cli::sign-request", req, "sign")?;
        (
            PathBuf::from(planned_required_str(&planned, ":pkg", "sign")?),
            PathBuf::from(planned_required_str(&planned, ":key", "sign")?),
            planned_optional_str(&planned, ":acceptance", "sign")?,
            planned_optional_str(&planned, ":signatures", "sign")?.map(PathBuf::from),
        )
    };
    let pkg = pkg_buf.as_path();
    let key_path = key_path_buf.as_path();
    let acceptance = acceptance_buf.as_deref();
    let signatures = signatures_buf.as_deref();

    let (_manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "manifest/parse", format!("{e}")))?;
    let store = gc_obligations::EvidenceStore::open(&pkg_dir).map_err(obligation_err)?;

    let acc_hex = match acceptance {
        Some(s) => s.trim().to_string(),
        None => gc_obligations::read_acceptance_hash_from_last(&pkg_dir).map_err(|e| match e {
            gc_obligations::SigningError::Io(_) => cli_err(EX_IO, "io/read", format!("{e}")),
            _ => cli_err(EX_PARSE, "sign/acceptance", format!("{e}")),
        })?,
    };

    let k = gc_obligations::KeyFile::load(key_path)
        .map_err(|e| cli_err(EX_PARSE, "sign/key", format!("{e}")))?;
    let sk = k
        .signing_key()
        .map_err(|e| cli_err(EX_PARSE, "sign/key", format!("{e}")))?;

    let (sig_artifact, _rec) = gc_obligations::sign_acceptance_hash(&store, &acc_hex, &sk)
        .map_err(|e| cli_err(EX_INTERNAL, "sign/error", format!("{e}")))?;

    // Update .genesis/last_signature and the signature set.
    let genesis_dir = pkg_dir.join(".genesis");
    std::fs::create_dir_all(&genesis_dir)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    std::fs::write(
        genesis_dir.join("last_signature"),
        format!("{sig_artifact}\n"),
    )
    .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let sigset_path = signatures
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| gc_obligations::signatures_file_path(&pkg_dir));
    let mut set = gc_obligations::load_signature_set(&sigset_path).unwrap_or_default();
    set.push(sig_artifact.clone());
    gc_obligations::write_signature_set(&sigset_path, &set)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    // Append a transparency log entry (best-effort deterministic format; if this fails, treat as error).
    let pkg_artifact = gc_obligations::package_artifact_hash(pkg).map_err(obligation_err)?;
    let transparency_entry = gc_obligations::append_transparency_entry(
        &store,
        &pkg_dir,
        &pkg_artifact,
        &acc_hex,
        &sig_artifact,
        &k.pk_b64,
    )
    .map_err(|e| cli_err(EX_INTERNAL, "transparency/error", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/sign-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "key": key_path.display().to_string(),
            "package_artifact": pkg_artifact,
            "acceptance_artifact": acc_hex,
            "signature_artifact": sig_artifact,
            "sigset": sigset_path.display().to_string(),
            "transparency_entry": transparency_entry,
            "pk_b64": k.pk_b64,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{sig_artifact}\n")
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_transparency_verify(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let pkg_buf = if frontend_is_rust(&frontend) {
        pkg.to_path_buf()
    } else {
        let req = Term::Map(
            [(
                TermOrdKey(Term::symbol(":pkg")),
                Term::Str(pkg.display().to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let planned = selfhost_plan_request_map(
            cli,
            "core/cli::transparency-verify-request",
            req,
            "transparency-verify",
        )?;
        PathBuf::from(planned_required_str(
            &planned,
            ":pkg",
            "transparency-verify",
        )?)
    };
    let pkg = pkg_buf.as_path();

    let (_manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "manifest/parse", format!("{e}")))?;
    let store = gc_obligations::EvidenceStore::open(&pkg_dir).map_err(obligation_err)?;
    let r = gc_obligations::verify_transparency_log(&store, &pkg_dir)
        .map_err(|e| cli_err(EX_INTERNAL, "transparency/error", format!("{e}")))?;
    let exit_code = if r.ok { EX_OK } else { EX_VERIFY };
    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/transparency-verify-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "head": r.head,
            "entries": r.entries,
            "errors": r.errors,
        })),
        error: None,
    };
    let mut stdout = String::new();
    if !cli.json {
        stdout.push_str(if r.ok { "ok\n" } else { "not ok\n" });
    }
    Ok(CmdOut {
        exit_code,
        stdout,
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_typecheck(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);
    let result = gc_obligations::typecheck_package_with_step_limit_and_frontend(
        pkg,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(obligation_err)?;
    let report_s = result.report_coreform;

    let exit_code = if result.ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: result.ok,
        kind: "genesis/typecheck-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "coreform_frontend": frontend_info,
            "report_coreform": report_s,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{report_s}\n")
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_optimize(
    cli: &Cli,
    file: &PathBuf,
    out: Option<&PathBuf>,
    emit_wasm: Option<&PathBuf>,
    engine: Option<FmtEngine>,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<CmdOut, CliError> {
    let engine = resolved_engine(cli, "optimize", engine)?;
    let frontend_info = coreform_frontend_for_engine_json(cli, engine)?;
    let src = std::fs::read_to_string(file)
        .with_context(|| format!("read {}", file.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;

    let forms = match engine {
        FmtEngine::Rust => {
            let forms = parse_module(&src)
                .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
            canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?
        }
        FmtEngine::Selfhost => {
            let mut ctx = EvalCtx::with_step_limit(None);
            ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut ctx, &mut env)?;
            ctx.steps = 0;
            ctx.step_limit = None;
            selfhost_parse_canonicalize_module(&mut ctx, &env, &src)?
        }
    };
    let pipeline =
        gc_opt::optimize_command_pipeline(&forms, stage1_gate, stage2_gate, emit_wasm.is_some())
            .map_err(|e| match e {
                gc_opt::OptimizeCommandError::Stage1Build(msg) => {
                    cli_err(EX_INTERNAL, "stage1/error", msg)
                }
                gc_opt::OptimizeCommandError::Stage1Gate(out) => CliError {
                    exit_code: EX_OBLIGATIONS,
                    json: JsonError {
                        code: "obligation/stage1-validation",
                        message: "core/obligation::stage1-validation failed".to_string(),
                        context: Some(gc_opt::stage1_pipeline_json(&out)),
                    },
                },
                gc_opt::OptimizeCommandError::Stage2Gate(s2) => CliError {
                    exit_code: EX_OBLIGATIONS,
                    json: JsonError {
                        code: "obligation/translation-validation",
                        message:
                            "core/obligation::translation-validation (stage2 CoreForm->WASM) failed"
                                .to_string(),
                        context: Some(gc_opt::stage2_report_json(&s2)),
                    },
                },
                gc_opt::OptimizeCommandError::Stage2Compile(e) => match e {
                    gc_opt::Stage2CompileError::Unsupported(msg) => {
                        cli_err(EX_OBLIGATIONS, "stage2/unsupported", msg)
                    }
                    gc_opt::Stage2CompileError::Internal(msg) => {
                        cli_err(EX_INTERNAL, "stage2/error", msg)
                    }
                },
            })?;

    if let Some(p) = emit_wasm {
        let art = pipeline.wasm_artifact.as_ref().ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "stage2/error",
                "missing wasm artifact from optimize pipeline",
            )
        })?;
        std::fs::write(p, &art.wasm_bytes)
            .with_context(|| format!("write {}", p.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }

    let out_s = print_module(&pipeline.optimized_forms);

    if let Some(p) = out {
        std::fs::write(p, out_s.as_bytes())
            .with_context(|| format!("write {}", p.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }

    let stdout = if cli.json || out.is_some() {
        String::new()
    } else {
        out_s.clone()
    };

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/optimize-v0.2",
        data: Some(serde_json::json!({
            "file": file.display().to_string(),
            "out": out.map(|p| p.display().to_string()),
            "wasm_out": emit_wasm.map(|p| p.display().to_string()),
            "engine": match engine {
                FmtEngine::Rust => "rust",
                FmtEngine::Selfhost => "selfhost",
            },
            "selfhost_artifact": selfhost_artifact_identity_for_engine(cli, engine),
            "coreform_frontend": frontend_info,
            "stage1": gc_opt::stage1_pipeline_json(&pipeline.stage1),
            "stage2": pipeline.stage2.as_ref().map(gc_opt::stage2_report_json),
            "changed": pipeline.changed,
            "original_hash": hex32(pipeline.original_hash),
            "optimized_hash": hex32(pipeline.optimized_hash),
            "egg_runs": pipeline.stage1.optimize_report.stats.egg_runs,
            "egg_iterations": pipeline.stage1.optimize_report.stats.iterations,
            "egg_eclasses": pipeline.stage1.optimize_report.stats.eclasses,
            "egg_enodes": pipeline.stage1.optimize_report.stats.enodes,
            "egg_rewrites_applied": pipeline.stage1.optimize_report.stats.rewrites_applied,
            "optimized_coreform": out_s,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout,
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_semantic_edit(cli: &Cli, cmd: &SemanticEditCmd) -> Result<CmdOut, CliError> {
    match cmd {
        SemanticEditCmd::Index { pkg, module_path } => {
            cmd_semantic_edit_index(cli, pkg, module_path)
        }
    }
}

pub(super) fn cmd_semantic_edit_index(
    cli: &Cli,
    pkg: &Path,
    module_path: &str,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);
    let (_manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "package/invalid", format!("{e}")))?;
    let module_abs = pkg_dir.join(module_path);
    let src = std::fs::read_to_string(&module_abs)
        .with_context(|| format!("read {}", module_abs.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let nodes = gc_patches::semantic_node_index_for_module_with_frontend(
        module_path,
        &src,
        &frontend,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
    )
    .map_err(|e| match e {
        gc_patches::PatchError::Parse(_) | gc_patches::PatchError::Validate(_) => {
            cli_err(EX_PARSE, "semantic-edit/invalid", format!("{e}"))
        }
        gc_patches::PatchError::Io(_) => cli_err(EX_IO, "io/error", format!("{e}")),
        gc_patches::PatchError::Obligations(inner) => obligation_err(inner),
    })?;
    let nodes_json: Vec<serde_json::Value> = nodes
        .iter()
        .map(|node| {
            serde_json::json!({
                "module_path": node.module_path,
                "node_id": node.node_id,
                "path": print_term(&node.path),
                "path_repr": node.path_repr,
                "term_tag": node.term_tag,
                "term_hash": node.term_hash,
            })
        })
        .collect();

    let mut stdout = String::new();
    if !cli.json {
        for node in &nodes {
            stdout.push_str(&format!(
                "{} {} {} {}\n",
                node.node_id, node.path_repr, node.term_tag, node.term_hash
            ));
        }
    }
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/semantic-edit-index-v0.1",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "module_path": module_path,
            "module_abs": module_abs.display().to_string(),
            "coreform_frontend": frontend_info,
            "node_count": nodes.len(),
            "nodes": nodes_json,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout,
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_apply_patch(
    cli: &Cli,
    patch: &Path,
    pkg: &Path,
    caps: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let r = gc_patches::apply_patch_with_step_limit_and_frontend(
        patch,
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(|e| match e {
        gc_patches::PatchError::Parse(_) | gc_patches::PatchError::Validate(_) => {
            cli_err(EX_PARSE, "patch/invalid", format!("{e}"))
        }
        gc_patches::PatchError::Io(_) => cli_err(EX_IO, "io/error", format!("{e}")),
        gc_patches::PatchError::Obligations(inner) => obligation_err(inner),
    })?;

    let exit_code = if r.ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/apply-patch-v0.2",
        data: Some(serde_json::json!({
            "patch": patch.display().to_string(),
            "pkg": pkg.display().to_string(),
            "caps": caps.map(|p| p.display().to_string()),
            "coreform_frontend": frontend_info,
            "patch_artifact": r.patch_artifact,
            "report_artifact": r.report_artifact,
            "acceptance_artifact": r.acceptance_artifact,
            "package_artifact": r.package_artifact,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", r.report_artifact)
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_verify(
    cli: &Cli,
    pkg: &Path,
    acceptance: Option<&str>,
    policy: Option<&Path>,
    signatures: Option<&Path>,
    scan_store: bool,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let (pkg_buf, acceptance_buf, policy_buf, signatures_buf, scan_store) =
        if frontend_is_rust(&frontend) {
            (
                pkg.to_path_buf(),
                acceptance.map(|s| s.to_string()),
                policy.map(Path::to_path_buf),
                signatures.map(Path::to_path_buf),
                scan_store,
            )
        } else {
            let req = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":pkg")),
                        Term::Str(pkg.display().to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":acceptance")),
                        acceptance
                            .map(|s| Term::Str(s.to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":policy")),
                        policy
                            .map(|p| Term::Str(p.display().to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":signatures")),
                        signatures
                            .map(|p| Term::Str(p.display().to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":scan-store")),
                        Term::Bool(scan_store),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let planned =
                selfhost_plan_request_map(cli, "core/cli::verify-request", req, "verify")?;
            (
                PathBuf::from(planned_required_str(&planned, ":pkg", "verify")?),
                planned_optional_str(&planned, ":acceptance", "verify")?,
                planned_optional_str(&planned, ":policy", "verify")?.map(PathBuf::from),
                planned_optional_str(&planned, ":signatures", "verify")?.map(PathBuf::from),
                planned_required_bool(&planned, ":scan-store", "verify")?,
            )
        };
    let pkg = pkg_buf.as_path();
    let acceptance = acceptance_buf.as_deref();
    let policy = policy_buf.as_deref();
    let signatures = signatures_buf.as_deref();

    let r =
        gc_obligations::verify_package_with_policy(pkg, acceptance, scan_store, policy, signatures)
            .map_err(obligation_err)?;
    let exit_code = if r.ok { EX_OK } else { EX_VERIFY };

    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/verify-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "acceptance_artifact": r.acceptance_artifact,
            "policy": policy.map(|p| p.display().to_string()),
            "signatures": signatures.map(|p| p.display().to_string()),
            "policy_min_signatures": r.policy_min_signatures,
            "checked_signatures": r.checked_signatures,
            "valid_signatures": r.valid_signatures,
            "store_scanned": r.store_scanned,
            "checked_modules": r.checked_modules,
            "checked_deps": r.checked_deps,
            "checked_artifacts": r.checked_artifacts,
            "errors": r.errors,
        })),
        error: None,
    };

    let mut stdout = String::new();
    if !cli.json {
        stdout.push_str(if r.ok { "ok\n" } else { "not ok\n" });
    }
    Ok(CmdOut {
        exit_code,
        stdout,
        json: json_envelope_value(env)?,
    })
}
