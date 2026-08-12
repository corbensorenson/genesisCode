use super::*;

pub(crate) mod helpers;

use crate::obligation_authority::{
    evaluate_gfx_frame_budgets_with_authority, evaluate_gfx_golden_with_authority,
};

pub(super) fn obligation_gfx_golden_images(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_gfx_golden_with_authority(store, pkg_dir, manifest, modules, frontend, limits)
}

pub(super) fn obligation_gfx_frame_budgets(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    evaluate_gfx_frame_budgets_with_authority(store, pkg_dir, manifest, modules, frontend, limits)
}
