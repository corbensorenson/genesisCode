use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use gc_prelude::SelfhostBootstrapMode;

use crate::ObligationError;

pub(super) const SELFHOST_TOOLCHAIN_ARTIFACT_ENV: &str = "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT";
pub(super) const SELFHOST_ONLY_ENV: &str = "GENESIS_SELFHOST_ONLY";
const DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = ".genesis/selfhost/toolchain.gc";
const WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = "selfhost/toolchain.gc";

#[derive(Debug, Clone)]
pub struct SelfhostFrontendConfig {
    pub bootstrap_mode: SelfhostBootstrapMode,
    pub artifact: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum CoreformFrontend {
    Rust,
    Selfhost(SelfhostFrontendConfig),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrontendRuntimeProfile {
    Production = 0,
    ParityHarness = 1,
}

static FRONTEND_RUNTIME_PROFILE: AtomicU8 = AtomicU8::new(FrontendRuntimeProfile::Production as u8);

pub fn set_frontend_runtime_profile_parity_harness(enabled: bool) {
    let value = if enabled {
        FrontendRuntimeProfile::ParityHarness as u8
    } else {
        FrontendRuntimeProfile::Production as u8
    };
    FRONTEND_RUNTIME_PROFILE.store(value, Ordering::Relaxed);
}

fn default_selfhost_artifact_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL)
}

fn workspace_selfhost_artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL)
}

fn resolved_selfhost_artifact_for_frontend() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var(SELFHOST_TOOLCHAIN_ARTIFACT_ENV) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    let p = default_selfhost_artifact_path();
    if p.is_file() {
        return Some(p);
    }
    let wp = workspace_selfhost_artifact_path();
    if wp.is_file() {
        return Some(wp);
    }
    None
}

pub fn default_coreform_frontend() -> CoreformFrontend {
    CoreformFrontend::Selfhost(SelfhostFrontendConfig {
        bootstrap_mode: SelfhostBootstrapMode::ArtifactOnly,
        artifact: resolved_selfhost_artifact_for_frontend(),
    })
}

pub(super) fn frontend_is_rust(frontend: &CoreformFrontend) -> bool {
    matches!(frontend, CoreformFrontend::Rust)
}

pub(super) fn env_truthy(name: &str) -> bool {
    fn is_truthy(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }
    std::env::var(name)
        .ok()
        .map(|v| is_truthy(&v))
        .unwrap_or(false)
}

pub(super) fn rust_frontend_compat_enabled() -> bool {
    FRONTEND_RUNTIME_PROFILE.load(Ordering::Relaxed) == FrontendRuntimeProfile::ParityHarness as u8
}

pub(super) fn non_artifact_bootstrap_modes_allowed() -> bool {
    FRONTEND_RUNTIME_PROFILE.load(Ordering::Relaxed) == FrontendRuntimeProfile::ParityHarness as u8
}

fn bootstrap_mode_label(mode: SelfhostBootstrapMode) -> &'static str {
    match mode {
        SelfhostBootstrapMode::ArtifactOnly => "artifact-only",
        SelfhostBootstrapMode::ArtifactPreferred => "artifact-preferred",
        SelfhostBootstrapMode::Embedded => "embedded",
    }
}

pub(super) fn enforce_frontend_bootstrap_mode_with_flag(
    frontend: &CoreformFrontend,
    context: &str,
    allow_non_artifact_bootstrap_modes: bool,
) -> Result<(), ObligationError> {
    let CoreformFrontend::Selfhost(cfg) = frontend else {
        return Ok(());
    };
    if cfg.bootstrap_mode == SelfhostBootstrapMode::ArtifactOnly
        || allow_non_artifact_bootstrap_modes
    {
        return Ok(());
    }
    Err(ObligationError::Module(format!(
        "non-artifact selfhost bootstrap mode `{}` is development-only in {context}; use artifact-only",
        bootstrap_mode_label(cfg.bootstrap_mode)
    )))
}

pub(super) fn enforce_frontend_allowed_with_flag(
    frontend: &CoreformFrontend,
    context: &str,
    selfhost_only: bool,
    rust_compat_enabled: bool,
) -> Result<(), ObligationError> {
    enforce_frontend_bootstrap_mode_with_flag(
        frontend,
        context,
        non_artifact_bootstrap_modes_allowed(),
    )?;
    if selfhost_only && matches!(frontend, CoreformFrontend::Rust) {
        return Err(ObligationError::Module(format!(
            "selfhost-only mode forbids Rust frontend in {context}; use CoreformFrontend::Selfhost"
        )));
    }
    if !rust_compat_enabled && matches!(frontend, CoreformFrontend::Rust) {
        return Err(ObligationError::Module(format!(
            "Rust frontend is disabled in this profile in {context}; use dedicated parity harness binaries for CLI compatibility workflows"
        )));
    }
    Ok(())
}

pub(super) fn enforce_frontend_allowed(
    frontend: &CoreformFrontend,
    context: &str,
) -> Result<(), ObligationError> {
    enforce_frontend_allowed_with_flag(
        frontend,
        context,
        env_truthy(SELFHOST_ONLY_ENV),
        rust_frontend_compat_enabled(),
    )
}
