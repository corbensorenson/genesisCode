use super::*;
use crate::obligation_authority::{
    PropertyAttemptObservation, PropertyOutcomeObservation, property_authority_context,
    property_authority_finalize, property_authority_plan, property_body,
};

pub(super) fn obligation_property_tests(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let context = property_authority_context(pkg_dir, manifest, modules, limits)?;
    let plan = property_authority_plan(manifest, &context, frontend, limits)?;
    let mut outcomes = Vec::with_capacity(plan.len());
    for test in &plan {
        let body = property_body(&context, test)?;
        let mut attempts = Vec::with_capacity(test.seeds.len());
        for (index, seed) in test.seeds.iter().copied().enumerate() {
            let mut ctx = mk_eval_ctx(limits);
            let arg = Value::data(Term::Int(BigInt::from(seed)));
            let (kind, result, pass) = match body.clone().apply(&mut ctx, arg) {
                Err(error) => (":apply-error", Term::Str(error.to_string()), false),
                Ok(Value::EffectProgram(_)) => (":effect-program", Term::Nil, false),
                Ok(value) => {
                    let is_error = ctx.protocol.is_some_and(|protocol| {
                        matches!(value, Value::Sealed { token, .. } if token == protocol.error)
                    });
                    let pass = matches!(value.as_data(), Some(Term::Bool(true))) && !is_error;
                    let protocol_error = ctx.protocol.map(|protocol| protocol.error);
                    (":value", value.to_term_for_log(protocol_error), pass)
                }
            };
            attempts.push(PropertyAttemptObservation {
                index: index as u64,
                seed,
                kind,
                result,
            });
            // The self-hosted plan authorizes this exact bounded stop rule.
            if !pass {
                break;
            }
        }
        outcomes.push(PropertyOutcomeObservation {
            suite_index: test.suite_index,
            entry_index: test.entry_index,
            attempts,
        });
    }
    property_authority_finalize(store, manifest, &context, &outcomes, frontend, limits)
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
        Some(Value::Data(t)) => match t.as_ref() {
            Term::Int(i) => i
                .to_u64()
                .ok_or_else(|| ObligationError::Test("property :cases must fit u64".to_string()))?,
            _ => {
                return Err(ObligationError::Test(format!(
                    "property :cases must be int, got {}",
                    Value::Data(t.clone()).debug_repr()
                )));
            }
        },
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
            Some(Value::Data(t)) => Some(t.as_ref().clone()),
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
