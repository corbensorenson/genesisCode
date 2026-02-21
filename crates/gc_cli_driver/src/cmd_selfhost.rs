use super::*;

pub(super) fn percent_basis_points(part: usize, total: usize) -> u64 {
    if total == 0 {
        return 0;
    }
    ((part as u128 * 10_000u128) / total as u128) as u64
}

pub(super) fn percent_string_from_bps(bps: u64) -> String {
    format!("{}.{:02}%", bps / 100, bps % 100)
}

pub(super) fn write_content_addressed_artifact(
    store_dir: &Path,
    bytes: &[u8],
) -> Result<(String, PathBuf), CliError> {
    std::fs::create_dir_all(store_dir)
        .with_context(|| format!("create {}", store_dir.display()))
        .map_err(|e| cli_err(EX_IO, "io/mkdir", format!("{e}")))?;

    let hex = blake3::hash(bytes).to_hex().to_string();
    let path = store_dir.join(&hex);
    if !path.is_file() {
        std::fs::write(&path, bytes)
            .with_context(|| format!("write {}", path.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }
    Ok((hex, path))
}

pub(super) fn cmd_selfhost_dashboard(
    cli: &Cli,
    markdown: Option<&Path>,
    store: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let artifact = resolved_selfhost_artifact_for_frontend(cli);
    let artifact_path = artifact.as_ref().map(|p| p.display().to_string());
    let artifact_exists = artifact.as_ref().is_some_and(|p| p.is_file());
    let strict = selfhost_only_enabled(cli);

    let total_commands = SELFHOST_CUTOVER_ROWS.len();
    let routed_count = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.selfhost_routed)
        .count();
    let default_selfhost_count = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.default_selfhost)
        .count();
    let fast_path_total = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.fast_path_required)
        .count();
    let fast_path_default_ok = SELFHOST_CUTOVER_ROWS
        .iter()
        .filter(|r| r.fast_path_required)
        .all(|r| r.default_selfhost && r.selfhost_routed);
    let routed_bps = percent_basis_points(routed_count, total_commands);
    let default_bps = percent_basis_points(default_selfhost_count, total_commands);

    let rows_term: Vec<Term> = SELFHOST_CUTOVER_ROWS
        .iter()
        .map(|row| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str(row.cmd.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":fast-path-required")),
                        Term::Bool(row.fast_path_required),
                    ),
                    (
                        TermOrdKey(Term::symbol(":selfhost-routed")),
                        Term::Bool(row.selfhost_routed),
                    ),
                    (
                        TermOrdKey(Term::symbol(":default-selfhost")),
                        Term::Bool(row.default_selfhost),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();

    let dashboard_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/selfhost-cutover-dashboard-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":strict")), Term::Bool(strict)),
            (
                TermOrdKey(Term::symbol(":artifact-configured")),
                Term::Bool(artifact.is_some()),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-exists")),
                Term::Bool(artifact_exists),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-path")),
                artifact
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":summary")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":total-commands")),
                            Term::Int((total_commands as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-routed-commands")),
                            Term::Int((routed_count as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-default-commands")),
                            Term::Int((default_selfhost_count as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":fast-path-required-commands")),
                            Term::Int((fast_path_total as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":fast-path-default-ok")),
                            Term::Bool(fast_path_default_ok),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-routed-bps")),
                            Term::Int((routed_bps as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-default-bps")),
                            Term::Int((default_bps as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":commands")),
                Term::Vector(rows_term),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact_bytes = print_term(&dashboard_term);

    let store_dir = store
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DASHBOARD_STORE_DEFAULT_REL));
    let (artifact_hash, artifact_path_fs) =
        write_content_addressed_artifact(&store_dir, artifact_bytes.as_bytes())?;

    let markdown_path = markdown
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DASHBOARD_MARKDOWN_DEFAULT_REL));
    let markdown_body = {
        let mut lines = vec![
            "# Selfhost Cutover Dashboard (v0.2)".to_string(),
            "".to_string(),
            format!("- Artifact hash: `{artifact_hash}`"),
            format!("- Store artifact: `{}`", artifact_path_fs.display()),
            format!(
                "- Selfhost toolchain artifact configured: `{}`",
                artifact_path.as_deref().unwrap_or("none")
            ),
            format!("- Selfhost toolchain artifact exists: `{artifact_exists}`"),
            "".to_string(),
            "## Summary".to_string(),
            "".to_string(),
            "| Metric | Value |".to_string(),
            "| --- | --- |".to_string(),
            format!("| Total command groups | {} |", total_commands),
            format!("| Selfhost-routed command groups | {} |", routed_count),
            format!(
                "| Selfhost-routed coverage | {} |",
                percent_string_from_bps(routed_bps)
            ),
            format!(
                "| Default selfhost coverage | {} |",
                percent_string_from_bps(default_bps)
            ),
            format!("| Fast-path default OK | {} |", fast_path_default_ok),
            "".to_string(),
            "## Command Coverage".to_string(),
            "".to_string(),
            "| Command | Fast Path | Selfhost Routed | Default Selfhost |".to_string(),
            "| --- | --- | --- | --- |".to_string(),
        ];
        for row in SELFHOST_CUTOVER_ROWS {
            lines.push(format!(
                "| `{}` | {} | {} | {} |",
                row.cmd, row.fast_path_required, row.selfhost_routed, row.default_selfhost
            ));
        }
        lines.push(String::new());
        lines.join("\n")
    };
    if let Some(parent) = markdown_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))
            .map_err(|e| cli_err(EX_IO, "io/mkdir", format!("{e}")))?;
    }
    std::fs::write(&markdown_path, markdown_body.as_bytes())
        .with_context(|| format!("write {}", markdown_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/selfhost-dashboard-v0.2",
        data: Some(serde_json::json!({
            "artifact_hash": artifact_hash,
            "store_artifact": artifact_path_fs.display().to_string(),
            "store_dir": store_dir.display().to_string(),
            "markdown": markdown_path.display().to_string(),
            "artifact_configured": artifact.is_some(),
            "artifact_exists": artifact_exists,
            "artifact_path": artifact_path,
            "summary": {
                "total_commands": total_commands,
                "selfhost_routed_commands": routed_count,
                "selfhost_default_commands": default_selfhost_count,
                "fast_path_required_commands": fast_path_total,
                "fast_path_default_ok": fast_path_default_ok,
                "selfhost_routed_percent": percent_string_from_bps(routed_bps),
                "selfhost_default_percent": percent_string_from_bps(default_bps),
            }
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!(
                "{}\n{}\n",
                artifact_path_fs.display(),
                markdown_path.display()
            )
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_selfhost_artifact(
    cli: &Cli,
    out: &Path,
    min_stage2_supported_modules: u64,
    min_stage2_validated_modules: u64,
) -> Result<CmdOut, CliError> {
    #[derive(Debug, Clone)]
    struct Stage2Seed {
        source: String,
        forms: Vec<Term>,
        module_hash: [u8; 32],
        stage2_module_hash: [u8; 32],
        stage1_ok: bool,
        stage1_errors: Vec<String>,
        supported: bool,
        ok: bool,
        errors: Vec<String>,
        wasm_hash: Option<[u8; 32]>,
        wasm_bytes_len: Option<usize>,
    }

    #[derive(Debug, Clone)]
    struct Stage2Summary {
        module_hash: [u8; 32],
        supported: bool,
        ok: bool,
        errors: Vec<String>,
        wasm_hash: Option<[u8; 32]>,
        wasm_bytes_len: Option<usize>,
    }

    #[derive(Debug, Clone)]
    struct Stage2SeedIndex {
        generated_by: Option<String>,
        modules: std::collections::BTreeMap<String, Stage2Seed>,
    }

    fn load_stage2_seed_index(path: &Path) -> Option<Stage2SeedIndex> {
        let src = std::fs::read_to_string(path).ok()?;
        let term = parse_term(&src).ok()?;
        let Term::Map(root) = term else { return None };
        match root.get(&TermOrdKey(Term::symbol(":kind"))) {
            Some(Term::Str(s)) if s == "genesis/selfhost-toolchain-artifact-v0.2" => {}
            _ => return None,
        }
        let generated_by = match root.get(&TermOrdKey(Term::symbol(":generated-by"))) {
            Some(Term::Str(s)) => Some(s.clone()),
            _ => None,
        };
        let modules = match root.get(&TermOrdKey(Term::symbol(":modules"))) {
            Some(Term::Vector(v)) => v,
            _ => return None,
        };
        let mut out = std::collections::BTreeMap::new();
        for m in modules {
            let Term::Map(mm) = m else { continue };
            let path = match mm.get(&TermOrdKey(Term::symbol(":path"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => continue,
            };
            let source = match mm.get(&TermOrdKey(Term::symbol(":source"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => continue,
            };
            let forms = match mm.get(&TermOrdKey(Term::symbol(":forms"))) {
                Some(Term::Vector(v)) => v.clone(),
                _ => continue,
            };
            let module_hash = match mm.get(&TermOrdKey(Term::symbol(":module-h"))) {
                Some(Term::Bytes(b)) if b.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(b.as_ref());
                    h
                }
                _ => continue,
            };
            let stage2_module_hash = match mm.get(&TermOrdKey(Term::symbol(":stage2-module-h"))) {
                Some(Term::Bytes(b)) if b.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(b.as_ref());
                    h
                }
                _ => module_hash,
            };
            let stage1_ok = matches!(
                mm.get(&TermOrdKey(Term::symbol(":stage1-ok"))),
                Some(Term::Bool(true))
            );
            let stage1_errors = match mm.get(&TermOrdKey(Term::symbol(":stage1-errors"))) {
                Some(Term::Vector(v)) => v
                    .iter()
                    .filter_map(|t| match t {
                        Term::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let supported = matches!(
                mm.get(&TermOrdKey(Term::symbol(":stage2-supported"))),
                Some(Term::Bool(true))
            );
            let ok = matches!(
                mm.get(&TermOrdKey(Term::symbol(":stage2-ok"))),
                Some(Term::Bool(true))
            );
            let errors = match mm.get(&TermOrdKey(Term::symbol(":stage2-errors"))) {
                Some(Term::Vector(v)) => v
                    .iter()
                    .filter_map(|t| match t {
                        Term::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let wasm_hash = match mm.get(&TermOrdKey(Term::symbol(":stage2-wasm-h"))) {
                Some(Term::Bytes(b)) if b.len() == 32 => {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(b.as_ref());
                    Some(h)
                }
                _ => None,
            };
            let wasm_bytes_len = match mm.get(&TermOrdKey(Term::symbol(":stage2-wasm-bytes"))) {
                Some(Term::Int(i)) => i.to_string().parse::<usize>().ok(),
                _ => None,
            };
            out.insert(
                path,
                Stage2Seed {
                    source,
                    forms,
                    module_hash,
                    stage2_module_hash,
                    stage1_ok,
                    stage1_errors,
                    supported,
                    ok,
                    errors,
                    wasm_hash,
                    wasm_bytes_len,
                },
            );
        }
        Some(Stage2SeedIndex {
            generated_by,
            modules: out,
        })
    }

    let frontend = resolved_coreform_frontend(cli)?;
    let (out_buf, min_stage2_supported_modules, min_stage2_validated_modules) =
        if frontend_is_rust(&frontend) {
            (
                out.to_path_buf(),
                min_stage2_supported_modules,
                min_stage2_validated_modules,
            )
        } else {
            let req = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":out")),
                        Term::Str(out.display().to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":min-stage2-supported-modules")),
                        Term::Int((min_stage2_supported_modules as i64).into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":min-stage2-validated-modules")),
                        Term::Int((min_stage2_validated_modules as i64).into()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let planned = selfhost_plan_request_map(
                cli,
                "core/cli::selfhost-artifact-request",
                req,
                "selfhost-artifact",
            )?;
            (
                PathBuf::from(planned_required_str(&planned, ":out", "selfhost-artifact")?),
                planned_required_u64(
                    &planned,
                    ":min-stage2-supported-modules",
                    "selfhost-artifact",
                )?,
                planned_required_u64(
                    &planned,
                    ":min-stage2-validated-modules",
                    "selfhost-artifact",
                )?,
            )
        };
    let out = out_buf.as_path();

    let bootstrap_mode = maybe_embedded_bootstrap_mode();
    let bootstrap_artifact = if bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly {
        resolved_explicit_selfhost_artifact(cli)
            .or_else(|| {
                if out.is_file() {
                    Some(out.to_path_buf())
                } else {
                    None
                }
            })
            .or_else(|| resolved_selfhost_artifact_for_frontend(cli))
    } else {
        None
    };
    if bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly && bootstrap_artifact.is_none() {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/bootstrap",
            "selfhost-artifact requires an existing toolchain artifact when embedded bootstrap is unavailable; pass --selfhost-artifact or set GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
        ));
    }

    let toolchain_sources = selfhost_coreform_toolchain_v1_sources()
        .map_err(|e| cli_err(EX_INTERNAL, "selfhost/sources", format!("{e}")))?;
    let stage2_seed_index = bootstrap_artifact
        .as_deref()
        .and_then(load_stage2_seed_index);
    let reuse_seed_results = stage2_seed_index.as_ref().is_some_and(|idx| {
        idx.generated_by.as_deref() == Some(&format!("genesis {}", env!("CARGO_PKG_VERSION")))
    });
    let full_seed_reuse = reuse_seed_results
        && stage2_seed_index.as_ref().is_some_and(|idx| {
            toolchain_sources.iter().all(|(path, src)| {
                idx.modules
                    .get(path)
                    .is_some_and(|seed| seed.source == *src)
            })
        });

    let mut stage2_seed_hits = 0u64;
    let mut stage2_computed = 0u64;

    let mut modules = Vec::new();
    let mut all_ok = true;
    let mut stage2_supported = 0u64;
    let mut stage2_validated = 0u64;
    let mut gate_errors: Vec<String> = Vec::new();

    if full_seed_reuse {
        let Some(seed_index) = stage2_seed_index.as_ref() else {
            return Err(cli_err(
                EX_INTERNAL,
                "selfhost/seed",
                "stage2 seed index unexpectedly missing for full-reuse path",
            ));
        };
        for (path, src) in &toolchain_sources {
            let Some(seed) = seed_index.modules.get(path) else {
                return Err(cli_err(
                    EX_INTERNAL,
                    "selfhost/seed",
                    format!("stage2 seed missing module entry for {}", path),
                ));
            };
            stage2_seed_hits = stage2_seed_hits.saturating_add(1);
            let stage2 = Stage2Summary {
                module_hash: seed.stage2_module_hash,
                supported: seed.supported,
                ok: seed.ok,
                errors: seed.errors.clone(),
                wasm_hash: seed.wasm_hash,
                wasm_bytes_len: seed.wasm_bytes_len,
            };
            if !seed.stage1_ok || (stage2.supported && !stage2.ok) {
                all_ok = false;
            }
            if stage2.supported {
                stage2_supported = stage2_supported.saturating_add(1);
                if stage2.ok {
                    stage2_validated = stage2_validated.saturating_add(1);
                }
            }
            modules.push(Term::Map(
                [
                    (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                    (TermOrdKey(Term::symbol(":source")), Term::Str(src.clone())),
                    (
                        TermOrdKey(Term::symbol(":forms")),
                        Term::Vector(seed.forms.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":module-h")),
                        Term::Bytes(seed.module_hash.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage1-ok")),
                        Term::Bool(seed.stage1_ok),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage1-errors")),
                        Term::Vector(seed.stage1_errors.iter().cloned().map(Term::Str).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-supported")),
                        Term::Bool(stage2.supported),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-ok")),
                        Term::Bool(stage2.ok),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-errors")),
                        Term::Vector(stage2.errors.iter().cloned().map(Term::Str).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-module-h")),
                        Term::Bytes(stage2.module_hash.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-wasm-h")),
                        stage2
                            .wasm_hash
                            .map(|h| Term::Bytes(h.to_vec().into()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-wasm-bytes")),
                        stage2
                            .wasm_bytes_len
                            .map(|n| Term::Int((n as i64).into()))
                            .unwrap_or(Term::Nil),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
        }
    } else {
        // Artifact rebuild uses trusted bundled sources; do not charge user step limits here.
        let step_limit = StepLimit::Unlimited;
        let mem_limits = resolved_mem_limits(cli);
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(mem_limits);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            bootstrap_mode,
            bootstrap_artifact.as_deref(),
        )
        .map_err(|e| cli_err(EX_PARSE, "selfhost/bootstrap", format!("{e}")))?;
        ctx.steps = 0;
        ctx.step_limit = step_limit.resolve();

        for (path, src) in &toolchain_sources {
            let seed = if reuse_seed_results {
                stage2_seed_index
                    .as_ref()
                    .and_then(|idx| idx.modules.get(path))
                    .filter(|s| s.source == *src)
                    .cloned()
            } else {
                None
            };

            let (forms, module_h, stage1_ok, stage1_errors, stage2) = if let Some(seed) = seed {
                stage2_seed_hits = stage2_seed_hits.saturating_add(1);
                (
                    seed.forms,
                    seed.module_hash,
                    seed.stage1_ok,
                    seed.stage1_errors,
                    Stage2Summary {
                        module_hash: seed.stage2_module_hash,
                        supported: seed.supported,
                        ok: seed.ok,
                        errors: seed.errors,
                        wasm_hash: seed.wasm_hash,
                        wasm_bytes_len: seed.wasm_bytes_len,
                    },
                )
            } else {
                let forms =
                    selfhost_parse_canonicalize_module(&mut ctx, &env, src).map_err(|e| {
                        cli_err(
                            e.exit_code,
                            "selfhost/canon",
                            format!("{path}: {}", e.json.message),
                        )
                    })?;
                let module_h = selfhost_hash_module_forms(&mut ctx, &env, &forms).map_err(|e| {
                    cli_err(
                        e.exit_code,
                        "selfhost/hash",
                        format!("{path}: {}", e.json.message),
                    )
                })?;
                let stage1_forms = selfhost_stage1_transform_module(&mut ctx, &env, &forms)
                    .map_err(|e| {
                        cli_err(
                            e.exit_code,
                            "selfhost/stage1",
                            format!("{path}: {}", e.json.message),
                        )
                    })?;
                let gate_report = gc_opt::stage1_validation_report(&forms, &stage1_forms);
                stage2_computed = stage2_computed.saturating_add(1);
                let report = gc_opt::stage2_validation_report(&stage1_forms);
                (
                    forms,
                    module_h,
                    gate_report.ok,
                    gate_report.errors,
                    Stage2Summary {
                        module_hash: report.module_hash,
                        supported: report.supported,
                        ok: report.ok,
                        errors: report.errors,
                        wasm_hash: report.wasm_hash,
                        wasm_bytes_len: report.wasm_bytes_len,
                    },
                )
            };

            if !stage1_ok || (stage2.supported && !stage2.ok) {
                all_ok = false;
            }
            if stage2.supported {
                stage2_supported = stage2_supported.saturating_add(1);
                if stage2.ok {
                    stage2_validated = stage2_validated.saturating_add(1);
                }
            }

            modules.push(Term::Map(
                [
                    (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                    (TermOrdKey(Term::symbol(":source")), Term::Str(src.clone())),
                    (
                        TermOrdKey(Term::symbol(":forms")),
                        Term::Vector(forms.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":module-h")),
                        Term::Bytes(module_h.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage1-ok")),
                        Term::Bool(stage1_ok),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage1-errors")),
                        Term::Vector(stage1_errors.into_iter().map(Term::Str).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-supported")),
                        Term::Bool(stage2.supported),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-ok")),
                        Term::Bool(stage2.ok),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-errors")),
                        Term::Vector(stage2.errors.iter().cloned().map(Term::Str).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-module-h")),
                        Term::Bytes(stage2.module_hash.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-wasm-h")),
                        stage2
                            .wasm_hash
                            .map(|h| Term::Bytes(h.to_vec().into()))
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":stage2-wasm-bytes")),
                        stage2
                            .wasm_bytes_len
                            .map(|n| Term::Int((n as i64).into()))
                            .unwrap_or(Term::Nil),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
        }
    }

    if stage2_supported < min_stage2_supported_modules {
        all_ok = false;
        gate_errors.push(format!(
            "stage2 supported modules {} is below required minimum {}",
            stage2_supported, min_stage2_supported_modules
        ));
    }
    if stage2_validated < min_stage2_validated_modules {
        all_ok = false;
        gate_errors.push(format!(
            "stage2 validated modules {} is below required minimum {}",
            stage2_validated, min_stage2_validated_modules
        ));
    }

    let artifact = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/selfhost-toolchain-artifact-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(all_ok)),
            (
                TermOrdKey(Term::symbol(":generated-by")),
                Term::Str(format!("genesis {}", env!("CARGO_PKG_VERSION"))),
            ),
            (
                TermOrdKey(Term::symbol(":stage2-summary")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":supported-modules")),
                            Term::Int((stage2_supported as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":validated-modules")),
                            Term::Int((stage2_validated as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":stage2-requirements")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":min-supported-modules")),
                            Term::Int((min_stage2_supported_modules as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":min-validated-modules")),
                            Term::Int((min_stage2_validated_modules as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":ok")),
                            Term::Bool(gate_errors.is_empty()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":errors")),
                            Term::Vector(gate_errors.iter().cloned().map(Term::Str).collect()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":modules")), Term::Vector(modules)),
        ]
        .into_iter()
        .collect(),
    );
    let artifact_s = print_term_compact(&artifact);
    std::fs::write(out, artifact_s.as_bytes())
        .with_context(|| format!("write {}", out.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let artifact_hash = *blake3::hash(artifact_s.as_bytes()).as_bytes();
    let exit_code = if all_ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: all_ok,
        kind: "genesis/selfhost-artifact-v0.2",
        data: Some(serde_json::json!({
            "out": out.display().to_string(),
            "ok": all_ok,
            "artifact_hash": hex32(artifact_hash),
            "stage2_supported_modules": stage2_supported,
            "stage2_validated_modules": stage2_validated,
            "min_stage2_supported_modules": min_stage2_supported_modules,
            "min_stage2_validated_modules": min_stage2_validated_modules,
            "stage2_requirements_ok": gate_errors.is_empty(),
            "stage2_requirement_errors": gate_errors,
            "stage2_cache_hits": stage2_seed_hits,
            "stage2_computed_modules": stage2_computed,
            "stage2_seed_artifact": bootstrap_artifact.as_ref().map(|p| p.display().to_string()),
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", out.display())
        },
        json: json_envelope_value(env)?,
    })
}
