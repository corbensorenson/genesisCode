#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Stage1Observation {
    pub(super) path: String,
    pub(super) original_module_hash: [u8; 32],
    pub(super) transformed_module_hash: [u8; 32],
    pub(super) original_value_hash: Option<[u8; 32]>,
    pub(super) transformed_value_hash: Option<[u8; 32]>,
    pub(super) original_eval_error: Option<String>,
    pub(super) transformed_eval_error: Option<String>,
    pub(super) egg_runs: u64,
    pub(super) egg_iterations: u64,
    pub(super) egg_eclasses: u64,
    pub(super) egg_enodes: u64,
}

fn stage1_optimizer_term(observation: &Stage1Observation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":egg-eclasses")),
                Term::Int(BigInt::from(observation.egg_eclasses)),
            ),
            (
                TermOrdKey(Term::symbol(":egg-enodes")),
                Term::Int(BigInt::from(observation.egg_enodes)),
            ),
            (
                TermOrdKey(Term::symbol(":egg-iterations")),
                Term::Int(BigInt::from(observation.egg_iterations)),
            ),
            (
                TermOrdKey(Term::symbol(":egg-runs")),
                Term::Int(BigInt::from(observation.egg_runs)),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn stage1_observation_term(observation: &Stage1Observation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":optimizer")),
                stage1_optimizer_term(observation),
            ),
            (
                TermOrdKey(Term::symbol(":original-eval-error")),
                observation
                    .original_eval_error
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":original-module-h")),
                Term::Bytes(observation.original_module_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":transformed-eval-error")),
                observation
                    .transformed_eval_error
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":original-value-h")),
                optional_hash_term(observation.original_value_hash),
            ),
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Str(observation.path.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":transformed-module-h")),
                Term::Bytes(observation.transformed_module_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":transformed-value-h")),
                optional_hash_term(observation.transformed_value_hash),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn stage1_inputs(observations: &[Stage1Observation]) -> Term {
    Term::Map(
        [(
            TermOrdKey(Term::symbol(":modules")),
            Term::Vector(observations.iter().map(stage1_observation_term).collect()),
        )]
        .into_iter()
        .collect(),
    )
}

fn stage1_expected(
    manifest: &PackageManifest,
    observations: &[Stage1Observation],
) -> (bool, Vec<String>, Term) {
    let mut errors = Vec::new();
    let modules = observations
        .iter()
        .map(|observation| {
            let mut module_errors = Vec::new();
            if let Some(error) = &observation.original_eval_error {
                module_errors.push(format!("original module is not gate-valid: {error}"));
            }
            if let Some(error) = &observation.transformed_eval_error {
                module_errors.push(format!("transformed module is not gate-valid: {error}"));
            }
            if observation.original_eval_error.is_none()
                && observation.transformed_eval_error.is_none()
                && observation.original_value_hash != observation.transformed_value_hash
            {
                module_errors.push("pure value hash mismatch after stage1 transform".to_string());
            }
            errors.extend(
                module_errors
                    .iter()
                    .map(|error| format!("{}: {error}", observation.path)),
            );
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":errors")),
                        Term::Vector(
                            module_errors.iter().cloned().map(Term::Str).collect(),
                        ),
                    ),
                    (
                        TermOrdKey(Term::symbol(":ok")),
                        Term::Bool(module_errors.is_empty()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":optimizer")),
                        stage1_optimizer_term(observation),
                    ),
                    (
                        TermOrdKey(Term::symbol(":original-module-h")),
                        Term::Bytes(observation.original_module_hash.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":original-value-h")),
                        optional_hash_term(observation.original_value_hash),
                    ),
                    (
                        TermOrdKey(Term::symbol(":path")),
                        Term::Str(observation.path.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":transformed-module-h")),
                        Term::Bytes(observation.transformed_module_hash.to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":transformed-value-h")),
                        optional_hash_term(observation.transformed_value_hash),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    let ok = errors.is_empty();
    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/stage1-validation-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":modules")), Term::Vector(modules)),
            (
                TermOrdKey(Term::symbol(":obligation")),
                Term::Str("core/obligation::stage1-validation".to_string()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    (ok, errors, report)
}

fn decode_stage1_result(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observations: &[Stage1Observation],
    request_hash: [u8; 32],
    term: Term,
) -> Result<ObligationResult, ObligationError> {
    let map = exact_map(
        &term,
        "stage1 authority result",
        &[
            ":errors",
            ":kind",
            ":name",
            ":ok",
            ":operation",
            ":report",
            ":request-h",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "stage1 authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "stage1 authority result")?
            != "core/obligation::stage1-validation"
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == ":stage1-validation")
        || string_field(map, ":request-h", "stage1 authority result")? != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("stage1 result identity mismatch"));
    }
    let expected = stage1_expected(manifest, observations);
    let outer_errors = string_vector(
        required_field(map, ":errors", "stage1 authority result")?,
        "stage1 authority result :errors",
    )?;
    if bool_field(map, ":ok", "stage1 authority result")? != expected.0
        || outer_errors != expected.1
        || required_field(map, ":report", "stage1 authority result")? != &expected.2
    {
        return Err(authority_error(
            "stage1 authority result contradicts optimizer observations",
        ));
    }
    let artifact = store.put_term(&expected.2)?;
    Ok(ObligationResult {
        name: "core/obligation::stage1-validation".to_string(),
        ok: expected.0,
        artifact: Some(artifact),
        errors: expected.1,
    })
}

pub(super) fn evaluate_stage1_obligation_with_authority(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observations: &[Stage1Observation],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let request = authority_request_term(
        ObligationAuthorityOperation::Stage1Validation,
        &manifest.name,
        stage1_inputs(observations),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    decode_stage1_result(store, manifest, observations, request_hash, result)
}
