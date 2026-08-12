pub(super) fn evaluate_obligation_with_authority(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let request = request_term(operation, store, manifest, modules, tests)?;
    let request_hash = hash_term(&request);
    let term = invoke_authority(request, frontend, limits)?;
    decode_authority_result(
        operation,
        store,
        manifest,
        modules,
        tests,
        &[],
        request_hash,
        term,
    )
}
