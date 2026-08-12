use super::obligation_exec_coverage::{
    CoverageAuthorityObservation, CoverageProfile, mcdc_independence_for_site,
};
use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "closed coverage contradiction reconstruction consumes the complete raw fact set"
)]
pub(crate) fn coverage_authority_observation(
    manifest: &PackageManifest,
    test_count: usize,
    profile: CoverageProfile,
    tracked: &BTreeSet<String>,
    total_hits: &BTreeMap<String, u64>,
    expected_statement_sites: &BTreeSet<String>,
    total_statement_site_hits: &BTreeMap<String, u64>,
    expected_decision_conditions: &BTreeMap<String, BTreeSet<String>>,
    total_decision_site_hits: &BTreeMap<String, DecisionCoverageCounters>,
    total_decision_samples: &BTreeMap<String, Vec<DecisionSample>>,
    total_decision_total: u64,
    total_decision_true: u64,
    total_decision_false: u64,
    test_terms: Vec<Term>,
    missing_effect_logs: &[String],
) -> CoverageAuthorityObservation {
    let mut ok = true;
    let mut errors = Vec::new();
    if test_count == 0 && (!tracked.is_empty() || profile.requires_structural_gates()) {
        ok = false;
        errors.push("coverage requires unit tests (package.toml `tests` is empty)".to_string());
    }
    for test_name in missing_effect_logs {
        ok = false;
        errors.push(format!(
            "coverage: test {test_name} returned effect program but no effect log was recorded"
        ));
    }
    let mut missing: Vec<Term> = Vec::new();
    let mut export_terms: Vec<Term> = Vec::new();
    for sym in tracked {
        let c = *total_hits.get(sym).unwrap_or(&0);
        if c == 0 {
            ok = false;
            missing.push(Term::Symbol(sym.clone()));
            errors.push(format!("export not covered: {sym}"));
        }
        export_terms.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":sym")), Term::Symbol(sym.clone())),
                (
                    TermOrdKey(Term::symbol(":hits")),
                    Term::Int((c as i64).into()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let mut statement_site_terms: Vec<Term> = Vec::new();
    let mut missing_statement_sites: Vec<Term> = Vec::new();
    for site in expected_statement_sites {
        let hits = *total_statement_site_hits.get(site).unwrap_or(&0);
        let site_ok = hits > 0;
        if !site_ok {
            missing_statement_sites.push(Term::Str(site.clone()));
        }
        statement_site_terms.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":site")), Term::Str(site.clone())),
                (
                    TermOrdKey(Term::symbol(":hits")),
                    Term::Int((hits as i64).into()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(site_ok)),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let mut decision_site_terms: Vec<Term> = Vec::new();
    let mut missing_decision_sites: Vec<Term> = Vec::new();
    let mut mcdc_terms: Vec<Term> = Vec::new();
    let mut missing_mcdc_sites: Vec<Term> = Vec::new();
    for (site, expected_conds) in expected_decision_conditions {
        let counts = total_decision_site_hits
            .get(site)
            .copied()
            .unwrap_or_default();
        let branch_ok = counts.total > 0 && counts.taken_true > 0 && counts.taken_false > 0;
        if !branch_ok {
            missing_decision_sites.push(Term::Str(site.clone()));
        }
        let cond_vec: Vec<Term> = expected_conds.iter().cloned().map(Term::symbol).collect();
        let samples = total_decision_samples
            .get(site)
            .cloned()
            .unwrap_or_default();
        let mcdc_status = mcdc_independence_for_site(&samples, expected_conds);
        let mut mcdc_status_terms: Vec<Term> = Vec::new();
        let mut mcdc_missing_for_site: Vec<Term> = Vec::new();
        for (cond, independent) in &mcdc_status {
            mcdc_status_terms.push(Term::Map(
                [
                    (TermOrdKey(Term::symbol(":sym")), Term::symbol(cond)),
                    (
                        TermOrdKey(Term::symbol(":independent")),
                        Term::Bool(*independent),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
            if !independent {
                mcdc_missing_for_site.push(Term::symbol(cond));
            }
        }
        if !mcdc_missing_for_site.is_empty() {
            missing_mcdc_sites.push(Term::Map(
                [
                    (TermOrdKey(Term::symbol(":site")), Term::Str(site.clone())),
                    (
                        TermOrdKey(Term::symbol(":missing-conditions")),
                        Term::Vector(mcdc_missing_for_site),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
        }
        mcdc_terms.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":site")), Term::Str(site.clone())),
                (
                    TermOrdKey(Term::symbol(":conditions")),
                    Term::Vector(mcdc_status_terms),
                ),
            ]
            .into_iter()
            .collect(),
        ));
        decision_site_terms.push(Term::Map(
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
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(branch_ok)),
                (
                    TermOrdKey(Term::symbol(":conditions")),
                    Term::Vector(cond_vec),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    if profile.requires_structural_gates() {
        if !missing_statement_sites.is_empty() {
            ok = false;
            errors.push(format!(
                "statement-site coverage missing {} site(s)",
                missing_statement_sites.len()
            ));
        }
        if !missing_decision_sites.is_empty() {
            ok = false;
            errors.push(format!(
                "decision coverage missing branch outcomes on {} site(s)",
                missing_decision_sites.len()
            ));
        }
    }
    if profile.requires_mcdc() && !missing_mcdc_sites.is_empty() {
        ok = false;
        errors.push(format!(
            "mcdc coverage missing condition independence on {} decision site(s)",
            missing_mcdc_sites.len()
        ));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/coverage-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::symbol(format!(":{}", profile.token())),
            ),
            (
                TermOrdKey(Term::symbol(":definition")),
                Term::Str("exports minus (tests, property_tests)".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(export_terms),
            ),
            (TermOrdKey(Term::symbol(":missing")), Term::Vector(missing)),
            (
                TermOrdKey(Term::symbol(":structural")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":decision")),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":total")),
                                        Term::Int((total_decision_total as i64).into()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":taken-true")),
                                        Term::Int((total_decision_true as i64).into()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":taken-false")),
                                        Term::Int((total_decision_false as i64).into()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        ),
                        (
                            TermOrdKey(Term::symbol(":expected")),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":statement-sites")),
                                        Term::Int((expected_statement_sites.len() as i64).into()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":decision-sites")),
                                        Term::Int(
                                            (expected_decision_conditions.len() as i64).into(),
                                        ),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        ),
                        (
                            TermOrdKey(Term::symbol(":statement-sites")),
                            Term::Vector(statement_site_terms),
                        ),
                        (
                            TermOrdKey(Term::symbol(":decision-sites")),
                            Term::Vector(decision_site_terms),
                        ),
                        (
                            TermOrdKey(Term::symbol(":missing-statement-sites")),
                            Term::Vector(missing_statement_sites),
                        ),
                        (
                            TermOrdKey(Term::symbol(":missing-decision-sites")),
                            Term::Vector(missing_decision_sites),
                        ),
                        (TermOrdKey(Term::symbol(":mcdc")), Term::Vector(mcdc_terms)),
                        (
                            TermOrdKey(Term::symbol(":missing-mcdc-sites")),
                            Term::Vector(missing_mcdc_sites),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(test_terms.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let report = if errors.is_empty() {
        report
    } else {
        match report {
            Term::Map(mut m) => {
                m.insert(
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
                );
                Term::Map(m)
            }
            other => Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":kind")),
                        Term::Str("genesis/coverage-v0.2".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":errors")),
                        Term::Vector(
                            std::iter::once(Term::Str(format!(
                                "internal coverage report shape drift: {}",
                                print_term(&other)
                            )))
                            .chain(errors.iter().cloned().map(Term::Str))
                            .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        }
    };

    let decision_observations = expected_decision_conditions
        .iter()
        .map(|(site, conditions)| {
            let counts = total_decision_site_hits
                .get(site)
                .copied()
                .unwrap_or_default();
            let samples = total_decision_samples
                .get(site)
                .into_iter()
                .flatten()
                .map(|sample| {
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":conditions")),
                                Term::Vector(
                                    sample
                                        .conditions
                                        .iter()
                                        .map(|(symbol, value)| {
                                            Term::Map(
                                                [
                                                    (
                                                        TermOrdKey(Term::symbol(":sym")),
                                                        Term::symbol(symbol),
                                                    ),
                                                    (
                                                        TermOrdKey(Term::symbol(":value")),
                                                        Term::Bool(*value),
                                                    ),
                                                ]
                                                .into_iter()
                                                .collect(),
                                            )
                                        })
                                        .collect(),
                                ),
                            ),
                            (
                                TermOrdKey(Term::symbol(":outcome")),
                                Term::Bool(sample.outcome),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )
                })
                .collect();
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":conditions")),
                        Term::Vector(conditions.iter().cloned().map(Term::symbol).collect()),
                    ),
                    (TermOrdKey(Term::symbol(":samples")), Term::Vector(samples)),
                    (TermOrdKey(Term::symbol(":site")), Term::Str(site.clone())),
                    (
                        TermOrdKey(Term::symbol(":taken-false")),
                        Term::Int(BigInt::from(counts.taken_false)),
                    ),
                    (
                        TermOrdKey(Term::symbol(":taken-true")),
                        Term::Int(BigInt::from(counts.taken_true)),
                    ),
                    (
                        TermOrdKey(Term::symbol(":total")),
                        Term::Int(BigInt::from(counts.total)),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    let inputs = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":decision")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":taken-false")),
                            Term::Int(BigInt::from(total_decision_false)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":taken-true")),
                            Term::Int(BigInt::from(total_decision_true)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":total")),
                            Term::Int(BigInt::from(total_decision_total)),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":decision-sites")),
                Term::Vector(decision_observations),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(
                    tracked
                        .iter()
                        .map(|symbol| {
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":hits")),
                                        Term::Int(BigInt::from(
                                            total_hits.get(symbol).copied().unwrap_or_default(),
                                        )),
                                    ),
                                    (TermOrdKey(Term::symbol(":sym")), Term::symbol(symbol)),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":missing-effect-logs")),
                Term::Vector(missing_effect_logs.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::symbol(format!(":{}", profile.token())),
            ),
            (
                TermOrdKey(Term::symbol(":statement-sites")),
                Term::Vector(
                    expected_statement_sites
                        .iter()
                        .map(|site| {
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":hits")),
                                        Term::Int(BigInt::from(
                                            total_statement_site_hits
                                                .get(site)
                                                .copied()
                                                .unwrap_or_default(),
                                        )),
                                    ),
                                    (TermOrdKey(Term::symbol(":site")), Term::Str(site.clone())),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":test-count")),
                Term::Int(BigInt::from(test_count as u64)),
            ),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms)),
        ]
        .into_iter()
        .collect(),
    );
    CoverageAuthorityObservation {
        inputs,
        expected_ok: ok,
        expected_errors: errors,
        expected_report: report,
    }
}
