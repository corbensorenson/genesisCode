use super::*;

#[path = "obligation_exec_budgets.rs"]
mod obligation_exec_budgets;
#[path = "obligation_exec_coverage.rs"]
mod obligation_exec_coverage;
#[cfg(test)]
#[path = "obligation_exec_coverage_profile_tests.rs"]
mod obligation_exec_coverage_profile_tests;
#[path = "obligation_exec_replay.rs"]
mod obligation_exec_replay;
#[path = "obligation_exec_tests.rs"]
mod obligation_exec_tests;
#[cfg(test)]
pub(crate) use obligation_exec_coverage::mcdc_independence_for_site;
pub(crate) use obligation_exec_coverage::{CoverageProfile, CoverageRunArgs, obligation_coverage};

pub(super) fn obligation_property_tests(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    obligation_exec_tests::obligation_property_tests(store, pkg_dir, manifest, modules, limits)
}

pub(super) fn is_callable_value(v: &Value) -> bool {
    obligation_exec_tests::is_callable_value(v)
}

pub(super) fn parse_test_entry(v: &Value) -> Result<(Value, Option<Term>), ObligationError> {
    obligation_exec_tests::parse_test_entry(v)
}

pub(super) fn obligation_replayable(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    obligation_exec_replay::obligation_replayable(store, pkg_dir, manifest, modules, tests, limits)
}

pub(super) fn obligation_concurrency_replay(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    obligation_exec_replay::obligation_concurrency_replay(
        store, pkg_dir, manifest, modules, tests, limits,
    )
}

pub(super) fn obligation_budgets(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    obligation_exec_budgets::obligation_budgets(store, manifest, tests)
}

fn obligation_report_term(contract: &str, args: &[Term]) -> Result<Term, ObligationError> {
    let mut ctx = EvalCtx::with_step_limit(StepLimit::Default.resolve());
    ctx.set_mem_limits(MemLimits::default());
    let prelude = build_prelude(&mut ctx);
    let mut f = prelude
        .env
        .get(contract)
        .ok_or_else(|| ObligationError::Module(format!("missing prelude binding {contract}")))?;
    for arg in args {
        f = f
            .apply(&mut ctx, Value::Data(arg.clone()))
            .map_err(|e| ObligationError::Test(format!("{contract} apply failed: {e}")))?;
    }
    let out = f.to_term_for_log(ctx.protocol.map(|p| p.error));
    match out {
        Term::Map(_) => Ok(out),
        other => Err(ObligationError::Test(format!(
            "{contract} returned non-map report: {}",
            print_term(&other)
        ))),
    }
}

fn term_map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, key: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn term_vec_strings(t: &Term, field: &str) -> Result<Vec<String>, ObligationError> {
    let Term::Vector(xs) = t else {
        return Err(ObligationError::Test(format!(
            "core/obligation::plan returned non-vector {field}"
        )));
    };
    let mut out = Vec::with_capacity(xs.len());
    for x in xs {
        let Term::Str(s) = x else {
            return Err(ObligationError::Test(format!(
                "core/obligation::plan returned non-string in {field}"
            )));
        };
        out.push(s.clone());
    }
    Ok(out)
}

pub(super) fn obligation_plan_symbols(
    obligations: &[String],
) -> Result<Vec<String>, ObligationError> {
    let report = obligation_report_term(
        "core/obligation::plan",
        &[Term::Vector(
            obligations
                .iter()
                .cloned()
                .map(Term::Str)
                .collect::<Vec<_>>(),
        )],
    )?;
    let Term::Map(report_map) = report else {
        return Err(ObligationError::Test(
            "core/obligation::plan returned non-map report".to_string(),
        ));
    };

    let rejected = match term_map_get(&report_map, ":rejected") {
        Some(t) => term_vec_strings(t, ":rejected")?,
        None => {
            return Err(ObligationError::Test(
                "core/obligation::plan report missing :rejected".to_string(),
            ));
        }
    };
    if !rejected.is_empty() {
        return Err(ObligationError::Test(format!(
            "core/obligation::plan rejected obligation entries: {}",
            rejected.join(", ")
        )));
    }

    match term_map_get(&report_map, ":run") {
        Some(t) => term_vec_strings(t, ":run"),
        None => Err(ObligationError::Test(
            "core/obligation::plan report missing :run".to_string(),
        )),
    }
}

pub(super) fn obligation_acceptance_ok(
    results: &[ObligationResult],
) -> Result<bool, ObligationError> {
    let result_terms = results
        .iter()
        .map(|r| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":name")), Term::Str(r.name.clone())),
                    (TermOrdKey(Term::symbol(":ok")), Term::Bool(r.ok)),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect::<Vec<_>>();
    let report = obligation_report_term(
        "core/obligation::acceptance-ok",
        &[Term::Vector(result_terms)],
    )?;
    term_map_get_bool(&report, ":ok").ok_or_else(|| {
        ObligationError::Test(
            "core/obligation::acceptance-ok returned report missing :ok bool".to_string(),
        )
    })
}

pub(super) fn obligation_unit_tests(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    let mut test_terms = Vec::new();
    for t in tests {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(t.id.suite_sym.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(t.id.test_name.clone()),
        );
        m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(t.ok));
        m.insert(
            TermOrdKey(Term::symbol(":value-h")),
            Term::Bytes(t.value_hash.to_vec().into()),
        );
        if let Some(e) = &t.error {
            m.insert(TermOrdKey(Term::symbol(":error")), Term::Str(e.clone()));
        }
        if let Some(log) = &t.effect_log {
            let log_h = store.put_term(&log.to_term())?;
            m.insert(TermOrdKey(Term::symbol(":log-artifact")), Term::Str(log_h));
        }
        test_terms.push(Term::Map(m));
    }
    let report = obligation_report_term(
        "core/obligation::unit-tests-report",
        &[Term::Str(manifest.name.clone()), Term::Vector(test_terms)],
    )?;
    let ok = term_map_get_bool(&report, ":ok").ok_or_else(|| {
        ObligationError::Test(
            "core/obligation::unit-tests-report returned report missing :ok bool".to_string(),
        )
    })?;
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::unit-tests".to_string(),
        ok,
        artifact: Some(artifact),
        errors: Vec::new(),
    })
}

pub(super) fn obligation_determinism(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    // Rule: if a module declares :caps = [], then its inferred effect ops must be empty,
    // and any tests defined by that module must not perform effects.
    let mut errors = Vec::new();
    let mut ok = true;

    // Static scan.
    for m in modules {
        let inf = gc_types::infer_effects(&m.forms);
        let meta = extract_meta_static(&m.forms);
        if let Some(meta) = meta
            && let Some(caps) = meta_caps(&meta)
            && caps.is_empty()
            && (inf.unknown || !inf.ops.is_empty())
        {
            ok = false;
            errors.push(format!(
                "{} declares :caps [] but has inferred effects (unknown={}, ops={:?})",
                m.entry.path, inf.unknown, inf.ops
            ));
        }
    }

    // Runtime check: any effectful test for a module with :caps [] fails.
    // We approximate by mapping suite symbol -> module (static def scan).
    let suite_to_mod = suite_to_module(modules);
    for t in tests {
        if let Some(mod_i) = suite_to_mod.get(&t.id.suite_sym)
            && let Some(meta) = extract_meta_static(&modules[*mod_i].forms)
            && let Some(caps) = meta_caps(&meta)
        {
            let observed_effects = t.effect_log.as_ref().is_some_and(|l| !l.entries.is_empty());
            if caps.is_empty() && observed_effects {
                ok = false;
                errors.push(format!(
                    "test {} in {} performed effects but module declares :caps []",
                    t.id.test_name, t.id.suite_sym
                ));
            }
        }
    }

    let report = obligation_report_term(
        "core/obligation::determinism-report",
        &[
            Term::Str(manifest.name.clone()),
            Term::Bool(ok),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        ],
    )?;
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::determinism".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_caps_declared(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors = Vec::new();
    let suite_to_mod = suite_to_module(modules);

    for t in tests {
        let Some(log) = &t.effect_log else { continue };
        let used: BTreeSet<String> = log.entries.iter().map(|e| e.op.clone()).collect();
        let Some(mod_i) = suite_to_mod.get(&t.id.suite_sym) else {
            ok = false;
            errors.push(format!(
                "cannot find defining module for suite {}",
                t.id.suite_sym
            ));
            continue;
        };
        let meta = extract_meta_static(&modules[*mod_i].forms).ok_or_else(|| {
            ObligationError::Test(format!(
                "module {} missing ::meta for caps check",
                modules[*mod_i].entry.path
            ))
        })?;
        let declared = meta_caps(&meta).ok_or_else(|| {
            ObligationError::Test(format!(
                "module {} ::meta missing :caps",
                modules[*mod_i].entry.path
            ))
        })?;
        let declared: BTreeSet<String> = declared.into_iter().collect();
        for op in used {
            if !declared.contains(&op) {
                ok = false;
                errors.push(format!(
                    "test {} used op {} but module {} did not declare it in :caps",
                    t.id.test_name, op, modules[*mod_i].entry.path
                ));
            }
        }
    }

    let report = obligation_report_term(
        "core/obligation::capabilities-declared-report",
        &[
            Term::Str(manifest.name.clone()),
            Term::Bool(ok),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        ],
    )?;
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::capabilities-declared".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_typecheck(
    store: &EvidenceStore,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
    strict_sound: bool,
) -> Result<ObligationResult, ObligationError> {
    let report = typecheck_report_with_frontend(modules, frontend, limits, strict_sound)?;
    let ok = report.ok;
    let artifact = store.put_term(&report.to_term())?;
    Ok(ObligationResult {
        name: if strict_sound {
            "core/obligation::typecheck-strict".to_string()
        } else {
            "core/obligation::typecheck".to_string()
        },
        ok,
        artifact: Some(artifact),
        errors: report.errors,
    })
}

pub(super) fn typecheck_report_with_frontend(
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
    strict_sound: bool,
) -> Result<gc_types::TypecheckReport, ObligationError> {
    let mut mods = Vec::new();
    for m in modules {
        let meta = if strict_sound {
            strict_sound_meta(m.meta.as_ref())
        } else {
            m.meta.clone()
        };
        mods.push(gc_types::ModuleForTypecheck {
            path: m.entry.path.clone(),
            forms: m.forms.clone(),
            meta,
        });
    }
    let report = gc_types::typecheck_package(&mods);
    verify_selfhost_infer_effects_parity(modules, frontend, limits)?;
    Ok(report)
}

fn strict_sound_meta(meta: Option<&Term>) -> Option<Term> {
    let mut map = match meta {
        Some(Term::Map(m)) => m.clone(),
        _ => BTreeMap::new(),
    };
    map.insert(
        TermOrdKey(Term::symbol(":strict-effects")),
        Term::Bool(true),
    );
    map.insert(TermOrdKey(Term::symbol(":strict-shapes")), Term::Bool(true));
    Some(Term::Map(map))
}

pub(super) fn verify_selfhost_infer_effects_parity(
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<(), ObligationError> {
    let CoreformFrontend::Selfhost(cfg) = frontend else {
        return Ok(());
    };

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
    .map_err(|e| ObligationError::Typecheck(format!("selfhost/init: {e}")))?;
    // Apply user/configured limits to inference work.
    ctx.steps = 0;
    ctx.step_limit = limits.step_limit.resolve();

    for m in modules {
        let rust = gc_types::infer_effects(&m.forms);
        let selfhost = selfhost_infer_effects_forms(&mut ctx, &env, &m.forms)?;
        if rust.ops != selfhost.ops || rust.unknown != selfhost.unknown {
            let rust_ops = rust.ops.into_iter().collect::<Vec<_>>().join(",");
            let self_ops = selfhost.ops.into_iter().collect::<Vec<_>>().join(",");
            return Err(ObligationError::Typecheck(format!(
                "selfhost core/cli::infer-effects parity mismatch for {} (rust_ops=[{}] rust_unknown={} selfhost_ops=[{}] selfhost_unknown={})",
                m.entry.path, rust_ops, rust.unknown, self_ops, selfhost.unknown
            )));
        }
    }
    Ok(())
}
