use super::*;

pub(super) fn obligation_budgets(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Budgets,
        store,
        manifest,
        &[],
        tests,
        frontend,
        limits,
    )
}
