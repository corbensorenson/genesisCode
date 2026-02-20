use std::collections::BTreeMap;
use std::path::Path;

use blake3::Hasher;
use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EffectProgram, EffectRequest, EvalCtx, SealId, Value, value_hash};
use gc_prelude::build_prelude;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::error::EffectsError;
use crate::lock::ExclusiveLock;
use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
use crate::policy::{CapsPolicy, OpPolicy};
use crate::refs::{RefsDb, SetInput, SetManyResult, SetResult};
use crate::runner_editor_host::{EditorHostRuntime, editor_host_call};
use crate::runner_gc_payload::{
    payload_gc_depth, payload_gc_include_lock, payload_gc_include_refs, payload_gc_lock,
    payload_gc_pins, payload_gc_quarantine, payload_gc_quarantine_dir, payload_gc_target,
    payload_gc_ttl_days,
};
use crate::runner_gfx_host::{GfxHostRuntime, gfx_host_call};
use crate::runner_gpk_payload::{
    GpkIncludeDeps, GpkIncludeEvidence, GpkMode, payload_data, payload_gpk_depth, payload_gpk_in,
    payload_gpk_include_deps, payload_gpk_include_evidence, payload_gpk_mode, payload_gpk_out,
    payload_gpk_refs, payload_gpk_root, payload_gpk_set_refs,
};
use crate::runner_gpu_host::{GpuHostRuntime, gpu_host_call};
use crate::runner_io_ops::{
    FsReadError, atomic_write_text, effective_base_dir, io_error_payload, path_to_slash,
    payload_path, read_file_with_optional_limit, sandbox_path_allow_missing, sandbox_path_read,
    sandbox_path_write, write_file_no_follow,
};
use crate::runner_pkg_payload::{
    payload_pkg_bool, payload_pkg_lock, payload_pkg_name, payload_pkg_policy,
    payload_pkg_publish_commit, payload_pkg_publish_depth, payload_pkg_publish_expected_old,
    payload_pkg_publish_policy, payload_pkg_publish_ref, payload_pkg_publish_remote,
    payload_pkg_registry, payload_pkg_registry_default, payload_pkg_selector, payload_pkg_strategy,
    payload_pkg_tag_policy, payload_pkg_update_policy, payload_pkg_workspace,
};
use crate::runner_refs_ops::{
    LocalRefSetRequest, local_refs_set_policy_gated, local_refs_validate_policy_gate,
    payload_refs_expected_old, payload_refs_hash, payload_refs_name, payload_refs_policy_hash,
    payload_refs_prefix,
};
use crate::runner_store_ops::{
    is_hex64, payload_store_artifact, payload_store_hash, payload_store_optional_hash,
    store_get_term, store_scan_hashes,
};
use crate::runner_sync_payload::{
    payload_sync_depth, payload_sync_force, payload_sync_refs, payload_sync_remote,
    payload_sync_roots, payload_sync_set_refs,
};
use crate::runner_task::{
    TaskBudgetState, TaskRuntime, enforce_task_policy_limits, task_runtime_call,
    task_schedule_event_for,
};
use crate::runner_timeout::{with_timeout, with_timeout_cancellable};
use crate::runner_vcs_payload::{
    payload_vcs_hash, payload_vcs_max, payload_vcs_opt_hash, payload_vcs_opt_sym_or_str,
    payload_vcs_out, payload_vcs_patch, payload_vcs_root, payload_vcs_store, payload_vcs_sym,
};
use crate::store::ArtifactStore;

#[path = "runner_cap_gc_gpk_low.rs"]
mod runner_cap_gc_gpk_low;
#[path = "runner_cap_pkg_low.rs"]
mod runner_cap_pkg_low;
#[path = "runner_cap_refs.rs"]
mod runner_cap_refs;
#[path = "runner_cap_store.rs"]
mod runner_cap_store;
#[path = "runner_cap_vcs_low.rs"]
mod runner_cap_vcs_low;
#[path = "runner_capability_dispatch.rs"]
mod runner_capability_dispatch;
#[path = "runner_gc_ops.rs"]
mod runner_gc_ops;
#[path = "runner_remote_ops.rs"]
mod runner_remote_ops;
#[path = "runner_response_budget.rs"]
mod runner_response_budget;
#[path = "runner_runtime_budget.rs"]
mod runner_runtime_budget;
#[path = "runner_vcs_pkg_helpers.rs"]
mod runner_vcs_pkg_helpers;
use runner_cap_gc_gpk_low::*;
use runner_cap_pkg_low::*;
use runner_cap_refs::*;
use runner_cap_store::*;
use runner_cap_vcs_low::*;
use runner_capability_dispatch::*;
use runner_gc_ops::*;
use runner_remote_ops::*;
use runner_response_budget::*;
use runner_runtime_budget::*;
use runner_vcs_pkg_helpers::*;

type GcStoreLock = ExclusiveLock;

const HARD_REMOTE_ARTIFACT_MAX_BYTES: usize = 32 * 1024 * 1024;
const HARD_SYNC_PULL_BATCH_MAX_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn set_force_wasi_remote_profile(enabled: bool) {
    runner_remote_ops::set_force_wasi_remote_profile(enabled);
}

#[derive(Debug, Clone, Default)]
struct ArtifactBudgetState {
    store_written_bytes: usize,
    log_artifact_written_bytes: usize,
}

struct HashingWriter<'a, W: std::io::Write> {
    inner: &'a mut W,
    hasher: blake3::Hasher,
}

impl<'a, W: std::io::Write> HashingWriter<'a, W> {
    fn new(inner: &'a mut W) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"GCv0.2\0gpk\0");
        Self { inner, hasher }
    }

    fn finish_hex(self) -> String {
        self.hasher.finalize().to_hex().to_string()
    }
}

impl<'a, W: std::io::Write> std::io::Write for HashingWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub struct RunResult {
    pub value: Value,
    pub log: EffectLog,
}

pub fn run(
    ctx: &mut EvalCtx,
    policy: &CapsPolicy,
    program: Value,
    program_hash: [u8; 32],
    toolchain: String,
) -> Result<RunResult, EffectsError> {
    let proto = ctx.protocol.ok_or(EffectsError::MissingProtocol)?;

    let store = match policy.artifact_store_dir() {
        Some(sd) => Some(ArtifactStore::open(sd)?),
        None => None,
    };

    let refs = match policy.refs_db_path() {
        Some(p) => Some(RefsDb::open(p)?),
        None => None,
    };

    let mut entries = Vec::new();
    let mut i: u64 = 0;
    let mut cur = program;
    let mut task_budget_state = TaskBudgetState::default();
    let mut task_runtime = TaskRuntime::default();
    let mut gfx_runtime = GfxHostRuntime::default();
    let mut gpu_runtime = GpuHostRuntime::default();
    let mut editor_runtime = EditorHostRuntime::default();
    let mut artifact_budget_state = ArtifactBudgetState::default();
    let mut runtime_budget_state = RuntimeBudgetState::default();

    loop {
        let Value::EffectProgram(p) = cur else {
            return Err(EffectsError::NotAnEffectProgram);
        };
        match p.as_ref() {
            EffectProgram::Pure(v) => {
                let log = EffectLog {
                    version: 3,
                    program_hash,
                    toolchain,
                    entries,
                };
                return Ok(RunResult {
                    value: (*v.as_ref()).clone(),
                    log,
                });
            }
            EffectProgram::Perform { request } => {
                let (req, sealed_token) = unseal_effect_request(request.as_ref(), proto.effect)?;
                if sealed_token != proto.effect {
                    return Err(EffectsError::BadEffectSeal);
                }

                let payload_h = hash_term(&req.payload);
                let cont_h = value_hash(&req.k);
                let req_h = hash_request(&req.op, payload_h, cont_h);

                let pre_limit = enforce_request_runtime_limits(
                    policy,
                    &mut runtime_budget_state,
                    &req.op,
                    &req.payload,
                    proto.error,
                );

                let (mut decision, mut cap_term, mut resp_val) = if let Some(limit_err) = pre_limit
                {
                    (Decision::Deny, Term::Nil, limit_err)
                } else if !policy.is_allowed(&req.op) {
                    (
                        Decision::Deny,
                        Term::Nil,
                        mk_caps_denied(proto.error, &req.op),
                    )
                } else {
                    let pol = policy.op_policy(&req.op);
                    let cap_term = cap_term(&req.op, pol)?;
                    let resp = if let Some(task_resp) = task_runtime_call(
                        &mut task_runtime,
                        policy,
                        &req.op,
                        &req.payload,
                        proto.error,
                    ) {
                        task_resp
                    } else if let Some(gfx_resp) =
                        gfx_host_call(&mut gfx_runtime, &req.op, &req.payload, pol, proto.error)
                    {
                        gfx_resp
                    } else if let Some(gpu_resp) =
                        gpu_host_call(&mut gpu_runtime, &req.op, &req.payload, pol, proto.error)
                    {
                        gpu_resp
                    } else if let Some(editor_resp) = editor_host_call(
                        &mut editor_runtime,
                        &req.op,
                        &req.payload,
                        pol,
                        proto.error,
                    ) {
                        editor_resp
                    } else {
                        call_capability(
                            &req.op,
                            &req.payload,
                            pol,
                            policy,
                            store.as_ref(),
                            refs.as_ref(),
                            &mut artifact_budget_state,
                            proto.error,
                        )?
                    };
                    (Decision::Allow, cap_term, resp)
                };

                if decision == Decision::Allow
                    && let Some(limit_err) = enforce_task_policy_limits(
                        policy,
                        &mut task_budget_state,
                        &task_runtime,
                        i,
                        &req.op,
                        &req.payload,
                        &resp_val,
                        proto.error,
                    )
                {
                    decision = Decision::Deny;
                    cap_term = Term::Nil;
                    resp_val = limit_err;
                };
                if decision == Decision::Allow
                    && let Some(limit_err) = enforce_log_artifact_budget(
                        policy,
                        &mut artifact_budget_state,
                        &req.op,
                        &resp_val,
                        proto.error,
                    )?
                {
                    resp_val = limit_err;
                }
                if let Some(limit_err) = enforce_response_runtime_limits(
                    policy,
                    &mut runtime_budget_state,
                    &req.op,
                    &resp_val,
                    proto.error,
                )? {
                    decision = Decision::Deny;
                    cap_term = Term::Nil;
                    resp_val = limit_err;
                }
                let resp_logged = logged_resp(policy, &req.op, &store, &resp_val, proto.error)?;

                let resp_h = value_hash(&resp_val);
                let task_event = task_schedule_event_for(i, &req.op, &req.payload, &resp_val);

                entries.push(EffectLogEntry {
                    i,
                    op: req.op.clone(),
                    payload_h,
                    cont_h,
                    req_h,
                    task_id: task_event.task_id,
                    parent_task: task_event.parent_task,
                    schedule_step: task_event.schedule_step,
                    await_edge: task_event.await_edge,
                    decision,
                    cap: cap_term,
                    resp: resp_logged,
                    resp_h,
                });
                i = i.saturating_add(1);

                // Apply continuation; allow auto-lifting a non-effect-program result into Pure.
                let k = (*req.k).clone();
                let next = k.apply(ctx, resp_val)?;
                cur = match next {
                    Value::EffectProgram(_) => next,
                    other => Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(other)))),
                };
            }
        }
    }
}

pub fn replay(ctx: &mut EvalCtx, program: Value, log: &EffectLog) -> Result<Value, EffectsError> {
    replay_with_store(ctx, program, log, None)
}

pub fn replay_with_store(
    ctx: &mut EvalCtx,
    program: Value,
    log: &EffectLog,
    store: Option<&ArtifactStore>,
) -> Result<Value, EffectsError> {
    let proto = ctx.protocol.ok_or(EffectsError::MissingProtocol)?;
    let mut cur = program;
    let mut idx: usize = 0;
    loop {
        let Value::EffectProgram(p) = cur else {
            return Err(EffectsError::NotAnEffectProgram);
        };
        match p.as_ref() {
            EffectProgram::Pure(v) => {
                if idx != log.entries.len() {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "program finished with {} remaining log entries",
                        log.entries.len().saturating_sub(idx)
                    )));
                }
                return Ok((*v.as_ref()).clone());
            }
            EffectProgram::Perform { request } => {
                let entry = log.entries.get(idx).ok_or_else(|| {
                    EffectsError::ReplayMismatch("log ended before program finished".to_string())
                })?;
                let (req, sealed_token) = unseal_effect_request(request.as_ref(), proto.effect)?;
                if sealed_token != proto.effect {
                    return Err(EffectsError::BadEffectSeal);
                }

                let payload_h = hash_term(&req.payload);
                let cont_h = value_hash(&req.k);
                let req_h = hash_request(&req.op, payload_h, cont_h);

                if entry.i != idx as u64 {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "entry index mismatch: expected {}, got {}",
                        idx, entry.i
                    )));
                }
                if entry.op != req.op {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "op mismatch at {idx}: expected {}, got {}",
                        req.op, entry.op
                    )));
                }
                if entry.payload_h != payload_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "payload hash mismatch at {idx}"
                    )));
                }
                if entry.cont_h != cont_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "continuation hash mismatch at {idx}"
                    )));
                }
                if entry.req_h != req_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "request hash mismatch at {idx}"
                    )));
                }

                let resp_val = resp_from_log(&entry.resp, store, proto.error)?;

                let resp_h = value_hash(&resp_val);
                if entry.resp_h != resp_h {
                    return Err(EffectsError::ReplayMismatch(format!(
                        "response hash mismatch at {idx}"
                    )));
                }
                if log.version >= 3 {
                    let expected =
                        task_schedule_event_for(idx as u64, &req.op, &req.payload, &resp_val);
                    if entry.schedule_step != expected.schedule_step {
                        return Err(EffectsError::ReplayMismatch(format!(
                            "schedule-step mismatch at {idx}: expected {:?}, got {:?}",
                            expected.schedule_step, entry.schedule_step
                        )));
                    }
                    if entry.task_id != expected.task_id {
                        return Err(EffectsError::ReplayMismatch(format!(
                            "task-id mismatch at {idx}: expected {:?}, got {:?}",
                            expected.task_id, entry.task_id
                        )));
                    }
                    if entry.parent_task != expected.parent_task {
                        return Err(EffectsError::ReplayMismatch(format!(
                            "parent-task mismatch at {idx}: expected {:?}, got {:?}",
                            expected.parent_task, entry.parent_task
                        )));
                    }
                    if entry.await_edge != expected.await_edge {
                        return Err(EffectsError::ReplayMismatch(format!(
                            "await-edge mismatch at {idx}: expected {:?}, got {:?}",
                            expected.await_edge, entry.await_edge
                        )));
                    }
                }

                let k = (*req.k).clone();
                let next = k.apply(ctx, resp_val)?;
                cur = match next {
                    Value::EffectProgram(_) => next,
                    other => Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(other)))),
                };

                idx += 1;
            }
        }
    }
}

#[cfg(all(test, not(target_os = "wasi")))]
mod remote_allow_tests {
    use super::remote_allow_matches;

    #[test]
    fn remote_allow_rejects_host_confusion() {
        assert!(
            !remote_allow_matches(
                "https://trusted.example.com.evil",
                "https://trusted.example.com"
            )
            .expect("allow check")
        );
    }

    #[test]
    fn remote_allow_accepts_exact_origin_and_path_prefix() {
        assert!(
            remote_allow_matches(
                "https://registry.example.com",
                "https://registry.example.com"
            )
            .expect("allow check")
        );
    }
}
