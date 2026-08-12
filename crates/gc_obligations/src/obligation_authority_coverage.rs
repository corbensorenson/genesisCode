use crate::obligation_exec::{CoverageAuthorityObservation, CoverageProfile};

fn coverage_operation(profile: CoverageProfile) -> ObligationAuthorityOperation {
    match profile {
        CoverageProfile::Symbol => ObligationAuthorityOperation::Coverage,
        CoverageProfile::Decision => ObligationAuthorityOperation::CoverageDecision,
        CoverageProfile::Mcdc => ObligationAuthorityOperation::CoverageMcdc,
    }
}

fn decode_coverage_result(
    store: &EvidenceStore,
    operation: ObligationAuthorityOperation,
    observation: &CoverageAuthorityObservation,
    request_hash: [u8; 32],
    term: Term,
) -> Result<ObligationResult, ObligationError> {
    let map = exact_map(
        &term,
        "coverage authority result",
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
    if string_field(map, ":kind", "coverage authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "coverage authority result")?
            != operation.obligation_name()
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == operation.symbol())
        || string_field(map, ":request-h", "coverage authority result")? != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("coverage result identity mismatch"));
    }
    let errors = string_vector(
        required_field(map, ":errors", "coverage authority result")?,
        "coverage authority result :errors",
    )?;
    let report = required_field(map, ":report", "coverage authority result")?;
    if bool_field(map, ":ok", "coverage authority result")? != observation.expected_ok
        || errors != observation.expected_errors
        || report != &observation.expected_report
    {
        return Err(authority_error(
            "coverage authority result contradicts instrumentation observations",
        ));
    }
    let artifact = store.put_term(report)?;
    Ok(ObligationResult {
        name: operation.obligation_name().to_string(),
        ok: observation.expected_ok,
        artifact: Some(artifact),
        errors: observation.expected_errors.clone(),
    })
}

pub(super) fn evaluate_coverage_obligation_with_authority(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    profile: CoverageProfile,
    obligation_name: &str,
    observation: &CoverageAuthorityObservation,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let operation = coverage_operation(profile);
    if operation.obligation_name() != obligation_name {
        return Err(authority_error("coverage profile/name mismatch"));
    }
    let request = authority_request_term(operation, &manifest.name, observation.inputs.clone());
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, limits)?;
    decode_coverage_result(store, operation, observation, request_hash, result)
}
