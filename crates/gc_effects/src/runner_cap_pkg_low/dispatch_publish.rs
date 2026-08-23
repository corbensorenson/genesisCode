use super::*;
use crate::pkg_lock_read_authority::PkgLockReadAuthority;

#[path = "dispatch_publish/bridge_lock.rs"]
mod bridge_lock;
#[path = "dispatch_publish/bridge_objects.rs"]
mod bridge_objects;
#[path = "dispatch_publish/publish_authority.rs"]
mod publish_authority;

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_publish(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    refs_authority: Option<&mut RefsAuthority>,
    pkg_lock_read_authority: Option<&mut PkgLockReadAuthority>,
    pkg_package_manifest_authority: Option<&mut PkgPackageManifestAuthority>,
    budget: &mut ArtifactBudgetState,
    bridge_runtime: &mut HostBridgeRuntime,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let _ = timeout_ms;
    match op_eff {
        "core/pkg-low::snapshot" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::snapshot".to_string())
            })?;
            handle_snapshot(
                payload,
                pol,
                policy,
                store,
                pkg_lock_read_authority,
                pkg_package_manifest_authority,
                budget,
                error_tok,
                op,
            )
        }
        "core/pkg-low::publish" => publish_authority::handle_publish(
            payload,
            pol,
            policy,
            store,
            refs,
            refs_authority,
            pkg_lock_read_authority,
            budget,
            bridge_runtime,
            error_tok,
            op,
        ),
        "core/pkg-low::bridge" => bridge_objects::dispatch_bridge(
            payload,
            pol,
            policy,
            store,
            refs,
            pkg_lock_read_authority,
            budget,
            bridge_runtime,
            error_tok,
            op,
        ),
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/pkg-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}
