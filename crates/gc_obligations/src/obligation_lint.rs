use super::*;

pub(super) fn obligation_lint(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_obligation_with_authority(
        ObligationAuthorityOperation::Lint,
        store,
        manifest,
        modules,
        &[],
        frontend,
        limits,
    )
}

pub(super) fn obligation_ai_style(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_obligation_with_authority(
        ObligationAuthorityOperation::AiStyle,
        store,
        manifest,
        modules,
        &[],
        frontend,
        limits,
    )
}
