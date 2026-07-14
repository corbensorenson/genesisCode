use super::cmd_selfhost_helpers::{
    extract_manifest_module_paths, maybe_update_selfhost_freshness_metadata,
};
use super::*;

pub(super) fn cmd_selfhost_artifact(
    cli: &Cli,
    out: &Path,
    min_stage2_supported_modules: u64,
    min_stage2_validated_modules: u64,
    recover_missing_artifact: bool,
) -> Result<CmdOut, CliError> {
    #[derive(Debug, Clone)]
    struct Stage2Seed {
        source: String,
        forms: Vec<Term>,
        module_hash: [u8; 32],
        stage2_module_hash: [u8; 32],
        lowering_mode: Option<gc_opt::Stage2LoweringMode>,
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
        lowering_mode: Option<gc_opt::Stage2LoweringMode>,
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
        stage2_supported_modules: Option<u64>,
        stage2_validated_modules: Option<u64>,
        stage2_strict_modules: Option<u64>,
        stage2_constant_fallback_modules: Option<u64>,
    }

    fn parse_stage2_lowering_mode(t: &Term) -> Option<gc_opt::Stage2LoweringMode> {
        match t {
            Term::Str(s) if s == "strict" => Some(gc_opt::Stage2LoweringMode::Strict),
            Term::Str(s) if s == "constant-fallback" => {
                Some(gc_opt::Stage2LoweringMode::ConstantFallback)
            }
            _ => None,
        }
    }

    fn stage2_lowering_mode_term(mode: Option<gc_opt::Stage2LoweringMode>) -> Term {
        match mode {
            Some(gc_opt::Stage2LoweringMode::Strict) => Term::Str("strict".to_string()),
            Some(gc_opt::Stage2LoweringMode::ConstantFallback) => {
                Term::Str("constant-fallback".to_string())
            }
            None => Term::Nil,
        }
    }

    fn normalize_stage2_lowering_mode(
        mode: Option<gc_opt::Stage2LoweringMode>,
        supported: bool,
        ok: bool,
    ) -> Option<gc_opt::Stage2LoweringMode> {
        if mode.is_some() {
            return mode;
        }
        if supported && ok {
            // Legacy seed artifacts predate lowering-mode accounting.
            // For deterministic accounting, treat successful supported modules as strict by default.
            return Some(gc_opt::Stage2LoweringMode::Strict);
        }
        None
    }

    fn load_stage2_seed_index(path: &Path) -> Option<Stage2SeedIndex> {
        let src = std::fs::read_to_string(path).ok()?;
        let term = parse_term(&src).ok()?;
        let Term::Map(root) = term else { return None };
        match root.get(&TermOrdKey(Term::symbol(":kind"))) {
            Some(Term::Str(s)) if s == gc_prelude::SELFHOST_TOOLCHAIN_ARTIFACT_KIND => {}
            _ => return None,
        }
        match root.get(&TermOrdKey(Term::symbol(":v"))) {
            Some(Term::Int(v)) if v == &gc_prelude::SELFHOST_TOOLCHAIN_ARTIFACT_VERSION.into() => {}
            _ => return None,
        }
        let generated_by = match root.get(&TermOrdKey(Term::symbol(":generated-by"))) {
            Some(Term::Str(s)) => Some(s.clone()),
            _ => None,
        };
        let parse_u64 =
            |m: &std::collections::BTreeMap<TermOrdKey, Term>, key: &str| -> Option<u64> {
                match m.get(&TermOrdKey(Term::symbol(key))) {
                    Some(Term::Int(i)) => i.to_string().parse::<u64>().ok(),
                    _ => None,
                }
            };
        let (
            stage2_supported_modules,
            stage2_validated_modules,
            stage2_strict_modules,
            stage2_constant_fallback_modules,
        ) = match root.get(&TermOrdKey(Term::symbol(":stage2-summary"))) {
            Some(Term::Map(summary)) => (
                parse_u64(summary, ":supported-modules"),
                parse_u64(summary, ":validated-modules"),
                parse_u64(summary, ":strict-modules"),
                parse_u64(summary, ":constant-fallback-modules"),
            ),
            _ => (None, None, None, None),
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
            let lowering_mode = mm
                .get(&TermOrdKey(Term::symbol(":stage2-lowering-mode")))
                .and_then(parse_stage2_lowering_mode);
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
                    lowering_mode,
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
            stage2_supported_modules,
            stage2_validated_modules,
            stage2_strict_modules,
            stage2_constant_fallback_modules,
        })
    }

    fn load_manifest_toolchain_sources_for_recovery() -> Result<Vec<(String, String)>, CliError> {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest_path = workspace_root.join("selfhost/toolchain_manifest.gc");
        let manifest_src = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))
            .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
        let module_paths = extract_manifest_module_paths(&manifest_src);
        if module_paths.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "selfhost/recovery",
                format!(
                    "manifest did not declare recoverable selfhost module paths: {}",
                    manifest_path.display()
                ),
            ));
        }
        let mut out = Vec::with_capacity(module_paths.len());
        for rel in module_paths {
            let path = workspace_root.join(&rel);
            let src = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))
                .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
            out.push((rel, src));
        }
        Ok(out)
    }

    let (out_buf, requested_min_stage2_supported_modules, requested_min_stage2_validated_modules) =
        if recover_missing_artifact {
            (
                out.to_path_buf(),
                min_stage2_supported_modules,
                min_stage2_validated_modules,
            )
        } else {
            let frontend = resolved_coreform_frontend(cli)?;
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
            }
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
    let force_manifest_recovery =
        bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly && recover_missing_artifact;
    let stage2_seed_index = if force_manifest_recovery {
        None
    } else {
        bootstrap_artifact
            .as_deref()
            .and_then(load_stage2_seed_index)
    };
    let use_manifest_recovery = force_manifest_recovery
        || (bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly
            && recover_missing_artifact
            && (bootstrap_artifact.is_none() || stage2_seed_index.is_none()));
    if bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly
        && bootstrap_artifact.is_none()
        && !use_manifest_recovery
    {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/bootstrap",
            "selfhost-artifact requires an existing toolchain artifact when embedded bootstrap is unavailable; pass --selfhost-artifact or set GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT (or use --recover-missing-artifact to rebuild from manifest sources)",
        ));
    }

    let toolchain_sources = if use_manifest_recovery {
        load_manifest_toolchain_sources_for_recovery()?
    } else {
        selfhost_coreform_toolchain_v1_sources()
            .map_err(|e| cli_err(EX_INTERNAL, "selfhost/sources", format!("{e}")))?
    };
    let fallback_stage2_floor = (toolchain_sources.len() as u64).max(1);
    let seed_policy_floor = stage2_seed_index.as_ref().map(|idx| {
        let _seed_mode_counts = (
            idx.stage2_strict_modules.unwrap_or(0),
            idx.stage2_constant_fallback_modules.unwrap_or(0),
        );
        let mut supported = idx.stage2_supported_modules.unwrap_or(0);
        let mut validated = idx.stage2_validated_modules.unwrap_or(0);
        if supported == 0 || validated == 0 {
            let mut counted_supported = 0u64;
            let mut counted_validated = 0u64;
            for seed in idx.modules.values() {
                if seed.supported {
                    counted_supported = counted_supported.saturating_add(1);
                    if seed.ok {
                        counted_validated = counted_validated.saturating_add(1);
                    }
                }
            }
            if supported == 0 {
                supported = counted_supported;
            }
            if validated == 0 {
                validated = counted_validated;
            }
        }
        (
            supported.max(fallback_stage2_floor),
            validated.max(fallback_stage2_floor),
        )
    });
    let policy_min_stage2_supported_modules = seed_policy_floor
        .map(|(supported, _)| supported)
        .unwrap_or(fallback_stage2_floor);
    let policy_min_stage2_validated_modules = seed_policy_floor
        .map(|(_, validated)| validated)
        .unwrap_or(fallback_stage2_floor);
    let min_stage2_supported_modules = if requested_min_stage2_supported_modules > 0 {
        requested_min_stage2_supported_modules
    } else {
        policy_min_stage2_supported_modules
    };
    let min_stage2_validated_modules = if requested_min_stage2_validated_modules > 0 {
        requested_min_stage2_validated_modules
    } else {
        policy_min_stage2_validated_modules
    };
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
    let mut stage2_strict_modules = 0u64;
    let mut stage2_constant_fallback_modules = 0u64;
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
                lowering_mode: normalize_stage2_lowering_mode(
                    seed.lowering_mode,
                    seed.supported,
                    seed.ok,
                ),
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
                match stage2.lowering_mode {
                    Some(gc_opt::Stage2LoweringMode::Strict) => {
                        stage2_strict_modules = stage2_strict_modules.saturating_add(1);
                    }
                    Some(gc_opt::Stage2LoweringMode::ConstantFallback) => {
                        stage2_constant_fallback_modules =
                            stage2_constant_fallback_modules.saturating_add(1);
                    }
                    None => {}
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
                        TermOrdKey(Term::symbol(":stage2-lowering-mode")),
                        stage2_lowering_mode_term(stage2.lowering_mode),
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
        if !use_manifest_recovery {
            load_selfhost_coreform_toolchain_v1_with_mode(
                &mut ctx,
                &mut env,
                bootstrap_mode,
                bootstrap_artifact.as_deref(),
            )
            .map_err(|e| cli_err(EX_PARSE, "selfhost/bootstrap", format!("{e}")))?;
            ctx.steps = 0;
            ctx.step_limit = step_limit.resolve();
        }

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
                        lowering_mode: normalize_stage2_lowering_mode(
                            seed.lowering_mode,
                            seed.supported,
                            seed.ok,
                        ),
                        supported: seed.supported,
                        ok: seed.ok,
                        errors: seed.errors,
                        wasm_hash: seed.wasm_hash,
                        wasm_bytes_len: seed.wasm_bytes_len,
                    },
                )
            } else if use_manifest_recovery {
                let forms =
                    canonicalize_module(parse_module(src).map_err(|e| {
                        cli_err(EX_PARSE, "selfhost/parse", format!("{path}: {e}"))
                    })?)
                    .map_err(|e| cli_err(EX_PARSE, "selfhost/canon", format!("{path}: {e}")))?;
                let module_h = hash_module(&forms);
                let stage1 = gc_opt::stage1_pipeline(&forms)
                    .map_err(|e| cli_err(EX_INTERNAL, "selfhost/stage1", format!("{path}: {e}")))?;
                stage2_computed = stage2_computed.saturating_add(1);
                let report = gc_opt::stage2_validation_report(&stage1.transformed_forms);
                (
                    forms,
                    module_h,
                    stage1.gate_report.ok,
                    stage1.gate_report.errors,
                    Stage2Summary {
                        module_hash: report.module_hash,
                        lowering_mode: normalize_stage2_lowering_mode(
                            report.lowering_mode,
                            report.supported,
                            report.ok,
                        ),
                        supported: report.supported,
                        ok: report.ok,
                        errors: report.errors,
                        wasm_hash: report.wasm_hash,
                        wasm_bytes_len: report.wasm_bytes_len,
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
                        lowering_mode: normalize_stage2_lowering_mode(
                            report.lowering_mode,
                            report.supported,
                            report.ok,
                        ),
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
                match stage2.lowering_mode {
                    Some(gc_opt::Stage2LoweringMode::Strict) => {
                        stage2_strict_modules = stage2_strict_modules.saturating_add(1);
                    }
                    Some(gc_opt::Stage2LoweringMode::ConstantFallback) => {
                        stage2_constant_fallback_modules =
                            stage2_constant_fallback_modules.saturating_add(1);
                    }
                    None => {}
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
                        TermOrdKey(Term::symbol(":stage2-lowering-mode")),
                        stage2_lowering_mode_term(stage2.lowering_mode),
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
                Term::Str(gc_prelude::SELFHOST_TOOLCHAIN_ARTIFACT_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":v")),
                Term::Int(gc_prelude::SELFHOST_TOOLCHAIN_ARTIFACT_VERSION.into()),
            ),
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
                        (
                            TermOrdKey(Term::symbol(":strict-modules")),
                            Term::Int((stage2_strict_modules as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":constant-fallback-modules")),
                            Term::Int((stage2_constant_fallback_modules as i64).into()),
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
    maybe_update_selfhost_freshness_metadata(out, artifact_s.as_bytes())?;

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
            "stage2_strict_modules": stage2_strict_modules,
            "stage2_constant_fallback_modules": stage2_constant_fallback_modules,
            "min_stage2_supported_modules": min_stage2_supported_modules,
            "min_stage2_validated_modules": min_stage2_validated_modules,
            "requested_min_stage2_supported_modules": requested_min_stage2_supported_modules,
            "requested_min_stage2_validated_modules": requested_min_stage2_validated_modules,
            "policy_min_stage2_supported_modules": policy_min_stage2_supported_modules,
            "policy_min_stage2_validated_modules": policy_min_stage2_validated_modules,
            "stage2_requirements_ok": gate_errors.is_empty(),
            "stage2_requirement_errors": gate_errors,
            "stage2_cache_hits": stage2_seed_hits,
            "stage2_computed_modules": stage2_computed,
            "stage2_seed_artifact": bootstrap_artifact.as_ref().map(|p| p.display().to_string()),
            "bootstrap_recovery_used": use_manifest_recovery,
            "bootstrap_recovery_mode": if use_manifest_recovery {
                Some("manifest-sources-rust-canonical-v0.1")
            } else {
                None::<&str>
            },
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
