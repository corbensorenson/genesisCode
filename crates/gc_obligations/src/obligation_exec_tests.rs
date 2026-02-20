use super::*;

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
