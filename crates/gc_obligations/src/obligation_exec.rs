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
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    obligation_exec_budgets::obligation_budgets(store, manifest, tests, frontend, limits)
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
            .apply(&mut ctx, Value::data(arg.clone()))
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
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_obligation_with_authority(
        ObligationAuthorityOperation::UnitTests,
        store,
        manifest,
        &[],
        tests,
        frontend,
        limits,
    )
}

pub(super) fn obligation_determinism(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    // Rule: if a module declares :caps = [], then its inferred effect ops must be empty,
    // and any tests defined by that module must not perform effects.
    let mut errors = Vec::new();
    let mut ok = true;

    let typecheck = typecheck_report_with_frontend(modules, frontend, limits, false)?;
    // The bound authority decoder guarantees this report is in exact module order.
    for (m, inferred) in modules.iter().zip(&typecheck.modules) {
        let meta = extract_meta_static(&m.forms);
        if let Some(meta) = meta
            && let Some(caps) = meta_caps(&meta)
            && caps.is_empty()
            && (inferred.unknown_ops || !inferred.inferred_ops.is_empty())
        {
            ok = false;
            errors.push(format!(
                "{} declares :caps [] but has inferred effects (unknown={}, ops={:?})",
                m.entry.path, inferred.unknown_ops, inferred.inferred_ops
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
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_obligation_with_authority(
        ObligationAuthorityOperation::CapabilitiesDeclared,
        store,
        manifest,
        modules,
        tests,
        frontend,
        limits,
    )
}

pub(super) fn obligation_typecheck(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
    strict_sound: bool,
) -> Result<ObligationResult, ObligationError> {
    evaluate_obligation_with_authority(
        if strict_sound {
            ObligationAuthorityOperation::TypecheckStrict
        } else {
            ObligationAuthorityOperation::Typecheck
        },
        store,
        manifest,
        modules,
        &[],
        frontend,
        limits,
    )
}

pub(super) fn typecheck_report_with_frontend(
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
    strict_sound: bool,
) -> Result<AuthoritativeTypecheckReport, ObligationError> {
    let mut mods = Vec::new();
    for m in modules {
        let meta = if strict_sound {
            strict_sound_meta(m.meta.as_ref())
        } else {
            m.meta.clone()
        };
        mods.push(TypecheckModuleInput {
            path: m.entry.path.clone(),
            forms: m.forms.clone(),
            meta,
        });
    }
    typecheck_modules_with_authority(&mods, frontend, limits.step_limit, limits.mem_limits)
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
