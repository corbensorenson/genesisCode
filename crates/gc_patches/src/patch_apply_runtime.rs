use super::*;

pub(super) fn initialize_patch_toolchain(
    frontend: &CoreformFrontend,
    mem_limits: MemLimits,
) -> Result<Option<SelfhostPatchToolchain>, PatchError> {
    if coreform_frontend_is_rust(frontend) {
        #[cfg(not(feature = "parity-oracle"))]
        return Err(PatchError::Validate(
            "patch apply requires artifact-only GenesisCode semantic and report authority"
                .to_string(),
        ));
        #[cfg(feature = "parity-oracle")]
        return Ok(None);
    }
    let CoreformFrontend::Selfhost(config) = frontend else {
        return Err(PatchError::Validate(
            "invalid frontend dispatch while initializing patch toolchain".to_string(),
        ));
    };
    SelfhostPatchToolchain::init(config, mem_limits).map(Some)
}
