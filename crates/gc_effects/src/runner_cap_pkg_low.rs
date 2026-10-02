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
    refs_authority: Option<&mut RefsAuthority>,
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
    let mut refs_authority = refs_authority;
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
            refs_authority.as_deref_mut(),
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
            refs_authority.as_deref_mut(),
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

pub(super) fn read_bounded_lock(
    file: &crate::runner_io_ops::DocumentRead,
) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let mut bytes = Vec::new();
    file.reader()
        .take(MAX_LOCK_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read lock file".to_string())?;
    if bytes.len() as u64 > MAX_LOCK_BYTES {
        return Err("lock file exceeds 4 MiB".to_string());
    }
    Ok(bytes)
}

#[cfg(any(test, feature = "parity-oracle"))]
fn read_parity_lock(
    file: &crate::runner_io_ops::DocumentRead,
) -> Result<gc_pkg::GenesisLock, String> {
    let bytes = read_bounded_lock(file)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "lock file is not UTF-8".to_owned())?;
    gc_pkg::GenesisLock::from_toml_str(std::path::Path::new(file.description()), text)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_read_optional_absence_does_not_mask_root_admission() {
        let fixture = tempfile::tempdir().unwrap();
        assert!(
            sandbox_optional_document_read(fixture.path(), "missing/genesis.lock")
                .unwrap()
                .is_none()
        );
        assert!(
            sandbox_optional_document_read(&fixture.path().join("absent-root"), "genesis.lock")
                .is_err()
        );
        assert!(sandbox_optional_document_read(fixture.path(), "../outside/genesis.lock").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn lock_read_retains_inside_file_after_ancestor_replacement() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(root.join("locks")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("locks/genesis.lock"), b"inside").unwrap();
        std::fs::write(outside.join("genesis.lock"), b"outside").unwrap();
        let authorized = sandbox_document_read(&root, "locks/genesis.lock").unwrap();
        std::fs::rename(root.join("locks"), root.join("original-locks")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("locks")).unwrap();
        assert_eq!(read_bounded_lock(&authorized).unwrap(), b"inside");
        assert!(sandbox_document_read(&root, "locks/genesis.lock").is_err());
    }

    #[test]
    fn bounded_lock_reader_rejects_oversized_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("genesis.lock");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_LOCK_BYTES + 1).unwrap();

        assert_eq!(
            read_bounded_lock(&sandbox_document_read(dir.path(), "genesis.lock").unwrap())
                .unwrap_err(),
            "lock file exceeds 4 MiB"
        );
    }

    #[test]
    fn lock_read_accepts_exact_limit_and_absolute_inside_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("genesis.lock");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_LOCK_BYTES).unwrap();
        let held = sandbox_document_read(dir.path(), path.to_str().unwrap()).unwrap();
        assert_eq!(
            read_bounded_lock(&held).unwrap().len() as u64,
            MAX_LOCK_BYTES
        );
    }

    #[cfg(unix)]
    #[test]
    fn lock_read_retains_final_file_and_root_identity() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("genesis.lock"), b"inside").unwrap();
        std::fs::write(fixture.path().join("outside"), b"outside").unwrap();
        let held = sandbox_document_read(&root, "genesis.lock").unwrap();
        std::fs::remove_file(root.join("genesis.lock")).unwrap();
        std::os::unix::fs::symlink(fixture.path().join("outside"), root.join("genesis.lock"))
            .unwrap();
        std::fs::rename(&root, fixture.path().join("moved-root")).unwrap();
        assert_eq!(read_bounded_lock(&held).unwrap(), b"inside");
    }

    #[test]
    fn lock_read_parity_uses_the_same_bound_and_rejects_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("genesis.lock");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_LOCK_BYTES + 1)
            .unwrap();
        let held = sandbox_document_read(dir.path(), "genesis.lock").unwrap();
        assert_eq!(
            read_parity_lock(&held).unwrap_err(),
            "lock file exceeds 4 MiB"
        );
        std::fs::write(&path, [0xff]).unwrap();
        let held = sandbox_document_read(dir.path(), "genesis.lock").unwrap();
        assert_eq!(
            read_parity_lock(&held).unwrap_err(),
            "lock file is not UTF-8"
        );
        std::fs::write(&path, "version = 1\nworkspace = \"held\"\n").unwrap();
        let held = sandbox_document_read(dir.path(), "genesis.lock").unwrap();
        assert_eq!(read_parity_lock(&held).unwrap().workspace, "held");
    }
}
