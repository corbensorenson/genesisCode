use super::*;

pub(super) fn obligation_stage1_validation(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors = Vec::new();
    let mut module_reports = Vec::new();

    for m in modules {
        let out =
            gc_opt::stage1_pipeline(&m.forms).map_err(|e| ObligationError::Opt(format!("{e}")))?;
        let gate_report = out.gate_report;
        let optimizer_stats = out.optimize_report.stats;

        if !gate_report.ok {
            ok = false;
            for e in &gate_report.errors {
                errors.push(format!("{}: {e}", m.entry.path));
            }
        }
        module_reports.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(m.entry.path.clone()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(gate_report.ok)),
                (
                    TermOrdKey(Term::symbol(":original-module-h")),
                    Term::Bytes(gate_report.original_module_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":transformed-module-h")),
                    Term::Bytes(gate_report.transformed_module_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":original-value-h")),
                    gate_report
                        .original_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":transformed-value-h")),
                    gate_report
                        .transformed_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(gate_report.errors.iter().cloned().map(Term::Str).collect()),
                ),
                (
                    TermOrdKey(Term::symbol(":optimizer")),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":egg-runs")),
                                Term::Int((optimizer_stats.egg_runs as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":egg-iterations")),
                                Term::Int((optimizer_stats.iterations as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":egg-eclasses")),
                                Term::Int((optimizer_stats.eclasses as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":egg-enodes")),
                                Term::Int((optimizer_stats.enodes as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/stage1-validation-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":obligation")),
                Term::Str("core/obligation::stage1-validation".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_reports),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::stage1-validation".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_translation_validation(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    test_runs: &[TestRun],
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<ObligationResult, ObligationError> {
    // Conservative v0.2: we only validate optimization by re-running the *whole package*
    // tests against an optimized copy of each module and comparing per-test hashes.
    //
    // If there are no tests, treat as pass.
    if test_runs.is_empty() {
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/translation-validation-v0.2".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":note")),
                    Term::Str("no tests".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let artifact = store.put_term(&report)?;
        return Ok(ObligationResult {
            name: "core/obligation::translation-validation".to_string(),
            ok: true,
            artifact: Some(artifact),
            errors: Vec::new(),
        });
    }

    let mut ok = true;
    let mut errors = Vec::new();
    let mut per_test = Vec::new();
    let mut stage2_entries = Vec::new();
    let mut stage2_supported: u64 = 0;
    let mut stage2_validated: u64 = 0;
    let mut selfhost_ctx = None;
    let mut selfhost_env = None;
    if let CoreformFrontend::Selfhost(cfg) = frontend {
        // Toolchain bootstrap is trusted and therefore uncharged.
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(limits.mem_limits);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            cfg.bootstrap_mode,
            cfg.artifact.as_deref(),
        )
        .map_err(|e| ObligationError::Opt(format!("selfhost/init: {e}")))?;

        // Apply user/configured limits to optimization work.
        ctx.steps = 0;
        ctx.step_limit = limits.step_limit.resolve();
        selfhost_ctx = Some(ctx);
        selfhost_env = Some(env);
    }

    // Optimize modules once and record optimizer statistics as evidence.
    let mut opt_modules = Vec::new();
    let mut opt_stats = gc_opt::OptimizeStats::default();
    let mut mod_terms: Vec<Term> = Vec::new();
    for m in modules {
        let orig_h = hash_module(&m.forms);
        let (rust_opt_raw, rust_opt_report) = gc_opt::optimize_module_with_report(&m.forms);
        let rust_opt_forms = canonicalize_module(rust_opt_raw)
            .map_err(|e| ObligationError::Opt(format!("stage1 canonicalize: {e}")))?;
        let opt_forms = match frontend {
            CoreformFrontend::Rust => rust_opt_forms.clone(),
            CoreformFrontend::Selfhost(_) => {
                let ctx = selfhost_ctx.as_mut().expect("selfhost ctx initialized");
                let env = selfhost_env.as_ref().expect("selfhost env initialized");
                let selfhost_opt_raw = selfhost_optimize_module_forms(ctx, env, &m.forms)?;
                let selfhost_opt = canonicalize_module(selfhost_opt_raw).map_err(|e| {
                    ObligationError::Opt(format!("selfhost optimize canonicalize: {e}"))
                })?;
                if selfhost_opt != rust_opt_forms {
                    let rust_h = hash_module(&rust_opt_forms);
                    let selfhost_h = hash_module(&selfhost_opt);
                    return Err(ObligationError::Opt(format!(
                        "selfhost core/cli::optimize-module parity mismatch for {} (rust={} selfhost={})",
                        m.entry.path,
                        hex32(rust_h),
                        hex32(selfhost_h),
                    )));
                }
                selfhost_opt
            }
        };
        opt_stats.egg_runs = opt_stats
            .egg_runs
            .saturating_add(rust_opt_report.stats.egg_runs);
        opt_stats.iterations = opt_stats
            .iterations
            .saturating_add(rust_opt_report.stats.iterations);
        opt_stats.eclasses = opt_stats
            .eclasses
            .saturating_add(rust_opt_report.stats.eclasses);
        opt_stats.enodes = opt_stats
            .enodes
            .saturating_add(rust_opt_report.stats.enodes);
        for (k, v) in rust_opt_report.stats.rewrites_applied {
            *opt_stats.rewrites_applied.entry(k).or_insert(0) += v;
        }
        let opt_h = hash_module(&opt_forms);

        let s2 = gc_opt::stage2_validation_report(&opt_forms);
        if s2.supported {
            stage2_supported = stage2_supported.saturating_add(1);
            if s2.ok {
                stage2_validated = stage2_validated.saturating_add(1);
            } else {
                ok = false;
                for e in &s2.errors {
                    errors.push(format!("stage2 {}: {e}", m.entry.path));
                }
            }
        }
        stage2_entries.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(m.entry.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":supported")),
                    Term::Bool(s2.supported),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(s2.ok)),
                (
                    TermOrdKey(Term::symbol(":module-h")),
                    Term::Bytes(s2.module_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":wasm-h")),
                    s2.wasm_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":value-kind")),
                    match s2.value_kind {
                        Some(gc_opt::Stage2ValueKind::Int) => Term::Symbol(":int".to_string()),
                        Some(gc_opt::Stage2ValueKind::Bool) => Term::Symbol(":bool".to_string()),
                        Some(gc_opt::Stage2ValueKind::Nil) => Term::Symbol(":nil".to_string()),
                        Some(gc_opt::Stage2ValueKind::Sym) => Term::Symbol(":sym".to_string()),
                        Some(gc_opt::Stage2ValueKind::Str) => Term::Symbol(":str".to_string()),
                        Some(gc_opt::Stage2ValueKind::Bytes) => Term::Symbol(":bytes".to_string()),
                        None => Term::Nil,
                    },
                ),
                (
                    TermOrdKey(Term::symbol(":orig-value-h")),
                    s2.original_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":wasm-value-h")),
                    s2.wasm_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":wasm-bytes")),
                    s2.wasm_bytes_len
                        .map(|n| Term::Int((n as i64).into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(s2.errors.iter().cloned().map(Term::Str).collect()),
                ),
            ]
            .into_iter()
            .collect(),
        ));

        mod_terms.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(m.entry.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":orig-h")),
                    Term::Bytes(orig_h.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":opt-h")),
                    Term::Bytes(opt_h.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":changed")),
                    Term::Bool(orig_h != opt_h),
                ),
            ]
            .into_iter()
            .collect(),
        ));
        let opt_meta = extract_meta_static(&opt_forms);
        opt_modules.push(LoadedModule {
            entry: m.entry.clone(),
            abs_path: m.abs_path.clone(),
            hash: opt_h,
            forms: opt_forms,
            meta: opt_meta,
        });
    }

    for tr in test_runs {
        if !tr.ok {
            ok = false;
            errors.push(format!(
                "original test failed for {}::{}",
                tr.id.suite_sym, tr.id.test_name
            ));
        }

        let opt = run_one_test(pkg_dir, manifest, &opt_modules, caps, tr.id.clone(), limits)?;

        if tr.value_hash != opt.value_hash {
            ok = false;
            errors.push(format!(
                "hash mismatch for {}::{}",
                tr.id.suite_sym, tr.id.test_name
            ));
        }
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(tr.id.suite_sym.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(tr.id.test_name.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":orig-h")),
            Term::Bytes(tr.value_hash.to_vec().into()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":opt-h")),
            Term::Bytes(opt.value_hash.to_vec().into()),
        );
        per_test.push(Term::Map(m));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/translation-validation-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":optimizer")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":egg-runs")),
                            Term::Int((opt_stats.egg_runs as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-iterations")),
                            Term::Int((opt_stats.iterations as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-eclasses")),
                            Term::Int((opt_stats.eclasses as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-enodes")),
                            Term::Int((opt_stats.enodes as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-rewrites")),
                            Term::Vector(
                                opt_stats
                                    .rewrites_applied
                                    .iter()
                                    .map(|(k, v)| {
                                        Term::Map(
                                            [
                                                (
                                                    TermOrdKey(Term::symbol(":name")),
                                                    Term::Str(k.clone()),
                                                ),
                                                (
                                                    TermOrdKey(Term::symbol(":n")),
                                                    Term::Int((*v as i64).into()),
                                                ),
                                            ]
                                            .into_iter()
                                            .collect(),
                                        )
                                    })
                                    .collect(),
                            ),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(mod_terms),
            ),
            (
                TermOrdKey(Term::symbol(":stage2")),
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
                            TermOrdKey(Term::symbol(":entries")),
                            Term::Vector(stage2_entries),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(per_test)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::translation-validation".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) struct PackageEval {
    modules: Vec<ModuleEval>,
    pub(super) exports_env: Env,
    // A fast lookup map for "internal" names: suite/test symbol -> module env
    internal_index: BTreeMap<String, usize>,
}

impl PackageEval {
    pub(super) fn from_modules(
        base_env: Env,
        modules: Vec<ModuleEval>,
    ) -> Result<Self, ObligationError> {
        let mut exports = BTreeMap::new();
        let mut internal_index = BTreeMap::new();
        for (i, m) in modules.iter().enumerate() {
            for name in m.defined.keys() {
                internal_index.entry(name.clone()).or_insert(i);
            }
            for e in &m.exports {
                let v = m.defined.get(e).ok_or_else(|| {
                    ObligationError::Module(format!(
                        "module {} exports {} but does not define it",
                        m.path.display(),
                        e
                    ))
                })?;
                exports.insert(e.clone(), v.clone());
            }
        }
        let exports_env = Env::with_bindings(&base_env, exports);
        Ok(Self {
            modules,
            exports_env,
            internal_index,
        })
    }

    pub(super) fn lookup_any(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.exports_env.get(name) {
            return Some(v);
        }
        let idx = self.internal_index.get(name)?;
        self.modules[*idx].env.get(name)
    }
}
