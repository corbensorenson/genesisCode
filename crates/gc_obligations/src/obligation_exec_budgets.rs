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
