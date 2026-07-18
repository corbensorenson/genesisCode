mod error;
mod lock;
mod log;
mod policy;
mod refs;
mod runner;
mod runner_browser_host;
mod runner_editor_host;
mod runner_ffi_schema;
mod runner_gc_payload;
mod runner_gfx_host;
mod runner_gpk_payload;
mod runner_gpu_backend_policy;
mod runner_gpu_device_backend;
mod runner_gpu_host;
mod runner_host_bridge;
mod runner_io_ops;
mod runner_pkg_payload;
mod runner_plugin_schema;
mod runner_process_control;
mod runner_refs_ops;
mod runner_store_ops;
mod runner_sync_payload;
mod runner_task;
mod runner_task_exec;
mod runner_timeout;
mod runner_vcs_payload;
mod runner_xr_host;
mod store;

pub use crate::error::EffectsError;
pub use crate::log::{
    Decision, EffectLog, EffectLogEntry, GCLOG_CURRENT_VERSION, GCLOG_LEGACY_VERSION,
    GCLOG_PROFILE_ID, LoggedResp,
};
pub use crate::policy::{CapsPolicy, OpPolicy};
pub use crate::refs::{RefEntry, RefsDb, SetResult};
pub use crate::runner::{RunResult, replay, replay_with_store, run};
pub use crate::store::ArtifactStore;

#[cfg(not(target_os = "wasi"))]
pub(crate) fn platform_process_id() -> u32 {
    std::process::id()
}

#[cfg(target_os = "wasi")]
pub(crate) fn platform_process_id() -> u32 {
    0
}

pub fn set_force_wasi_remote_profile(enabled: bool) {
    runner::set_force_wasi_remote_profile(enabled);
}

/// Tighten the effect-operation ceiling for an isolated agent-session worker.
///
/// `None` clears the session ceiling. Capability policy remains authoritative;
/// when both limits exist, the lower limit wins.
pub fn set_session_effect_ceiling(limit: Option<u64>) {
    runner::set_session_effect_ceiling(limit);
}

#[cfg(test)]
mod tests;
