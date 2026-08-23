use super::*;
#[path = "runner_cap_pkg_low/dispatch_lock_io.rs"]
mod dispatch_lock_io;
#[path = "runner_cap_pkg_low/dispatch_publish.rs"]
mod dispatch_publish;
#[path = "runner_cap_pkg_low/dispatch_resolution.rs"]
mod dispatch_resolution;
#[path = "runner_cap_pkg_low/module_semantics.rs"]
mod module_semantics;

use module_semantics::{handle_load_package, handle_snapshot};

#[expect(
    clippy::too_many_arguments,
    reason = "host capability dispatch wiring keeps explicit context parameters visible"
)]
pub(super) fn capability_pkg_low(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    pkg_lock_read_authority: Option<&mut PkgLockReadAuthority>,
    pkg_lock_write_authority: Option<&mut PkgLockWriteAuthority>,
    pkg_resolution_identity_authority: Option<&mut PkgResolutionIdentityAuthority>,
    pkg_package_manifest_authority: Option<&mut PkgPackageManifestAuthority>,
    budget: &mut ArtifactBudgetState,
    bridge_runtime: &mut HostBridgeRuntime,
    error_tok: SealId,
    op: &str,
    _timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    if matches!(
        op_eff,
        "core/pkg-low::init"
            | "core/pkg-low::add"
            | "core/pkg-low::list"
            | "core/pkg-low::load-lock"
            | "core/pkg-low::load-package"
            | "core/pkg-low::save-lock"
    ) {
        return dispatch_lock_io::dispatch_lock_io(
            op_eff,
            payload,
            pol,
            policy,
            store,
            refs,
            pkg_lock_read_authority,
            pkg_lock_write_authority,
            pkg_package_manifest_authority,
            budget,
            error_tok,
            op,
            _timeout_ms,
        );
    }
    if matches!(
        op_eff,
        "core/pkg-low::info"
            | "core/pkg-low::lock"
            | "core/pkg-low::update"
            | "core/pkg-low::install"
            | "core/pkg-low::verify"
    ) {
        return dispatch_resolution::dispatch_resolution(
            op_eff,
            payload,
            pol,
            policy,
            store,
            refs,
            pkg_lock_read_authority,
            pkg_lock_write_authority,
            pkg_resolution_identity_authority,
            budget,
            error_tok,
            op,
            _timeout_ms,
        );
    }
    if matches!(
        op_eff,
        "core/pkg-low::snapshot" | "core/pkg-low::publish" | "core/pkg-low::bridge"
    ) {
        return dispatch_publish::dispatch_publish(
            op_eff,
            payload,
            pol,
            policy,
            store,
            refs,
            pkg_lock_read_authority,
            pkg_package_manifest_authority,
            budget,
            bridge_runtime,
            error_tok,
            op,
            _timeout_ms,
        );
    }
    Ok(mk_error(
        error_tok,
        "core/caps/unknown-op",
        format!("unknown capability op: {op}"),
        Some(op),
    ))
}

const MAX_LOCK_BYTES: u64 = 4 * 1024 * 1024;

fn read_bounded_lock(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|_| "cannot read lock file".to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_LOCK_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read lock file".to_string())?;
    if bytes.len() as u64 > MAX_LOCK_BYTES {
        return Err("lock file exceeds 4 MiB".to_string());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_lock_reader_rejects_oversized_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("genesis.lock");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_LOCK_BYTES + 1).unwrap();

        assert_eq!(
            read_bounded_lock(&path).unwrap_err(),
            "lock file exceeds 4 MiB"
        );
    }
}
