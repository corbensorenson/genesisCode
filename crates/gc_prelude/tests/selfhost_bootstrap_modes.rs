use gc_kernel::EvalCtx;
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
    set_bootstrap_runtime_profile_parity_harness,
};
use std::sync::{Mutex, OnceLock};

fn missing_artifact_path() -> std::path::PathBuf {
    let td = tempfile::tempdir().expect("tempdir");
    td.path().join("missing-selfhost-toolchain.gc")
}

fn bootstrap_profile_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct BootstrapProfileGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl BootstrapProfileGuard {
    fn parity_harness() -> Self {
        let guard = bootstrap_profile_lock().lock().expect("lock bootstrap profile");
        set_bootstrap_runtime_profile_parity_harness(true);
        Self { _guard: guard }
    }
}

impl Drop for BootstrapProfileGuard {
    fn drop(&mut self) {
        set_bootstrap_runtime_profile_parity_harness(false);
    }
}

#[test]
fn artifact_only_mode_rejects_missing_artifact_without_fallback() {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let missing = missing_artifact_path();
    let err = load_selfhost_coreform_toolchain_v1_with_mode(
        &mut ctx,
        &mut env,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&missing),
    )
    .expect_err("artifact-only should fail when artifact is missing");
    let msg = err.to_string();
    assert!(
        msg.contains("selfhost artifact bootstrap required"),
        "unexpected error: {msg}"
    );
    assert!(msg.contains("missing-selfhost-toolchain.gc"));
}

#[cfg(not(feature = "embedded-bootstrap"))]
#[test]
fn artifact_preferred_mode_reports_embedded_bootstrap_is_disabled() {
    let _profile = BootstrapProfileGuard::parity_harness();
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let missing = missing_artifact_path();
    let err = load_selfhost_coreform_toolchain_v1_with_mode(
        &mut ctx,
        &mut env,
        SelfhostBootstrapMode::ArtifactPreferred,
        Some(&missing),
    )
    .expect_err("artifact-preferred should fail when embedded fallback is disabled");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("embedded selfhost bootstrap is disabled at compile time"),
        "unexpected error: {msg}"
    );
}
