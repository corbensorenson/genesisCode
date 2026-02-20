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
    budget: &mut ArtifactBudgetState,
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
            budget,
            error_tok,
            op,
            _timeout_ms,
        );
    }
    if matches!(op_eff, "core/pkg-low::snapshot" | "core/pkg-low::publish") {
        return dispatch_publish::dispatch_publish(
            op_eff,
            payload,
            pol,
            policy,
            store,
            refs,
            budget,
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
