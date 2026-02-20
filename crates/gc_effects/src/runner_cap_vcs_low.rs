use super::*;
#[path = "runner_cap_vcs_low/dispatch_meta.rs"]
mod dispatch_meta;
#[path = "runner_cap_vcs_low/dispatch_patch_contract.rs"]
mod dispatch_patch_contract;
#[path = "runner_cap_vcs_low/dispatch_snapshot.rs"]
mod dispatch_snapshot;

#[expect(
    clippy::too_many_arguments,
    reason = "host capability dispatch wiring keeps explicit context parameters visible"
)]
pub(super) fn capability_vcs_low(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    if matches!(
        op_eff,
        "core/vcs-low::log" | "core/vcs-low::blame" | "core/vcs-low::why"
    ) {
        return dispatch_meta::dispatch_meta(
            op_eff, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        );
    }
    if matches!(
        op_eff,
        "core/vcs-low::diff-terms"
            | "core/vcs-low::apply-patch"
            | "core/vcs-low::merge3-contract-snapshots"
            | "core/vcs-low::resolve-conflict"
    ) {
        return dispatch_patch_contract::dispatch_patch_contract(
            op_eff, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        );
    }
    if matches!(
        op_eff,
        "core/vcs-low::diff" | "core/vcs-low::apply" | "core/vcs-low::merge3"
    ) {
        return dispatch_snapshot::dispatch_snapshot(
            op_eff, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        );
    }
    Ok(mk_error(
        error_tok,
        "core/caps/unknown-op",
        format!("unknown capability op: {op}"),
        Some(op),
    ))
}
