use super::*;

#[path = "obligation_exec_budgets.rs"]
mod obligation_exec_budgets;
#[path = "obligation_exec_coverage.rs"]
mod obligation_exec_coverage;
#[path = "obligation_exec_coverage_finalize.rs"]
mod obligation_exec_coverage_finalize;
#[cfg(test)]
#[path = "obligation_exec_coverage_profile_tests.rs"]
mod obligation_exec_coverage_profile_tests;
#[path = "obligation_exec_replay.rs"]
mod obligation_exec_replay;
#[path = "obligation_exec_tests.rs"]
mod obligation_exec_tests;
#[cfg(test)]
pub(crate) use obligation_exec_coverage::mcdc_independence_for_site;
pub(crate) use obligation_exec_coverage::{
    CoverageAuthorityObservation, CoverageProfile, CoverageRunArgs, obligation_coverage,
};
pub(super) use obligation_exec_coverage_finalize::coverage_authority_observation;
pub(super) use obligation_exec_replay::{replay_observations, run_replay_authority};

pub(super) fn replay_effect_program(
    ctx: &mut EvalCtx,
    program: Value,
    log: &EffectLog,
    store: &gc_effects::ArtifactStore,
    frontend: &CoreformFrontend,
) -> Result<Value, ObligationError> {
    match frontend {
        CoreformFrontend::Selfhost(config) => {
            let program_hash = value_hash(&program);
            gc_effects::replay_with_selfhost_authority(
                ctx,
                program,
                log,
                Some(store),
                program_hash,
                config.bootstrap_mode,
                config.artifact.as_deref(),
            )
            .map_err(|error| ObligationError::Test(format!("replay failed: {error}")))
        }
        CoreformFrontend::Rust => {
            #[cfg(feature = "replay-parity-oracle")]
            {
                gc_effects::replay_with_store(ctx, program, log, Some(store))
                    .map_err(|error| ObligationError::Test(format!("replay failed: {error}")))
            }
            #[cfg(not(feature = "replay-parity-oracle"))]
            {
                let _ = (ctx, program, log, store);
                Err(ObligationError::Test(
                    "Rust replay oracle is disabled outside the parity harness".to_string(),
                ))
            }
        }
    }
}

pub(super) fn obligation_property_tests(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    obligation_exec_tests::obligation_property_tests(
        store, pkg_dir, manifest, modules, frontend, limits,
    )
}

pub(super) fn is_callable_value(v: &Value) -> bool {
    obligation_exec_tests::is_callable_value(v)
}

pub(super) fn parse_test_entry(v: &Value) -> Result<(Value, Option<Term>), ObligationError> {
    obligation_exec_tests::parse_test_entry(v)
}

pub(super) fn parse_property_entry(
    value: &Value,
    default_cases: u64,
) -> Result<(Value, u64), ObligationError> {
    obligation_exec_tests::parse_property_entry(value, default_cases)
}

pub(super) fn property_seed_for_case(pkg: &str, suite: &str, name: &str, index: u64) -> u64 {
    obligation_exec_tests::seed_for_case(pkg, suite, name, index)
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
    evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Determinism,
        store,
        manifest,
        modules,
        tests,
        frontend,
        limits,
    )
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
