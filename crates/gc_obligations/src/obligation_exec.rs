use super::*;

pub(super) fn obligation_budgets(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();

    let max_steps = manifest.budgets.max_steps_per_test;
    let max_entries = manifest.budgets.max_effect_entries_per_test;
    let max_log_bytes = manifest.budgets.max_effect_log_bytes_per_test;

    let mut test_terms: Vec<Term> = Vec::new();
    for t in tests {
        let mut t_ok = true;
        if let Some(ms) = max_steps
            && t.steps > ms
        {
            t_ok = false;
            errors.push(format!(
                "test {}::{} exceeded max_steps_per_test: {} > {}",
                t.id.suite_sym, t.id.test_name, t.steps, ms
            ));
        }
        if let Some(me) = max_entries
            && t.effect_entries > me
        {
            t_ok = false;
            errors.push(format!(
                "test {}::{} exceeded max_effect_entries_per_test: {} > {}",
                t.id.suite_sym, t.id.test_name, t.effect_entries, me
            ));
        }
        if let Some(ml) = max_log_bytes
            && t.effect_log_bytes > ml
        {
            t_ok = false;
            errors.push(format!(
                "test {}::{} exceeded max_effect_log_bytes_per_test: {} > {}",
                t.id.suite_sym, t.id.test_name, t.effect_log_bytes, ml
            ));
        }
        ok &= t_ok;

        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(t.id.suite_sym.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(t.id.test_name.clone()),
        );
        m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(t_ok));
        m.insert(
            TermOrdKey(Term::symbol(":steps")),
            Term::Int((t.steps as i64).into()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":effect-entries")),
            Term::Int((t.effect_entries as i64).into()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":effect-log-bytes")),
            Term::Int((t.effect_log_bytes as i64).into()),
        );
        test_terms.push(Term::Map(m));
    }

    let mut limits = BTreeMap::new();
    if let Some(ms) = max_steps {
        limits.insert(
            TermOrdKey(Term::symbol(":max-steps-per-test")),
            Term::Int((ms as i64).into()),
        );
    }
    if let Some(me) = max_entries {
        limits.insert(
            TermOrdKey(Term::symbol(":max-effect-entries-per-test")),
            Term::Int((me as i64).into()),
        );
    }
    if let Some(ml) = max_log_bytes {
        limits.insert(
            TermOrdKey(Term::symbol(":max-effect-log-bytes-per-test")),
            Term::Int((ml as i64).into()),
        );
    }

    let mut report = BTreeMap::new();
    report.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/budgets-v0.2".to_string()),
    );
    report.insert(
        TermOrdKey(Term::symbol(":package")),
        Term::Str(manifest.name.clone()),
    );
    report.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    report.insert(TermOrdKey(Term::symbol(":limits")), Term::Map(limits));
    report.insert(TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms));
    if !errors.is_empty() {
        report.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        );
    }

    let report = Term::Map(report);
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::budgets".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_coverage(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
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
    if tracked.is_empty() {
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/coverage-v0.2".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str(manifest.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":note")),
                    Term::Str("no non-test exports".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let artifact = store.put_term(&report)?;
        return Ok(ObligationResult {
            name: "core/obligation::coverage".to_string(),
            ok: true,
            artifact: Some(artifact),
            errors: Vec::new(),
        });
    }

    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();

    if tests.is_empty() {
        ok = false;
        errors.push("coverage requires unit tests (package.toml `tests` is empty)".to_string());
    }

    // Used for replaying effectful tests without re-running capabilities.
    let effect_store = gc_effects::ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
        .map_err(|e| ObligationError::Test(format!("artifact store open failed: {e}")))?;

    let mut total_hits: BTreeMap<String, u64> = BTreeMap::new();
    let mut test_terms: Vec<Term> = Vec::new();

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
            .apply(&mut ctx, Value::Data(Term::Nil))
            .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;

        match (value, &t.effect_log) {
            (v @ Value::EffectProgram(_), Some(log)) => {
                let _ = gc_effects::replay_with_store(&mut ctx, v, log, Some(&effect_store))
                    .map_err(|e| ObligationError::Test(format!("replay failed: {e}")))?;
            }
            (Value::EffectProgram(_), None) => {
                ok = false;
                errors.push(format!(
                    "coverage: test {} returned effect program but no effect log was recorded",
                    t.id.test_name
                ));
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
            ]
            .into_iter()
            .collect(),
        ));
    }

    let mut missing: Vec<Term> = Vec::new();
    let mut export_terms: Vec<Term> = Vec::new();
    for sym in &tracked {
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
                TermOrdKey(Term::symbol(":definition")),
                Term::Str("exports minus (tests, property_tests)".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(export_terms),
            ),
            (TermOrdKey(Term::symbol(":missing")), Term::Vector(missing)),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms)),
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

    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::coverage".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

#[derive(Debug, Clone)]
struct PropertyTest {
    id: TestId,
    body: Value,
    cases: u64,
}

pub(super) fn obligation_property_tests(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let default_cases = manifest.property.cases_per_test.unwrap_or(64);
    if manifest.property_tests.is_empty() {
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/property-tests-v0.2".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str(manifest.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":note")),
                    Term::Str("no property tests".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let artifact = store.put_term(&report)?;
        return Ok(ObligationResult {
            name: "core/obligation::property-tests".to_string(),
            ok: true,
            artifact: Some(artifact),
            errors: Vec::new(),
        });
    }

    // Evaluate package once to extract property bodies and per-test case counts.
    let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
    let mut props: Vec<PropertyTest> = Vec::new();

    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();
    let mut test_terms: Vec<Term> = Vec::new();

    for suite in &manifest.property_tests {
        let Some(suite_v) = eval.lookup_any(suite) else {
            ok = false;
            errors.push(format!("missing property suite symbol {suite}"));
            continue;
        };
        let Some(suite_map) = value_as_map(&suite_v) else {
            ok = false;
            errors.push(format!("property suite {suite} must be a map"));
            continue;
        };
        for (k, vv) in suite_map.iter() {
            let name = match &k.0 {
                Term::Str(s) => s.clone(),
                Term::Symbol(s) => s.clone(),
                other => {
                    ok = false;
                    errors.push(format!(
                        "property suite {suite}: key must be string/symbol, got {}",
                        print_term(other)
                    ));
                    continue;
                }
            };
            match parse_property_entry(vv, default_cases) {
                Ok((body, cases)) => props.push(PropertyTest {
                    id: TestId {
                        suite_sym: suite.clone(),
                        test_name: name,
                    },
                    body,
                    cases,
                }),
                Err(e) => {
                    ok = false;
                    errors.push(format!("property suite {suite}::{name}: {e}"));
                }
            }
        }
    }

    for p in &props {
        let mut seeds: Vec<u64> = Vec::with_capacity(p.cases as usize);
        for i in 0..p.cases {
            seeds.push(seed_for_case(
                &manifest.name,
                &p.id.suite_sym,
                &p.id.test_name,
                i,
            ));
        }

        let mut t_ok = true;
        let mut first_failure: Option<Term> = None;

        for (i, seed) in seeds.iter().copied().enumerate() {
            let mut ctx = mk_eval_ctx(limits);
            let arg = Value::Data(Term::Int(BigInt::from(seed)));
            let r = match p.body.clone().apply(&mut ctx, arg) {
                Ok(v) => v,
                Err(e) => {
                    t_ok = false;
                    first_failure = Some(Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":i")), Term::Int((i as i64).into())),
                            (
                                TermOrdKey(Term::symbol(":seed")),
                                Term::Int(BigInt::from(seed)),
                            ),
                            (
                                TermOrdKey(Term::symbol(":result")),
                                Term::Str(format!("apply failed: {e}")),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ));
                    errors.push(format!(
                        "property test apply failed {}::{} at case {}: {e}",
                        p.id.suite_sym, p.id.test_name, i
                    ));
                    break;
                }
            };

            if matches!(r, Value::EffectProgram(_)) {
                t_ok = false;
                first_failure = Some(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":i")), Term::Int((i as i64).into())),
                        (
                            TermOrdKey(Term::symbol(":seed")),
                            Term::Int(BigInt::from(seed)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":result")),
                            Term::Str(
                                "effect program returned (property tests must be pure)".to_string(),
                            ),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
                errors.push(format!(
                    "property test {}::{} returned an effect program (must be pure)",
                    p.id.suite_sym, p.id.test_name
                ));
                break;
            }

            let is_error = ctx
                .protocol
                .is_some_and(|pt| matches!(r, Value::Sealed { token, .. } if token == pt.error));

            let pass = matches!(r, Value::Data(Term::Bool(true))) && !is_error;
            if !pass {
                t_ok = false;
                let proto_err = ctx.protocol.map(|pt| pt.error);
                let rt = r.to_term_for_log(proto_err);
                first_failure = Some(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":i")), Term::Int((i as i64).into())),
                        (
                            TermOrdKey(Term::symbol(":seed")),
                            Term::Int(BigInt::from(seed)),
                        ),
                        (TermOrdKey(Term::symbol(":result")), rt),
                    ]
                    .into_iter()
                    .collect(),
                ));
                errors.push(format!(
                    "property test failed {}::{} at case {}",
                    p.id.suite_sym, p.id.test_name, i
                ));
                break;
            }
        }

        ok &= t_ok;

        let mut tm = BTreeMap::new();
        tm.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(p.id.suite_sym.clone()),
        );
        tm.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(p.id.test_name.clone()),
        );
        tm.insert(
            TermOrdKey(Term::symbol(":cases")),
            Term::Int((p.cases as i64).into()),
        );
        tm.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(t_ok));
        tm.insert(
            TermOrdKey(Term::symbol(":seeds")),
            Term::Vector(
                seeds
                    .iter()
                    .copied()
                    .map(|s| Term::Int(BigInt::from(s)))
                    .collect(),
            ),
        );
        if let Some(ff) = first_failure {
            tm.insert(TermOrdKey(Term::symbol(":first-failure")), ff);
        }
        test_terms.push(Term::Map(tm));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/property-tests-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":config")),
                Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":cases-per-test")),
                        Term::Int((default_cases as i64).into()),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms)),
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
                        Term::Str("genesis/property-tests-v0.2".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":errors")),
                        Term::Vector(
                            std::iter::once(Term::Str(format!(
                                "internal property-tests report shape drift: {}",
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
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::property-tests".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn is_callable_value(v: &Value) -> bool {
    matches!(
        v,
        Value::Closure { .. } | Value::CompiledClosure { .. } | Value::NativeFn(_)
    )
}

pub(super) fn parse_property_entry(
    v: &Value,
    default_cases: u64,
) -> Result<(Value, u64), ObligationError> {
    if is_callable_value(v) {
        return Ok((v.clone(), default_cases));
    }
    let Some(m) = value_as_map(v) else {
        return Err(ObligationError::Test(format!(
            "invalid property entry: {}",
            v.debug_repr()
        )));
    };
    let body = m
        .get(&TermOrdKey(Term::Symbol(":body".to_string())))
        .ok_or_else(|| ObligationError::Test("property map missing :body".to_string()))?;
    if !is_callable_value(body) {
        return Err(ObligationError::Test(
            "property :body must be callable".to_string(),
        ));
    }
    let cases = match m.get(&TermOrdKey(Term::Symbol(":cases".to_string()))) {
        None => default_cases,
        Some(Value::Data(Term::Int(i))) => i
            .to_u64()
            .ok_or_else(|| ObligationError::Test("property :cases must fit u64".to_string()))?,
        Some(other) => {
            return Err(ObligationError::Test(format!(
                "property :cases must be int, got {}",
                other.debug_repr()
            )));
        }
    };
    Ok((body.clone(), cases))
}

pub(super) fn seed_for_case(pkg: &str, suite: &str, name: &str, i: u64) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(b"GCv0.2\0property\0seed\0");
    h.update(pkg.as_bytes());
    h.update(b"\0");
    h.update(suite.as_bytes());
    h.update(b"\0");
    h.update(name.as_bytes());
    h.update(b"\0");
    h.update(&i.to_le_bytes());
    let out = h.finalize();
    let mut b = [0u8; 8];
    b.copy_from_slice(&out.as_bytes()[0..8]);
    u64::from_le_bytes(b)
}

pub(super) fn parse_test_entry(v: &Value) -> Result<(Value, Option<Term>), ObligationError> {
    // Either a callable directly, or a map { :body callable :expect datum }
    if is_callable_value(v) {
        return Ok((v.clone(), None));
    }
    if let Some(m) = value_as_map(v) {
        let body = m
            .get(&TermOrdKey(Term::Symbol(":body".to_string())))
            .ok_or_else(|| ObligationError::Test("test map missing :body".to_string()))?;
        if !is_callable_value(body) {
            return Err(ObligationError::Test(
                "test :body must be callable".to_string(),
            ));
        }
        let expect = match m.get(&TermOrdKey(Term::Symbol(":expect".to_string()))) {
            None => None,
            Some(Value::Data(t)) => Some(t.clone()),
            Some(other) => {
                return Err(ObligationError::Test(format!(
                    "test :expect must be a datum, got {}",
                    other.debug_repr()
                )));
            }
        };
        return Ok((body.clone(), expect));
    }
    Err(ObligationError::Test(format!(
        "invalid test entry: {}",
        v.debug_repr()
    )))
}

pub(super) fn obligation_unit_tests(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut test_terms = Vec::new();
    for t in tests {
        ok &= t.ok;
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
    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/unit-tests-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms)),
        ]
        .into_iter()
        .collect(),
    );
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

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/determinism-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
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

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/caps-declared-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
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
        name: "core/obligation::capabilities-declared".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_replayable(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors = Vec::new();
    let effect_store = gc_effects::ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
        .map_err(|e| ObligationError::Test(format!("artifact store open failed: {e}")))?;
    for t in tests {
        let Some(log) = &t.effect_log else { continue };

        // Re-evaluate and replay.
        let mut ctx = mk_eval_ctx(limits);
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
            .apply(&mut ctx, Value::Data(Term::Nil))
            .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;
        let Value::EffectProgram(_) = value else {
            ok = false;
            errors.push(format!(
                "test {} expected effect program for replayability",
                t.id.test_name
            ));
            continue;
        };
        let v2 = gc_effects::replay_with_store(&mut ctx, value, log, Some(&effect_store))
            .map_err(|e| ObligationError::Test(format!("replay failed: {e}")))?;
        let h2 = value_hash(&v2);
        if h2 != t.value_hash {
            ok = false;
            errors.push(format!(
                "replay mismatch for {}: {}",
                t.id.test_name,
                hex32(h2)
            ));
        }

        // Store log artifact too (for provenance).
        let _ = store.put_term(&log.to_term())?;
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/replayable-tests-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
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
        name: "core/obligation::replayable-tests".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_concurrency_replay(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors = Vec::new();
    let mut concurrent_tests: u64 = 0;
    let effect_store = gc_effects::ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
        .map_err(|e| ObligationError::Test(format!("artifact store open failed: {e}")))?;

    for t in tests {
        let Some(log) = &t.effect_log else { continue };
        if !log_contains_task_ops(log) {
            continue;
        }
        concurrent_tests = concurrent_tests.saturating_add(1);

        for (idx, entry) in log.entries.iter().enumerate() {
            if !is_task_like_op(&entry.op) {
                continue;
            }
            if entry.schedule_step != Some(idx as u64) {
                ok = false;
                errors.push(format!(
                    "concurrency log mismatch for {}::{} at entry {}: expected :schedule-step {}, got {:?}",
                    t.id.suite_sym, t.id.test_name, idx, idx, entry.schedule_step
                ));
            }
            if matches!(entry.op.as_str(), "core/task::await") && entry.await_edge.is_none() {
                ok = false;
                errors.push(format!(
                    "concurrency log missing :await-edge for {}::{} at entry {}",
                    t.id.suite_sym, t.id.test_name, idx
                ));
            }
            if matches!(
                entry.op.as_str(),
                "core/task::await"
                    | "core/task::cancel"
                    | "core/task::status"
                    | "editor/task::poll"
                    | "editor/task::cancel"
            ) && entry.task_id.is_none()
            {
                ok = false;
                errors.push(format!(
                    "concurrency log missing :task-id for {}::{} at entry {} ({})",
                    t.id.suite_sym, t.id.test_name, idx, entry.op
                ));
            }
        }

        let mut ctx = mk_eval_ctx(limits);
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
            .apply(&mut ctx, Value::Data(Term::Nil))
            .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;
        let Value::EffectProgram(_) = value else {
            ok = false;
            errors.push(format!(
                "test {} expected effect program for concurrency replay",
                t.id.test_name
            ));
            continue;
        };
        let v2 = gc_effects::replay_with_store(&mut ctx, value, log, Some(&effect_store))
            .map_err(|e| ObligationError::Test(format!("concurrency replay failed: {e}")))?;
        let h2 = value_hash(&v2);
        if h2 != t.value_hash {
            ok = false;
            errors.push(format!(
                "concurrency replay mismatch for {}::{}: {}",
                t.id.suite_sym,
                t.id.test_name,
                hex32(h2)
            ));
        }

        let _ = store.put_term(&log.to_term())?;
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/concurrency-replay-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":concurrent-tests")),
                Term::Int((concurrent_tests as i64).into()),
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
        name: "core/obligation::concurrency-replay".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn is_task_like_op(op: &str) -> bool {
    op.starts_with("core/task::") || op.starts_with("editor/task::")
}

pub(super) fn log_contains_task_ops(log: &EffectLog) -> bool {
    log.entries.iter().any(|e| is_task_like_op(&e.op))
}

pub(super) fn obligation_typecheck(
    store: &EvidenceStore,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let report = typecheck_report_with_frontend(modules, frontend, limits)?;
    let ok = report.ok;
    let artifact = store.put_term(&report.to_term())?;
    Ok(ObligationResult {
        name: "core/obligation::typecheck".to_string(),
        ok,
        artifact: Some(artifact),
        errors: report.errors,
    })
}

pub(super) fn typecheck_report_with_frontend(
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<gc_types::TypecheckReport, ObligationError> {
    let mut mods = Vec::new();
    for m in modules {
        mods.push(gc_types::ModuleForTypecheck {
            path: m.entry.path.clone(),
            forms: m.forms.clone(),
            meta: m.meta.clone(),
        });
    }
    let report = gc_types::typecheck_package(&mods);
    verify_selfhost_infer_effects_parity(modules, frontend, limits)?;
    Ok(report)
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
