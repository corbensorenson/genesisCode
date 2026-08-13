use super::*;
use crate::obligation_authority::evaluate_coverage_obligation_with_authority;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoverageProfile {
    Symbol,
    Decision,
    Mcdc,
}

pub(crate) struct CoverageRunArgs<'a> {
    pub store: &'a EvidenceStore,
    pub pkg_dir: &'a Path,
    pub manifest: &'a PackageManifest,
    pub modules: &'a [LoadedModule],
    pub tests: &'a [TestRun],
    pub limits: KernelLimits,
    pub frontend: &'a CoreformFrontend,
    pub profile: CoverageProfile,
    pub obligation_name: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CoverageAuthorityObservation {
    pub(crate) inputs: Term,
    pub(crate) expected_ok: bool,
    pub(crate) expected_errors: Vec<String>,
    pub(crate) expected_report: Term,
}

impl CoverageProfile {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::Symbol => "symbol",
            Self::Decision => "decision",
            Self::Mcdc => "mcdc",
        }
    }

    pub(super) fn requires_structural_gates(self) -> bool {
        matches!(self, Self::Decision | Self::Mcdc)
    }

    pub(super) fn requires_mcdc(self) -> bool {
        matches!(self, Self::Mcdc)
    }
}

fn sample_has_all_conditions(sample: &DecisionSample, conditions: &BTreeSet<String>) -> bool {
    conditions.iter().all(|c| sample.conditions.contains_key(c))
}

pub(crate) fn mcdc_independence_for_site(
    samples: &[DecisionSample],
    conditions: &BTreeSet<String>,
) -> BTreeMap<String, bool> {
    let mut out: BTreeMap<String, bool> = BTreeMap::new();
    for cond in conditions {
        let mut independent = false;
        for i in 0..samples.len() {
            if independent {
                break;
            }
            let a = &samples[i];
            if !sample_has_all_conditions(a, conditions) {
                continue;
            }
            for b in samples.iter().skip(i + 1) {
                if !sample_has_all_conditions(b, conditions) {
                    continue;
                }
                let Some(av) = a.conditions.get(cond) else {
                    continue;
                };
                let Some(bv) = b.conditions.get(cond) else {
                    continue;
                };
                if av == bv || a.outcome == b.outcome {
                    continue;
                }
                let mut others_equal = true;
                for other in conditions {
                    if other == cond {
                        continue;
                    }
                    if a.conditions.get(other) != b.conditions.get(other) {
                        others_equal = false;
                        break;
                    }
                }
                if others_equal {
                    independent = true;
                    break;
                }
            }
        }
        out.insert(cond.clone(), independent);
    }
    out
}

pub(crate) fn obligation_coverage(
    args: CoverageRunArgs<'_>,
) -> Result<ObligationResult, ObligationError> {
    let CoverageRunArgs {
        store,
        pkg_dir,
        manifest,
        modules,
        tests,
        limits,
        frontend,
        profile,
        obligation_name,
    } = args;
    // Coverage definition (v0.2): each non-test exported symbol must be *looked up as a variable*
    // at least once during the package unit tests.
    //
    // "Non-test export" means: exports listed in module ::meta :exports, excluding any suite
    // symbols configured in package.toml `tests` or `property_tests`.
    let mut exports: BTreeSet<String> = BTreeSet::new();
    for m in modules {
        let Some(meta) = extract_meta_static(&m.forms) else {
            continue;
        };
        let Some(es) = meta_exports(&meta) else {
            continue;
        };
        exports.extend(es);
    }

    let mut excluded: BTreeSet<String> = BTreeSet::new();
    excluded.extend(manifest.tests.iter().cloned());
    excluded.extend(manifest.property_tests.iter().cloned());

    let tracked: BTreeSet<String> = exports.difference(&excluded).cloned().collect();

    let mut expected_statement_sites: BTreeSet<String> = BTreeSet::new();
    let mut expected_decision_conditions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for m in modules {
        let cov = compiled_module_coverage_manifest(&m.forms, &m.entry.path).map_err(|e| {
            ObligationError::Module(format!(
                "{}: static coverage manifest failed: {e}",
                m.abs_path.display()
            ))
        })?;
        expected_statement_sites.extend(cov.statement_sites);
        for site in cov.decision_sites {
            expected_decision_conditions.entry(site).or_default();
        }
        for (site, conds) in cov.decision_conditions {
            expected_decision_conditions
                .entry(site)
                .or_default()
                .extend(conds);
        }
    }

    // Used for replaying effectful tests without re-running capabilities.
    let effect_store = gc_effects::ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
        .map_err(|e| ObligationError::Test(format!("artifact store open failed: {e}")))?;

    let mut total_hits: BTreeMap<String, u64> = BTreeMap::new();
    let mut total_statement_site_hits: BTreeMap<String, u64> = BTreeMap::new();
    let mut total_decision_site_hits: BTreeMap<String, DecisionCoverageCounters> = BTreeMap::new();
    let mut total_decision_samples: BTreeMap<String, Vec<DecisionSample>> = BTreeMap::new();
    let mut total_decision_total: u64 = 0;
    let mut total_decision_true: u64 = 0;
    let mut total_decision_false: u64 = 0;
    let mut test_terms: Vec<Term> = Vec::new();
    let mut missing_effect_logs: Vec<String> = Vec::new();

    for t in tests {
        let mut ctx = mk_eval_ctx(limits);
        ctx.enable_coverage(tracked.clone());

        let prelude = build_prelude(&mut ctx);
        let mut base = prelude.env;
        base = eval_dependencies(&mut ctx, pkg_dir, &base, &manifest.dependencies)?;
        let evals = eval_modules(&mut ctx, &base, modules)?;
        let pkg = PackageEval::from_modules(base, evals)?;

        let suite_v = pkg.lookup_any(&t.id.suite_sym).ok_or_else(|| {
            ObligationError::Test(format!("missing test suite symbol {}", t.id.suite_sym))
        })?;
        let suite_map = value_as_map(&suite_v).ok_or_else(|| {
            ObligationError::Test(format!("test suite {} must be a map", t.id.suite_sym))
        })?;
        let (test_body, _expect) = parse_test_entry(
            suite_map
                .get(&TermOrdKey(Term::Str(t.id.test_name.clone())))
                .or_else(|| suite_map.get(&TermOrdKey(Term::Symbol(t.id.test_name.clone()))))
                .ok_or_else(|| {
                    ObligationError::Test(format!(
                        "missing test {} in suite {}",
                        t.id.test_name, t.id.suite_sym
                    ))
                })?,
        )?;

        let value = test_body
            .apply(&mut ctx, Value::data(Term::Nil))
            .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;

        match (value, &t.effect_log) {
            (v @ Value::EffectProgram(_), Some(log)) => {
                let _ = replay_effect_program(&mut ctx, v, log, &effect_store, frontend)?;
            }
            (Value::EffectProgram(_), None) => {
                missing_effect_logs.push(t.id.test_name.clone());
            }
            _ => {}
        }

        let mut hits_vec: Vec<Term> = Vec::new();
        if let Some(hits) = ctx.coverage_hits() {
            for (sym, c) in hits {
                if *c == 0 {
                    continue;
                }
                *total_hits.entry(sym.clone()).or_insert(0) += *c;
                hits_vec.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":sym")), Term::Symbol(sym.clone())),
                        (
                            TermOrdKey(Term::symbol(":hits")),
                            Term::Int((*c as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }
        }
        let mut statement_sites_vec: Vec<Term> = Vec::new();
        if let Some(site_hits) = ctx.coverage_statement_site_hits() {
            for (site, hits) in site_hits {
                *total_statement_site_hits.entry(site.clone()).or_insert(0) += *hits;
                statement_sites_vec.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":site")), Term::Str(site.clone())),
                        (
                            TermOrdKey(Term::symbol(":hits")),
                            Term::Int((*hits as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }
        }
        let mut decision_sites_vec: Vec<Term> = Vec::new();
        if let Some(site_hits) = ctx.coverage_decision_site_hits() {
            for (site, counts) in site_hits {
                let acc = total_decision_site_hits.entry(site.clone()).or_default();
                acc.total = acc.total.saturating_add(counts.total);
                acc.taken_true = acc.taken_true.saturating_add(counts.taken_true);
                acc.taken_false = acc.taken_false.saturating_add(counts.taken_false);
                decision_sites_vec.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":site")), Term::Str(site.clone())),
                        (
                            TermOrdKey(Term::symbol(":total")),
                            Term::Int((counts.total as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":taken-true")),
                            Term::Int((counts.taken_true as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":taken-false")),
                            Term::Int((counts.taken_false as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }
        }
        if let Some(samples) = ctx.coverage_decision_samples() {
            for (site, xs) in samples {
                total_decision_samples
                    .entry(site.clone())
                    .or_default()
                    .extend(xs.iter().cloned());
            }
        }

        let decision = ctx.coverage_decision_counts().unwrap_or_default();
        total_decision_total = total_decision_total.saturating_add(decision.total);
        total_decision_true = total_decision_true.saturating_add(decision.taken_true);
        total_decision_false = total_decision_false.saturating_add(decision.taken_false);

        test_terms.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":suite")),
                    Term::Symbol(t.id.suite_sym.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str(t.id.test_name.clone()),
                ),
                (TermOrdKey(Term::symbol(":hits")), Term::Vector(hits_vec)),
                (
                    TermOrdKey(Term::symbol(":statement-sites")),
                    Term::Vector(statement_sites_vec),
                ),
                (
                    TermOrdKey(Term::symbol(":decision-sites")),
                    Term::Vector(decision_sites_vec),
                ),
                (
                    TermOrdKey(Term::symbol(":decision")),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":total")),
                                Term::Int((decision.total as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":taken-true")),
                                Term::Int((decision.taken_true as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":taken-false")),
                                Term::Int((decision.taken_false as i64).into()),
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

    let observation = coverage_authority_observation(
        manifest,
        tests.len(),
        profile,
        &tracked,
        &total_hits,
        &expected_statement_sites,
        &total_statement_site_hits,
        &expected_decision_conditions,
        &total_decision_site_hits,
        &total_decision_samples,
        total_decision_total,
        total_decision_true,
        total_decision_false,
        test_terms,
        &missing_effect_logs,
    );
    evaluate_coverage_obligation_with_authority(
        store,
        manifest,
        profile,
        obligation_name,
        &observation,
        frontend,
        limits,
    )
}
