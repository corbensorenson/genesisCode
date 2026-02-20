mod error;
mod lock;
mod log;
mod policy;
mod refs;
mod runner;
mod runner_editor_host;
mod runner_gc_payload;
mod runner_gfx_host;
mod runner_gpk_payload;
mod runner_gpu_host;
mod runner_host_bridge;
mod runner_io_ops;
mod runner_pkg_payload;
mod runner_refs_ops;
mod runner_store_ops;
mod runner_sync_payload;
mod runner_task;
mod runner_task_exec;
mod runner_timeout;
mod runner_vcs_payload;
mod store;

pub use crate::error::EffectsError;
pub use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
pub use crate::policy::{CapsPolicy, OpPolicy};
pub use crate::refs::{RefEntry, RefsDb, SetResult};
pub use crate::runner::{RunResult, replay, replay_with_store, run};
pub use crate::store::ArtifactStore;

pub fn set_force_wasi_remote_profile(enabled: bool) {
    runner::set_force_wasi_remote_profile(enabled);
}

#[cfg(test)]
mod tests;
