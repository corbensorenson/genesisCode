fn translation_stage2_expected(
    path: &str,
    observation: &TranslationStage2Observation,
) -> (bool, bool, Vec<String>, Term) {
    let supported = observation.status != TranslationStage2Status::Unsupported;
    let mut errors = observation.mechanism_errors.clone();
    if observation.status == TranslationStage2Status::Complete {
        if observation.result_equal == Some(false) {
            errors.push("stage2 wasm result differs from kernel result".to_string());
        }
        if observation.original_value_hash != observation.wasm_value_hash {
            errors.push("stage2 wasm value hash mismatch".to_string());
        }
    }
    let ok = observation.status == TranslationStage2Status::Complete && errors.is_empty();
    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":module-h")),
                Term::Bytes(observation.module_hash.to_vec().into()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":orig-value-h")),
                optional_hash_term(observation.original_value_hash),
            ),
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Str(path.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":supported")),
                Term::Bool(supported),
            ),
            (
                TermOrdKey(Term::symbol(":value-kind")),
                observation
                    .value_kind
                    .as_ref()
                    .map(|value| Term::symbol(value))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":wasm-bytes")),
                observation
                    .wasm_bytes_len
                    .map(|value| Term::Int(BigInt::from(value)))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":wasm-h")),
                optional_hash_term(observation.wasm_hash),
            ),
            (
                TermOrdKey(Term::symbol(":wasm-value-h")),
                optional_hash_term(observation.wasm_value_hash),
            ),
        ]
        .into_iter()
        .collect(),
    );
    (supported, ok, errors, report)
}

fn translation_expected(
    manifest: &PackageManifest,
    observation: &TranslationObservation,
) -> (bool, Vec<String>, Term) {
    if observation.original_tests.is_empty() {
        return (
            true,
            Vec::new(),
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":kind")),
                        Term::Str("genesis/translation-validation-v0.2".to_string()),
                    ),
                    (TermOrdKey(Term::symbol(":note")), Term::Str("no tests".to_string())),
                    (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                ]
                .into_iter()
                .collect(),
            ),
        );
    }

    let mut ok = observation.original_tests.iter().all(|test| {
        !test.sealed_error
            && test
                .expected_hash
                .is_none_or(|expected| expected == test.actual_hash)
    });
    let mut errors = Vec::new();
    let mut egg_runs = 0_u64;
    let mut egg_iterations = 0_u64;
    let mut egg_eclasses = 0_u64;
    let mut egg_enodes = 0_u64;
    let mut rewrites = BTreeMap::<String, u64>::new();
    let mut module_reports = Vec::new();
    let mut stage2_entries = Vec::new();
    let mut stage2_supported = 0_u64;
    let mut stage2_validated = 0_u64;
    for module in &observation.modules {
        egg_runs = egg_runs.saturating_add(module.egg_runs);
        egg_iterations = egg_iterations.saturating_add(module.egg_iterations);
        egg_eclasses = egg_eclasses.saturating_add(module.egg_eclasses);
        egg_enodes = egg_enodes.saturating_add(module.egg_enodes);
        for (name, count) in &module.rewrites {
            let entry = rewrites.entry(name.clone()).or_insert(0);
            *entry = entry.saturating_add(*count);
        }
        let (supported, stage_ok, stage_errors, stage_report) =
            translation_stage2_expected(&module.path, &module.stage2);
        if supported {
            stage2_supported = stage2_supported.saturating_add(1);
            if stage_ok {
                stage2_validated = stage2_validated.saturating_add(1);
            } else {
                ok = false;
                errors.extend(
                    stage_errors
                        .iter()
                        .map(|error| format!("stage2 {}: {error}", module.path)),
                );
            }
        }
        stage2_entries.push(stage_report);
        module_reports.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":changed")),
                    Term::Bool(module.original_hash != module.optimized_hash),
                ),
                (
                    TermOrdKey(Term::symbol(":opt-h")),
                    Term::Bytes(module.optimized_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":orig-h")),
                    Term::Bytes(module.original_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(module.path.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let mut test_reports = Vec::new();
    for test in &observation.optimized_tests {
        if test.original_hash != test.optimized_hash {
            ok = false;
            errors.push(format!("hash mismatch for {}::{}", test.suite, test.name));
        }
        test_reports.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str(test.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":opt-h")),
                    Term::Bytes(test.optimized_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":orig-h")),
                    Term::Bytes(test.original_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":suite")),
                    Term::symbol(test.suite.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/translation-validation-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_reports),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":optimizer")),
                Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":egg-eclasses")), Term::Int(BigInt::from(egg_eclasses))),
                        (TermOrdKey(Term::symbol(":egg-enodes")), Term::Int(BigInt::from(egg_enodes))),
                        (TermOrdKey(Term::symbol(":egg-iterations")), Term::Int(BigInt::from(egg_iterations))),
                        (
                            TermOrdKey(Term::symbol(":egg-rewrites")),
                            Term::Vector(
                                rewrites
                                    .iter()
                                    .map(|(name, count)| {
                                        Term::Map(
                                            [
                                                (TermOrdKey(Term::symbol(":n")), Term::Int(BigInt::from(*count))),
                                                (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                                            ]
                                            .into_iter()
                                            .collect(),
                                        )
                                    })
                                    .collect(),
                            ),
                        ),
                        (TermOrdKey(Term::symbol(":egg-runs")), Term::Int(BigInt::from(egg_runs))),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":stage2")),
                Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":entries")), Term::Vector(stage2_entries)),
                        (TermOrdKey(Term::symbol(":supported-modules")), Term::Int(BigInt::from(stage2_supported))),
                        (TermOrdKey(Term::symbol(":validated-modules")), Term::Int(BigInt::from(stage2_validated))),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_reports)),
        ]
        .into_iter()
        .collect(),
    );
    (ok, errors, report)
}

fn decode_translation_result(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observation: &TranslationObservation,
    request_hash: [u8; 32],
    term: Term,
) -> Result<ObligationResult, ObligationError> {
    let map = exact_map(
        &term,
        "translation authority result",
        &[":errors", ":kind", ":name", ":ok", ":operation", ":report", ":request-h", ":v"],
    )?;
    if string_field(map, ":kind", "translation authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "translation authority result")?
            != "core/obligation::translation-validation"
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == ":translation-validation")
        || string_field(map, ":request-h", "translation authority result")? != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("translation result identity mismatch"));
    }
    let expected = translation_expected(manifest, observation);
    let errors = string_vector(
        required_field(map, ":errors", "translation authority result")?,
        "translation authority result :errors",
    )?;
    let report = required_field(map, ":report", "translation authority result")?;
    if bool_field(map, ":ok", "translation authority result")? != expected.0
        || errors != expected.1
        || report != &expected.2
    {
        return Err(authority_error(
            "translation authority result contradicts execution observations",
        ));
    }
    let artifact = store.put_term(report)?;
    Ok(ObligationResult {
        name: "core/obligation::translation-validation".to_string(),
        ok: expected.0,
        artifact: Some(artifact),
        errors,
    })
}
