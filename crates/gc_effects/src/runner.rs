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
    payload_path, payload_pkg_path, read_file_with_optional_limit, sandbox_path_allow_missing,
    sandbox_path_read, sandbox_path_write, write_file_no_follow,
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

#[path = "runner_gc_ops.rs"]
mod runner_gc_ops;
#[path = "runner_remote_ops.rs"]
mod runner_remote_ops;
#[path = "runner_response_budget.rs"]
mod runner_response_budget;
#[path = "runner_vcs_pkg_helpers.rs"]
mod runner_vcs_pkg_helpers;
use runner_gc_ops::*;
use runner_remote_ops::*;
use runner_response_budget::*;
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

                let (mut decision, mut cap_term, mut resp_val) = if !policy.is_allowed(&req.op) {
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

fn call_capability(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let op_eff = dispatch_op_alias(op);
    let timeout_ms = pol.and_then(|p| p.timeout_ms).filter(|ms| *ms > 0);
    if timeout_ms.is_some() && op_eff == "io/fs::write" {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            "timeout_ms is not supported for io/fs::write (mutating op)".to_string(),
            Some(op),
        ));
    }
    match op_eff {
        "core/sync::pull" => capability_sync_pull(
            payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        ),

        "core/sync::push" => capability_sync_push(payload, pol, store, error_tok, op, timeout_ms),

        "core/pkg-low::init" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let workspace = match payload_pkg_workspace(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let policy_s =
                payload_pkg_policy(payload).unwrap_or_else(|| "policy:default-v0.1".to_string());
            let reg_default = payload_pkg_registry_default(payload);

            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let lock_path = match sandbox_path_write(&base_dir, &lock_s, create_dirs) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut l = gc_pkg::GenesisLock::empty(workspace);
            l.policy = policy_s;
            if let Some(rd) = reg_default {
                l.registries.insert("default".to_string(), rd);
            }
            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            if let Err(e) = atomic_write_text(&lock_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::add" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let name = match payload_pkg_name(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let selector = match payload_pkg_selector(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let update_policy = match payload_pkg_update_policy(payload) {
                Ok(Some(p)) => p,
                Ok(None) => gc_pkg::UpdatePolicy::Manual,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let strategy = match payload_pkg_strategy(payload) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let tag_policy = match payload_pkg_tag_policy(payload) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let registry = payload_pkg_registry(payload);

            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            l.set_requirement_with_metadata(
                &name,
                &selector,
                update_policy,
                registry,
                strategy,
                tag_policy,
            );
            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::list" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut reqs = Vec::new();
            for (name, r) in &l.requirements {
                let mut mm = BTreeMap::new();
                mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(r.selector.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::Symbol(match r.update_policy {
                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                    }),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":strategy")),
                    Term::Symbol(format!(":{}", r.strategy.as_str())),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":tag-policy")),
                    r.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                reqs.push(Term::Map(mm));
            }

            let mut locks = Vec::new();
            for (name, le) in &l.locked {
                let mut mm = BTreeMap::new();
                mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":commit")),
                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str(le.snapshot.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":environment-fingerprint")),
                    le.environment_fingerprint
                        .clone()
                        .map(Term::Str)
                        .unwrap_or(Term::Nil),
                );
                locks.push(Term::Map(mm));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":requirements")),
                Term::Vector(reqs),
            );
            m.insert(TermOrdKey(Term::symbol(":locked")), Term::Vector(locks));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::load-lock" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut reqs: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, r) in &l.requirements {
                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(r.selector.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::Symbol(match r.update_policy {
                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                    }),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                reqs.insert(TermOrdKey(Term::Str(name.clone())), Term::Map(mm));
            }

            let mut locked: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, le) in &l.locked {
                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":commit")),
                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str(le.snapshot.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    le.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":source_selector")),
                    if le.source_selector.is_empty() {
                        Term::Nil
                    } else {
                        Term::Str(le.source_selector.clone())
                    },
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":resolved-ref")),
                    le.resolved_ref.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":exports_hash")),
                    le.exports_hash.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                locked.insert(TermOrdKey(Term::Str(name.clone())), Term::Map(mm));
            }

            let mut registries: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, url) in &l.registries {
                registries.insert(TermOrdKey(Term::Str(name.clone())), Term::Str(url.clone()));
            }
            let mut artifacts: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, h) in &l.artifacts {
                artifacts.insert(TermOrdKey(Term::Str(name.clone())), Term::Str(h.clone()));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(l.workspace),
            );
            m.insert(TermOrdKey(Term::symbol(":policy")), Term::Str(l.policy));
            m.insert(TermOrdKey(Term::symbol(":requirements")), Term::Map(reqs));
            m.insert(TermOrdKey(Term::symbol(":locked")), Term::Map(locked));
            m.insert(
                TermOrdKey(Term::symbol(":registries")),
                Term::Map(registries),
            );
            m.insert(TermOrdKey(Term::symbol(":artifacts")), Term::Map(artifacts));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/pkg-low::load-package" => {
            let pkg_path_s = match payload_pkg_path(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let pkg_path = sandbox_path_read(&base_dir, &pkg_path_s)?;

            let (manifest, pkg_dir) = match gc_pkg::PackageManifest::load(&pkg_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/manifest-error",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let base = std::fs::canonicalize(&base_dir)?;
            let pkg_dir_resolved = std::fs::canonicalize(&pkg_dir)?;
            if !pkg_dir_resolved.starts_with(&base) {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/path-escape",
                    "package directory escapes base dir".to_string(),
                    Some(op),
                ));
            }

            let mut modules_out: Vec<Term> = Vec::new();
            for me in &manifest.modules {
                let module_fs_path = pkg_dir.join(&me.path);
                let resolved = match std::fs::canonicalize(&module_fs_path) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                if !resolved.starts_with(&base) {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("module escapes base dir: {}", me.path),
                        Some(op),
                    ));
                }
                let src = match std::fs::read_to_string(&resolved) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                let forms = match gc_coreform::parse_module(&src) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/parse-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let forms = match gc_coreform::canonicalize_module(forms) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/canon-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let module_h = gc_coreform::hash_module(&forms);
                if let Some(want_hex) = &me.hash {
                    if want_hex.len() != 64 || !want_hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-hash",
                            format!("manifest module hash is not 64-hex: {}", me.path),
                            Some(op),
                        ));
                    }
                    let got_hex = blake3::Hash::from_bytes(module_h).to_hex().to_string();
                    if &got_hex != want_hex {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/hash-mismatch",
                            format!("module hash mismatch: {}", me.path),
                            Some(op),
                        ));
                    }
                }

                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(me.path.clone()),
                );
                mm.insert(TermOrdKey(Term::symbol(":module")), Term::Vector(forms));
                mm.insert(
                    TermOrdKey(Term::symbol(":module-h")),
                    Term::Bytes(module_h.to_vec().into()),
                );
                modules_out.push(Term::Map(mm));
            }

            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":pkg")), Term::Str(pkg_path_s));
            out.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Str(manifest.name.clone()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":version")),
                Term::Str(manifest.version.clone()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    manifest
                        .obligations
                        .iter()
                        .cloned()
                        .map(Term::Symbol)
                        .collect(),
                ),
            );
            out.insert(
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(modules_out),
            );
            Ok(Value::Data(Term::Map(out)))
        }
        "core/pkg-low::save-lock" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let workspace = match m.get(&TermOrdKey(Term::symbol(":workspace"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":workspace must be string".to_string(),
                        Some(op),
                    ));
                }
            };
            let policy_s = match m.get(&TermOrdKey(Term::symbol(":policy"))) {
                Some(Term::Str(s)) => s.clone(),
                Some(Term::Nil) | None => "policy:default-v0.1".to_string(),
                _ => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":policy must be string or nil".to_string(),
                        Some(op),
                    ));
                }
            };
            let version = match m.get(&TermOrdKey(Term::symbol(":version"))) {
                None | Some(Term::Nil) => 2u64,
                Some(Term::Int(i)) => i.to_u64().unwrap_or(2),
                _ => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":version must be int or nil".to_string(),
                        Some(op),
                    ));
                }
            };
            let as_str_map =
                |v: Option<&Term>, field: &str| -> Result<BTreeMap<String, String>, Value> {
                    let mut out = BTreeMap::new();
                    let Some(term) = v else { return Ok(out) };
                    if matches!(term, Term::Nil) {
                        return Ok(out);
                    }
                    let Term::Map(mm) = term else {
                        return Err(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!("{field} must be map"),
                            Some(op),
                        ));
                    };
                    for (k, vv) in mm {
                        let key = match &k.0 {
                            Term::Str(s) => s.clone(),
                            _ => {
                                return Err(mk_error(
                                    error_tok,
                                    "core/pkg/bad-payload",
                                    format!("{field} keys must be strings"),
                                    Some(op),
                                ));
                            }
                        };
                        let val = match vv {
                            Term::Str(s) => s.clone(),
                            _ => {
                                return Err(mk_error(
                                    error_tok,
                                    "core/pkg/bad-payload",
                                    format!("{field}/{key} must be string"),
                                    Some(op),
                                ));
                            }
                        };
                        out.insert(key, val);
                    }
                    Ok(out)
                };
            let mut requirements: BTreeMap<String, gc_pkg::Requirement> = BTreeMap::new();
            if let Some(term) = m.get(&TermOrdKey(Term::symbol(":requirements")))
                && !matches!(term, Term::Nil)
            {
                let Term::Map(mm) = term else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":requirements must be map".to_string(),
                        Some(op),
                    ));
                };
                for (k, vv) in mm {
                    let name = match &k.0 {
                        Term::Str(s) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                ":requirements keys must be strings".to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let Term::Map(rm) = vv else {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!(":requirements/{name} must be map"),
                            Some(op),
                        ));
                    };
                    let selector = match rm.get(&TermOrdKey(Term::symbol(":selector"))) {
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":requirements/{name}/:selector must be string"),
                                Some(op),
                            ));
                        }
                    };
                    let update_policy = match rm.get(&TermOrdKey(Term::symbol(":update-policy"))) {
                        None | Some(Term::Nil) => gc_pkg::UpdatePolicy::Manual,
                        Some(Term::Symbol(s)) | Some(Term::Str(s))
                            if s == ":manual" || s == "manual" =>
                        {
                            gc_pkg::UpdatePolicy::Manual
                        }
                        Some(Term::Symbol(s)) | Some(Term::Str(s))
                            if s == ":auto" || s == "auto" =>
                        {
                            gc_pkg::UpdatePolicy::Auto
                        }
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(
                                    ":requirements/{name}/:update-policy must be :manual or :auto"
                                ),
                                Some(op),
                            ));
                        }
                    };
                    let registry = match rm.get(&TermOrdKey(Term::symbol(":registry"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":requirements/{name}/:registry must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let strategy = match rm.get(&TermOrdKey(Term::symbol(":strategy"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Symbol(s)) => {
                            gc_pkg::ResolutionStrategy::from_str(s.trim_start_matches(':'))
                        }
                        Some(Term::Str(s)) => gc_pkg::ResolutionStrategy::from_str(s),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(
                                    ":requirements/{name}/:strategy must be :pinned, :track-ref, :tag-policy, string, or nil"
                                ),
                                Some(op),
                            ));
                        }
                    };
                    if rm.contains_key(&TermOrdKey(Term::symbol(":strategy"))) && strategy.is_none()
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!(
                                ":requirements/{name}/:strategy must be :pinned, :track-ref, or :tag-policy"
                            ),
                            Some(op),
                        ));
                    }
                    let tag_policy = match rm.get(&TermOrdKey(Term::symbol(":tag-policy"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":requirements/{name}/:tag-policy must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let inferred = gc_pkg::infer_strategy(&selector);
                    requirements.insert(
                        name,
                        gc_pkg::Requirement {
                            selector,
                            update_policy,
                            registry,
                            strategy: strategy.unwrap_or(inferred),
                            tag_policy,
                        },
                    );
                }
            }

            let mut locked: BTreeMap<String, gc_pkg::LockedEntry> = BTreeMap::new();
            if let Some(term) = m.get(&TermOrdKey(Term::symbol(":locked")))
                && !matches!(term, Term::Nil)
            {
                let Term::Map(mm) = term else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":locked must be map".to_string(),
                        Some(op),
                    ));
                };
                for (k, vv) in mm {
                    let name = match &k.0 {
                        Term::Str(s) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                ":locked keys must be strings".to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let Term::Map(lm) = vv else {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!(":locked/{name} must be map"),
                            Some(op),
                        ));
                    };
                    let snapshot = match lm.get(&TermOrdKey(Term::symbol(":snapshot"))) {
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:snapshot must be string"),
                                Some(op),
                            ));
                        }
                    };
                    let commit = match lm.get(&TermOrdKey(Term::symbol(":commit"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:commit must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let registry = match lm.get(&TermOrdKey(Term::symbol(":registry"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:registry must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let source_selector = match lm
                        .get(&TermOrdKey(Term::symbol(":source_selector")))
                    {
                        None | Some(Term::Nil) => String::new(),
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:source_selector must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let resolved_ref = match lm.get(&TermOrdKey(Term::symbol(":resolved-ref"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:resolved-ref must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let exports_hash = match lm.get(&TermOrdKey(Term::symbol(":exports_hash"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:exports_hash must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let environment_fingerprint = match lm
                        .get(&TermOrdKey(Term::symbol(":environment-fingerprint")))
                    {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(
                                    ":locked/{name}/:environment-fingerprint must be string or nil"
                                ),
                                Some(op),
                            ));
                        }
                    };
                    locked.insert(
                        name,
                        gc_pkg::LockedEntry {
                            commit,
                            snapshot,
                            registry,
                            source_selector,
                            resolved_ref,
                            exports_hash,
                            environment_fingerprint,
                        },
                    );
                }
            }
            let registries = match as_str_map(
                m.get(&TermOrdKey(Term::symbol(":registries"))),
                ":registries",
            ) {
                Ok(x) => x,
                Err(v) => return Ok(v),
            };
            let artifacts =
                match as_str_map(m.get(&TermOrdKey(Term::symbol(":artifacts"))), ":artifacts") {
                    Ok(x) => x,
                    Err(v) => return Ok(v),
                };

            let mut l = gc_pkg::GenesisLock::empty(workspace);
            l.version = version;
            l.policy = policy_s;
            l.registries = registries;
            l.requirements = requirements;
            l.locked = locked;
            l.artifacts = artifacts;

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_write(
                &base_dir,
                &lock_s,
                pol.map(|p| p.create_dirs).unwrap_or(false),
            ) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            out.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            Ok(Value::Data(Term::Map(out)))
        }

        "core/pkg-low::info" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let name = match payload_pkg_name(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
            m.insert(
                TermOrdKey(Term::symbol(":requirement")),
                l.requirements
                    .get(&name)
                    .map(|r| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":selector")),
                                    Term::Str(r.selector.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":update-policy")),
                                    Term::Symbol(match r.update_policy {
                                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                                    }),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":registry")),
                                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":strategy")),
                                    Term::Symbol(format!(":{}", r.strategy.as_str())),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":tag-policy")),
                                    r.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":locked")),
                l.locked
                    .get(&name)
                    .map(|le| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":commit")),
                                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":snapshot")),
                                    Term::Str(le.snapshot.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":resolved-ref")),
                                    le.resolved_ref.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":environment-fingerprint")),
                                    le.environment_fingerprint
                                        .clone()
                                        .map(Term::Str)
                                        .unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::lock" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::lock".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::lock".to_string())
            })?;
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut out_locked: BTreeMap<String, gc_pkg::LockedEntry> = BTreeMap::new();
            for (name, req) in &l.requirements {
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        out_locked.insert(name.clone(), le);
                    }
                    Err(err_val) => return Ok(err_val),
                }
            }

            if strict
                && let Err(v) = validate_locked_entries_strict(
                    store,
                    &l.requirements,
                    &out_locked,
                    true,
                    error_tok,
                    op,
                )
            {
                return Ok(v);
            }
            l.locked = out_locked;
            let workspace_root = match persist_workspace_root_snapshot(store, &l, error_tok, op) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };
            l.artifacts.insert(
                "root_workspace_snapshot".to_string(),
                workspace_root.clone(),
            );
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, strict, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":locked-count")),
                Term::Int((l.locked.len() as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":strict")), Term::Bool(strict));
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                Term::Str(workspace_root.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            Term::Str(workspace_root),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock-h")),
                            Term::Str(lock_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::update" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::update".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::update".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut updated: u64 = 0;
            for (name, req) in &l.requirements {
                let should_update = req.update_policy == gc_pkg::UpdatePolicy::Auto
                    && matches!(
                        req.strategy,
                        gc_pkg::ResolutionStrategy::TrackRef
                            | gc_pkg::ResolutionStrategy::TagPolicy
                    );
                if !should_update && l.locked.contains_key(name) {
                    continue;
                }
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        l.locked.insert(name.clone(), le);
                        updated = updated.saturating_add(1);
                    }
                    Err(err_val) => return Ok(err_val),
                }
            }
            if strict
                && let Err(v) = validate_locked_entries_strict(
                    store,
                    &l.requirements,
                    &l.locked,
                    true,
                    error_tok,
                    op,
                )
            {
                return Ok(v);
            }
            let workspace_root = match persist_workspace_root_snapshot(store, &l, error_tok, op) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };
            l.artifacts.insert(
                "root_workspace_snapshot".to_string(),
                workspace_root.clone(),
            );
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, strict, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":updated")),
                Term::Int((updated as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":strict")), Term::Bool(strict));
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                Term::Str(workspace_root.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            Term::Str(workspace_root),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock-h")),
                            Term::Str(lock_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::install" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::install".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let frozen = payload_pkg_bool(payload, ":frozen").unwrap_or(false);
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);

            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if frozen {
                let missing = l.requirements_missing_locks();
                if !missing.is_empty() {
                    return Ok(mk_error_with_ctx(
                        error_tok,
                        "core/pkg/not-locked",
                        "lock is missing locked entries".to_string(),
                        Some(op),
                        Term::Map(
                            [(
                                TermOrdKey(Term::symbol(":missing")),
                                Term::Vector(missing.into_iter().map(Term::Str).collect()),
                            )]
                            .into_iter()
                            .collect(),
                        ),
                    ));
                }
            }

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, false, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };
            let workspace_root = l.artifacts.get("root_workspace_snapshot").cloned();

            for (name, le) in &l.locked {
                // Snapshot must exist and be well-formed.
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };

                // Shallow closure: snapshot + module artifacts.
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();

                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if strict && let Some(commit_hex) = &le.commit {
                    match validate_commit_artifact_closure(
                        store,
                        name,
                        snapshot_hex,
                        commit_hex,
                        true,
                        error_tok,
                        op,
                    ) {
                        Ok(n) => checked = checked.saturating_add(n),
                        Err(v) => return Ok(v),
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                workspace_root.clone().map(Term::Str).unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            workspace_root.map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::verify" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::verify".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };

            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;

            for (name, le) in &l.locked {
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();
                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if let Some(commit_hex) = &le.commit {
                    match validate_commit_artifact_closure(
                        store,
                        name,
                        snapshot_hex,
                        commit_hex,
                        true,
                        error_tok,
                        op,
                    ) {
                        Ok(n) => checked = checked.saturating_add(n),
                        Err(v) => return Ok(v),
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::snapshot" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::snapshot".to_string())
            })?;
            let pkg_path_s = match payload_pkg_path(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let pkg_path = sandbox_path_read(&base_dir, &pkg_path_s)?;

            let (manifest, pkg_dir) = match gc_pkg::PackageManifest::load(&pkg_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/manifest-error",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            // Ensure the package directory is within the sandbox base.
            let base = std::fs::canonicalize(&base_dir)?;
            let pkg_dir_resolved = std::fs::canonicalize(&pkg_dir)?;
            if !pkg_dir_resolved.starts_with(&base) {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/path-escape",
                    "package directory escapes base dir".to_string(),
                    Some(op),
                ));
            }

            let mut modules_out: Vec<Term> = Vec::new();
            for me in &manifest.modules {
                let module_fs_path = pkg_dir.join(&me.path);
                let resolved = match std::fs::canonicalize(&module_fs_path) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                if !resolved.starts_with(&base) {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("module escapes base dir: {}", me.path),
                        Some(op),
                    ));
                }
                let src = match std::fs::read_to_string(&resolved) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                let forms = match gc_coreform::parse_module(&src) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/parse-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let forms = match gc_coreform::canonicalize_module(forms) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/canon-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let module_h = gc_coreform::hash_module(&forms);
                if let Some(want_hex) = &me.hash {
                    if want_hex.len() != 64 || !want_hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-hash",
                            format!("manifest module hash is not 64-hex: {}", me.path),
                            Some(op),
                        ));
                    }
                    let got_hex = blake3::Hash::from_bytes(module_h).to_hex().to_string();
                    if &got_hex != want_hex {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/hash-mismatch",
                            format!("module hash mismatch: {}", me.path),
                            Some(op),
                        ));
                    }
                }

                let module_art = Term::Vector(forms);
                let module_bytes = print_term(&module_art);
                let store_hex = match store_put_with_budget(
                    store,
                    module_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };
                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::Symbol(":path".to_string())),
                    Term::Str(me.path.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::Symbol(":hash".to_string())),
                    Term::Str(store_hex),
                );
                mm.insert(
                    TermOrdKey(Term::Symbol(":module-h".to_string())),
                    Term::Bytes(module_h.to_vec().into()),
                );
                modules_out.push(Term::Map(mm));
            }

            let snapshot = Term::Map(
                [
                    (
                        TermOrdKey(Term::Symbol(":type".to_string())),
                        Term::Symbol(":vcs/snapshot".to_string()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":v".to_string())),
                        Term::Int(1.into()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":kind".to_string())),
                        Term::Symbol(":package".to_string()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":pkg/name".to_string())),
                        Term::Str(manifest.name.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":pkg/version".to_string())),
                        Term::Str(manifest.version.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":modules".to_string())),
                        Term::Vector(modules_out.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":obligations".to_string())),
                        Term::Vector(
                            manifest
                                .obligations
                                .iter()
                                .cloned()
                                .map(Term::Symbol)
                                .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let snapshot_bytes = print_term(&snapshot);
            let snap_hex = match store_put_with_budget(
                store,
                snapshot_bytes.as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let mut out = BTreeMap::new();
            out.insert(
                TermOrdKey(Term::Symbol(":snapshot".to_string())),
                Term::Str(snap_hex),
            );
            out.insert(
                TermOrdKey(Term::Symbol(":modules".to_string())),
                Term::Vector(modules_out),
            );
            Ok(Value::Data(Term::Map(out)))
        }

        "core/pkg-low::publish" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::publish".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::publish".to_string())
            })?;

            let remote_s = match payload_pkg_publish_remote(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let refname = match payload_pkg_publish_ref(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let policy_h = match payload_pkg_publish_policy(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let expected_old = match payload_pkg_publish_expected_old(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let depth = payload_pkg_publish_depth(payload).unwrap_or(0);
            let commit_override = match payload_pkg_publish_commit(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let commit_hex = if let Some(h) = commit_override {
                h
            } else {
                let h = match refs.get(&refname) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let Some(h) = h else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/ref-not-found",
                        format!("local ref is unset: {refname}"),
                        Some(op),
                    ));
                };
                h
            };

            if gc_vcs::validate_hex_hash(&commit_hex).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "commit must be 64-hex".to_string(),
                    Some(op),
                ));
            }
            if gc_vcs::validate_hex_hash(&policy_h).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "policy must be 64-hex".to_string(),
                    Some(op),
                ));
            }

            let pol_term = match store_get_term(store, &policy_h) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {policy_h}"),
                        Some(op),
                    ));
                }
            };
            let pol_art = match gc_vcs::Policy::from_term(&pol_term) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-policy",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            if pol_art.is_frozen_ref(&refname) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/ref-frozen",
                    format!("ref is frozen by policy: {refname}"),
                    Some(op),
                ));
            }
            let class = match pol_art.class_for_ref(&refname) {
                Some(c) => c,
                None => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/no-policy-class",
                        format!("policy has no matching class for ref: {refname}"),
                        Some(op),
                    ));
                }
            };

            let commit_term = match store_get_term(store, &commit_hex) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {commit_hex}"),
                        Some(op),
                    ));
                }
            };
            let commit = match gc_vcs::Commit::from_term(&commit_term) {
                Ok(c) => c,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-commit",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            for req in &class.required_obligations {
                if !commit.obligations.iter().any(|o| o == req) {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-obligation",
                        format!("commit missing required obligation: {req}"),
                        Some(op),
                    ));
                }
            }
            if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/missing-evidence",
                    "commit has required obligations but no evidence".to_string(),
                    Some(op),
                ));
            }
            let mut evidence_kinds: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for ev_h in &commit.evidence {
                let ev_term = match store_get_term(store, ev_h) {
                    Ok(t) => t,
                    Err(_) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("artifact not found: {ev_h}"),
                            Some(op),
                        ));
                    }
                };
                let ev = match gc_vcs::Evidence::from_term(&ev_term) {
                    Ok(ev) => ev,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-evidence",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                evidence_kinds.insert(gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind));
            }
            let missing_kinds =
                class.missing_required_evidence_kinds(&commit.obligations, &evidence_kinds);
            if !missing_kinds.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/missing-evidence-kind",
                    format!(
                        "commit evidence missing required kinds: {}",
                        missing_kinds.join(", ")
                    ),
                    Some(op),
                ));
            }

            if class.require_signatures {
                let signing_h = match gc_vcs::commit_signing_hash(&commit_term) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-commit",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut valid: u64 = 0;
                let mut seen_pks: std::collections::BTreeSet<Vec<u8>> =
                    std::collections::BTreeSet::new();
                for at_h in &commit.attestations {
                    let at_term = match store_get_term(store, at_h) {
                        Ok(t) => t,
                        Err(_) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/not-found",
                                format!("artifact not found: {at_h}"),
                                Some(op),
                            ));
                        }
                    };
                    let at = match gc_vcs::Attestation::from_term(&at_term) {
                        Ok(a) => a,
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-attestation",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let pk_vec = at.pk.to_vec();
                    if seen_pks.contains(&pk_vec) {
                        continue;
                    }
                    if gc_vcs::verify_commit_attestation(
                        &at,
                        &signing_h,
                        &class.allowed_public_keys,
                    )
                    .is_ok()
                    {
                        seen_pks.insert(pk_vec);
                        valid = valid.saturating_add(1);
                    }
                }
                if valid < class.min_signatures {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-signatures",
                        format!(
                            "need {} valid signatures, got {valid}",
                            class.min_signatures
                        ),
                        Some(op),
                    ));
                }
            }

            let mut set_ref_mm = BTreeMap::new();
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Str(refname.clone()),
            );
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":hash")),
                Term::Str(commit_hex.clone()),
            );
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":policy")),
                Term::Str(policy_h.clone()),
            );
            if let Some(eo) = &expected_old {
                set_ref_mm.insert(
                    TermOrdKey(Term::symbol(":expected-old")),
                    Term::Str(eo.clone()),
                );
            }

            let mut sync_payload_m = BTreeMap::new();
            sync_payload_m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(remote_s));
            sync_payload_m.insert(
                TermOrdKey(Term::symbol(":roots")),
                Term::Vector(vec![
                    Term::Str(commit_hex.clone()),
                    Term::Str(policy_h.clone()),
                ]),
            );
            if depth > 0 {
                sync_payload_m.insert(
                    TermOrdKey(Term::symbol(":depth")),
                    Term::Int((depth as i64).into()),
                );
            }
            sync_payload_m.insert(
                TermOrdKey(Term::symbol(":set-refs")),
                Term::Vector(vec![Term::Map(set_ref_mm)]),
            );

            let sync_payload = Term::Map(sync_payload_m);
            let sync_pol = pol.or_else(|| policy.op_policy("core/sync::push"));
            let sync_out = call_capability(
                "core/sync::push",
                &sync_payload,
                sync_pol,
                policy,
                Some(store),
                Some(refs),
                budget,
                error_tok,
            )?;

            let out = match sync_out {
                Value::Data(Term::Map(mut m)) => {
                    m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(commit_hex));
                    m.insert(TermOrdKey(Term::symbol(":ref")), Term::Str(refname));
                    m.insert(
                        TermOrdKey(Term::symbol(":provenance")),
                        commit_provenance_term(&commit),
                    );
                    Value::Data(Term::Map(m))
                }
                other => other,
            };
            Ok(out)
        }
        "core/store::put" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::put".to_string())
            })?;
            let art = payload_store_artifact(payload)?;
            let bytes = print_term(&art);
            let configured_max = match op_extra_positive_usize(pol, "max_bytes") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let max_bytes = effective_limit(configured_max, HARD_REMOTE_ARTIFACT_MAX_BYTES);
            if bytes.len() > max_bytes {
                return Ok(mk_resource_limit_error(
                    error_tok,
                    op,
                    "store put bytes",
                    bytes.len(),
                    max_bytes,
                ));
            }
            let h =
                match store_put_with_budget(store, bytes.as_bytes(), policy, budget, error_tok, op)
                {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };
            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::Symbol(":hash".to_string())), Term::Str(h));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/store::has" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::has".to_string())
            })?;
            let h = payload_store_hash(payload)?;
            let p = store.path_for(&h);
            let mut present = false;
            if p.exists() {
                if let Err(e) = store.verify_hex(&h) {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        e.to_string(),
                        Some(op),
                    ));
                }
                present = true;
            } else {
                match store_remote_client(policy, pol, timeout_ms, error_tok, op) {
                    Ok(Some((client, _base))) => {
                        let mp = match client.store_has(std::slice::from_ref(&h)) {
                            Ok(m) => m,
                            Err(e) => {
                                let code = match &e {
                                    gc_registry::RegistryError::Auth(_) => "core/store/remote-auth",
                                    _ => "core/store/remote-error",
                                };
                                return Ok(mk_error(error_tok, code, e.to_string(), Some(op)));
                            }
                        };
                        present = mp.get(&h).copied().unwrap_or(false);
                    }
                    Ok(None) => {}
                    Err(v) => return Ok(v),
                }
            }
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":present".to_string())),
                Term::Bool(present),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/store::get" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::get".to_string())
            })?;
            let configured_max = match op_extra_positive_usize(pol, "max_bytes") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let max_bytes = effective_limit(configured_max, HARD_REMOTE_ARTIFACT_MAX_BYTES);
            let h = payload_store_hash(payload)?;
            let p = store.path_for(&h);
            if !p.exists() {
                match store_remote_client(policy, pol, timeout_ms, error_tok, op) {
                    Ok(Some((client, _base))) => {
                        let bytes = match client.store_get_opt_bounded(&h, Some(max_bytes)) {
                            Ok(Some(b)) => b,
                            Ok(None) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/store/not-found",
                                    format!("artifact not found: {h}"),
                                    Some(op),
                                ));
                            }
                            Err(e) => {
                                if format!("{e}").contains("resource-limit:") {
                                    return Ok(mk_resource_limit_error(
                                        error_tok,
                                        op,
                                        "remote artifact bytes",
                                        max_bytes.saturating_add(1),
                                        max_bytes,
                                    ));
                                }
                                let code = match &e {
                                    gc_registry::RegistryError::Auth(_) => "core/store/remote-auth",
                                    _ => "core/store/remote-error",
                                };
                                return Ok(mk_error(error_tok, code, e.to_string(), Some(op)));
                            }
                        };
                        let got = hash_bytes_hex(&bytes);
                        if got != h {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/hash-mismatch",
                                "remote bytes hash mismatch".to_string(),
                                Some(op),
                            ));
                        }
                        match store_put_with_budget(store, &bytes, policy, budget, error_tok, op) {
                            Ok(stored_h) if stored_h == h => {}
                            Ok(_) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/store/hash-mismatch",
                                    "local store wrote under different hash".to_string(),
                                    Some(op),
                                ));
                            }
                            Err(v) => return Ok(v),
                        }
                    }
                    Ok(None) => {}
                    Err(v) => return Ok(v),
                }
            }
            if !p.exists() {
                return Ok(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("artifact not found: {h}"),
                    Some(op),
                ));
            }
            if let Err(e) = store.verify_hex(&h) {
                return Ok(mk_error(
                    error_tok,
                    "core/store/corruption",
                    e.to_string(),
                    Some(op),
                ));
            }
            if let Ok(md) = std::fs::metadata(store.path_for(&h))
                && md.len() > max_bytes as u64
            {
                return Ok(mk_resource_limit_error(
                    error_tok,
                    op,
                    "artifact bytes",
                    md.len() as usize,
                    max_bytes,
                ));
            }
            let bytes = match store.get_bytes_limited(&h, max_bytes) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let s = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/bad-artifact",
                        "artifact bytes are not a utf-8 CoreForm term".to_string(),
                        Some(op),
                    ));
                }
            };
            let t = match gc_coreform::parse_term(&s) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/bad-artifact",
                        format!("bad artifact term: {e}"),
                        Some(op),
                    ));
                }
            };
            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::Symbol(":artifact".to_string())), t);
            Ok(Value::Data(Term::Map(m)))
        }
        "core/store::verify" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::verify".to_string())
            })?;
            let maybe_hash = match payload_store_optional_hash(payload) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let mut checked: u64 = 0;
            if let Some(h) = maybe_hash.clone() {
                if !is_hex64(&h) {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/bad-hash",
                        format!("invalid store hash: {h}"),
                        Some(op),
                    ));
                }
                if !store.path_for(&h).exists() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {h}"),
                        Some(op),
                    ));
                }
                checked = 1;
                if let Err(e) = store.verify_hex(&h) {
                    let ctx = Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                            (
                                TermOrdKey(Term::symbol(":checked")),
                                Term::Int(BigInt::from(checked)),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    return Ok(mk_error_with_ctx(
                        error_tok,
                        "core/store/corruption",
                        e.to_string(),
                        Some(op),
                        ctx,
                    ));
                }
            } else {
                let hashes = match store_scan_hashes(store) {
                    Ok(hs) => hs,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                for h in hashes {
                    checked = checked.saturating_add(1);
                    if let Err(e) = store.verify_hex(&h) {
                        let ctx = Term::Map(
                            [
                                (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                                (
                                    TermOrdKey(Term::symbol(":checked")),
                                    Term::Int(BigInt::from(checked)),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        );
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/store/corruption",
                            e.to_string(),
                            Some(op),
                            ctx,
                        ));
                    }
                }
            }
            let mut out = BTreeMap::new();
            out.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int(BigInt::from(checked)),
            );
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            if let Some(h) = maybe_hash {
                out.insert(TermOrdKey(Term::symbol(":hash")), Term::Str(h));
            }
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::log" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::log".to_string())
            })?;

            let root_s = match payload_vcs_root(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let max = payload_vcs_max(payload).unwrap_or(1000);

            let mut root_commit = root_s.clone();
            if root_commit.starts_with("refs/") {
                let Some(rdb) = refs else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/missing-refs-db",
                        "root is a ref name but refs db is not configured".to_string(),
                        Some(op),
                    ));
                };
                let cur = match rdb.get(&root_commit) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/refs-io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let Some(h) = cur else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/ref-not-found",
                        format!("ref not found: {root_commit}"),
                        Some(op),
                    ));
                };
                root_commit = h;
            }

            if gc_vcs::validate_hex_hash(&root_commit).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-root",
                    "root must be a 64-hex commit hash or refs/...".to_string(),
                    Some(op),
                ));
            }

            use std::collections::HashSet;
            let mut visited: HashSet<String> = HashSet::new();
            let mut out: Vec<Term> = Vec::new();
            let mut stack: Vec<String> = vec![root_commit.clone()];

            let mut truncated = false;
            while let Some(h) = stack.pop() {
                if out.len() as u64 >= max {
                    truncated = true;
                    break;
                }
                if !visited.insert(h.clone()) {
                    continue;
                }
                let p = store.path_for(&h);
                if !p.exists() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {h}"),
                        Some(op),
                    ));
                }
                let t = match store_get_term(store, &h) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let c = match gc_vcs::Commit::from_term(&t) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/bad-commit",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };

                // Deterministic parent traversal: preserve stored order.
                for parent in c.parents.iter().rev() {
                    stack.push(parent.clone());
                }

                let mut cm = BTreeMap::new();
                cm.insert(TermOrdKey(Term::symbol(":hash")), Term::Str(h));
                cm.insert(
                    TermOrdKey(Term::symbol(":parents")),
                    Term::Vector(c.parents.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(
                    TermOrdKey(Term::symbol(":base")),
                    match c.base {
                        Some(b) => Term::Str(b),
                        None => Term::Nil,
                    },
                );
                cm.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(c.patch));
                cm.insert(TermOrdKey(Term::symbol(":result")), Term::Str(c.result));
                cm.insert(
                    TermOrdKey(Term::symbol(":obligations")),
                    Term::Vector(c.obligations.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(
                    TermOrdKey(Term::symbol(":evidence")),
                    Term::Vector(c.evidence.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(
                    TermOrdKey(Term::symbol(":attestations")),
                    Term::Vector(c.attestations.iter().cloned().map(Term::Str).collect()),
                );
                cm.insert(TermOrdKey(Term::symbol(":message")), Term::Str(c.message));
                out.push(Term::Map(cm));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":root")), Term::Str(root_commit));
            m.insert(
                TermOrdKey(Term::symbol(":truncated")),
                Term::Bool(truncated),
            );
            m.insert(TermOrdKey(Term::symbol(":commits")), Term::Vector(out));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::blame" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::blame".to_string())
            })?;

            let sym = match payload_vcs_sym(payload, ":sym") {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let path = match payload_vcs_opt_sym_or_str(payload, ":path") {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let snapshot_h = match payload_vcs_opt_hash(payload, ":snapshot") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let commit_h = match payload_vcs_opt_hash(payload, ":commit") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            if snapshot_h.is_none() && commit_h.is_none() {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :snapshot or :commit".to_string(),
                    Some(op),
                ));
            }

            let start_commit = if let Some(ch) = commit_h {
                ch
            } else {
                let Some(rdb) = refs else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/missing-refs-db",
                        "snapshot lookup requires refs db".to_string(),
                        Some(op),
                    ));
                };
                let Some(sh) = snapshot_h.clone() else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        "missing :snapshot".to_string(),
                        Some(op),
                    ));
                };
                let found = match vcs_find_commit_for_snapshot(store, rdb, &sh) {
                    Ok(x) => x,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op)));
                    }
                };
                let Some(h) = found else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/no-commit-for-snapshot",
                        format!("no commit found for snapshot: {sh}"),
                        Some(op),
                    ));
                };
                h
            };

            let (start_commit_obj, _) = match vcs_load_commit(store, &start_commit) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };

            if let Some(sh) = &snapshot_h
                && &start_commit_obj.result != sh
            {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "provided :commit does not resolve to provided :snapshot".to_string(),
                    Some(op),
                ));
            }

            let query_snapshot = snapshot_h.unwrap_or(start_commit_obj.result.clone());
            let value_h = match vcs_snapshot_symbol_ref(store, &query_snapshot, &sym) {
                Ok(Some(h)) => h,
                Ok(None) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/symbol-not-found",
                        format!("symbol not found in snapshot: {sym}"),
                        Some(op),
                    ));
                }
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };

            let blame_h = match vcs_blame_commit_for_symbol(store, &start_commit, &sym, &value_h) {
                Ok(h) => h,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };
            let (blame_commit, _) = match vcs_load_commit(store, &blame_h) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym));
            m.insert(TermOrdKey(Term::symbol(":value")), Term::Str(value_h));
            m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(blame_h));
            m.insert(
                TermOrdKey(Term::symbol(":snapshot")),
                Term::Str(blame_commit.result),
            );
            m.insert(
                TermOrdKey(Term::symbol(":query-snapshot")),
                Term::Str(query_snapshot),
            );
            m.insert(
                TermOrdKey(Term::symbol(":path")),
                path.map(Term::Str).unwrap_or(Term::Nil),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::why" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::why".to_string())
            })?;

            let sym = match payload_vcs_sym(payload, ":sym") {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let op_sym = match payload_vcs_opt_sym_or_str(payload, ":op") {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let path = match payload_vcs_opt_sym_or_str(payload, ":path") {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let snapshot_h = match payload_vcs_opt_hash(payload, ":snapshot") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let commit_h = match payload_vcs_opt_hash(payload, ":commit") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            if snapshot_h.is_none() && commit_h.is_none() {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :snapshot or :commit".to_string(),
                    Some(op),
                ));
            }

            let start_commit = if let Some(ch) = commit_h {
                ch
            } else {
                let Some(rdb) = refs else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/missing-refs-db",
                        "snapshot lookup requires refs db".to_string(),
                        Some(op),
                    ));
                };
                let Some(sh) = snapshot_h.clone() else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        "missing :snapshot".to_string(),
                        Some(op),
                    ));
                };
                let found = match vcs_find_commit_for_snapshot(store, rdb, &sh) {
                    Ok(x) => x,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op)));
                    }
                };
                let Some(h) = found else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/no-commit-for-snapshot",
                        format!("no commit found for snapshot: {sh}"),
                        Some(op),
                    ));
                };
                h
            };

            let (start_commit_obj, _) = match vcs_load_commit(store, &start_commit) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };
            if let Some(sh) = &snapshot_h
                && &start_commit_obj.result != sh
            {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "provided :commit does not resolve to provided :snapshot".to_string(),
                    Some(op),
                ));
            }

            let query_snapshot = snapshot_h.unwrap_or(start_commit_obj.result.clone());
            let value_h = match vcs_snapshot_symbol_ref(store, &query_snapshot, &sym) {
                Ok(Some(h)) => h,
                Ok(None) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/symbol-not-found",
                        format!("symbol not found in snapshot: {sym}"),
                        Some(op),
                    ));
                }
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };
            let blame_h = match vcs_blame_commit_for_symbol(store, &start_commit, &sym, &value_h) {
                Ok(h) => h,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/store-error", e, Some(op))),
            };

            let (blame_commit, blame_term) = match vcs_load_commit(store, &blame_h) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/vcs/bad-commit", e, Some(op))),
            };
            let (target, author, why) = match &blame_term {
                Term::Map(mm) => (
                    mm.get(&TermOrdKey(Term::symbol(":target")))
                        .cloned()
                        .unwrap_or(Term::Nil),
                    mm.get(&TermOrdKey(Term::symbol(":author")))
                        .cloned()
                        .unwrap_or(Term::Nil),
                    mm.get(&TermOrdKey(Term::symbol(":why")))
                        .cloned()
                        .unwrap_or(Term::Nil),
                ),
                _ => (Term::Nil, Term::Nil, Term::Nil),
            };

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym));
            m.insert(TermOrdKey(Term::symbol(":value")), Term::Str(value_h));
            m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(blame_h));
            m.insert(
                TermOrdKey(Term::symbol(":snapshot")),
                Term::Str(blame_commit.result),
            );
            m.insert(
                TermOrdKey(Term::symbol(":query-snapshot")),
                Term::Str(query_snapshot),
            );
            m.insert(
                TermOrdKey(Term::symbol(":message")),
                Term::Str(blame_commit.message),
            );
            m.insert(
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    blame_commit
                        .obligations
                        .into_iter()
                        .map(Term::Str)
                        .collect(),
                ),
            );
            m.insert(
                TermOrdKey(Term::symbol(":evidence")),
                Term::Vector(blame_commit.evidence.into_iter().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":attestations")),
                Term::Vector(
                    blame_commit
                        .attestations
                        .into_iter()
                        .map(Term::Str)
                        .collect(),
                ),
            );
            m.insert(
                TermOrdKey(Term::symbol(":path")),
                path.map(Term::Str).unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":op")),
                op_sym.map(Term::Str).unwrap_or(Term::Nil),
            );
            m.insert(TermOrdKey(Term::symbol(":target")), target);
            m.insert(TermOrdKey(Term::symbol(":author")), author);
            m.insert(TermOrdKey(Term::symbol(":why")), why);
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::diff-terms" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::diff-terms".to_string())
            })?;
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(to_t) = m.get(&TermOrdKey(Term::symbol(":to-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :to-term".to_string(),
                    Some(op),
                ));
            };
            let (patch_term, values) = match vcs_diff_patch_term(store, base_t, to_t) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/diff-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":patch-term")), patch_term);
            out.insert(
                TermOrdKey(Term::symbol(":values")),
                Term::Vector(values.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::apply-patch" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log(
                    "missing artifact store for core/vcs-low::apply-patch".to_string(),
                )
            })?;
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(patch_t) = m.get(&TermOrdKey(Term::symbol(":patch-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :patch-term".to_string(),
                    Some(op),
                ));
            };
            let patch = match gc_vcs::Patch::from_term(patch_t) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-patch",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let snapshot_t = match vcs_apply_patch_term(store, base_t, &patch) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/vcs/apply-error", e, Some(op)));
                }
            };
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":snapshot-term")), snapshot_t);
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::merge3-contract-snapshots" => {
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let Some(base_t) = m.get(&TermOrdKey(Term::symbol(":base-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :base-term".to_string(),
                    Some(op),
                ));
            };
            let Some(left_t) = m.get(&TermOrdKey(Term::symbol(":left-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :left-term".to_string(),
                    Some(op),
                ));
            };
            let Some(right_t) = m.get(&TermOrdKey(Term::symbol(":right-term"))) else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :right-term".to_string(),
                    Some(op),
                ));
            };
            let term_hash = |t: &Term| hash_bytes_hex(print_term(t).as_bytes());
            let base_h = match m.get(&TermOrdKey(Term::symbol(":base-hash"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => term_hash(base_t),
            };
            let left_h = match m.get(&TermOrdKey(Term::symbol(":left-hash"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => term_hash(left_t),
            };
            let right_h = match m.get(&TermOrdKey(Term::symbol(":right-hash"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => term_hash(right_t),
            };

            let base = match as_contract_snapshot(&base_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let left = match as_contract_snapshot(&left_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let right = match as_contract_snapshot(&right_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };

            if base.proto != left.proto || base.proto != right.proto {
                let conflict_term = mk_conflict_artifact(
                    ":contract-snapshot-merge3",
                    &base_h,
                    &left_h,
                    &right_h,
                    vec![Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":op")), Term::symbol(":proto")),
                            (
                                TermOrdKey(Term::symbol(":base")),
                                base.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":left")),
                                left.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":right")),
                                right.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )],
                );
                let mut out = BTreeMap::new();
                out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                out.insert(TermOrdKey(Term::symbol(":conflict-term")), conflict_term);
                return Ok(Value::Data(Term::Map(out)));
            }

            let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            keys.extend(base.overrides.keys().cloned());
            keys.extend(left.overrides.keys().cloned());
            keys.extend(right.overrides.keys().cloned());

            let mut merged: BTreeMap<String, String> = BTreeMap::new();
            let mut conflicts: Vec<Term> = Vec::new();
            for k in keys {
                let b = base.overrides.get(&k).cloned();
                let l = left.overrides.get(&k).cloned();
                let r = right.overrides.get(&k).cloned();

                let pick = if l == r {
                    l.clone()
                } else if l == b {
                    r.clone()
                } else if r == b {
                    l.clone()
                } else {
                    None
                };

                if l == r || l == b || r == b {
                    if let Some(h) = pick {
                        merged.insert(k.clone(), h);
                    }
                    continue;
                }

                conflicts.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":op")), Term::Symbol(k.clone())),
                        (
                            TermOrdKey(Term::symbol(":base")),
                            b.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":left")),
                            l.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":right")),
                            r.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }

            if !conflicts.is_empty() {
                conflicts.sort_by_cached_key(print_term);
                let conflict_term = mk_conflict_artifact(
                    ":contract-snapshot-merge3",
                    &base_h,
                    &left_h,
                    &right_h,
                    conflicts,
                );
                let mut out = BTreeMap::new();
                out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                out.insert(TermOrdKey(Term::symbol(":conflict-term")), conflict_term);
                return Ok(Value::Data(Term::Map(out)));
            }

            let merged_snapshot = gc_vcs::ContractSnapshot {
                proto: base.proto,
                overrides: merged,
            }
            .to_term();
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":snapshot-term")), merged_snapshot);
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::resolve-conflict" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log(
                    "missing artifact store for core/vcs-low::resolve-conflict".to_string(),
                )
            })?;
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let out_s = match m.get(&TermOrdKey(Term::symbol(":out"))) {
                None | Some(Term::Nil) => None,
                Some(Term::Str(s)) => Some(s.clone()),
                Some(other) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!(":out must be string or nil, got {}", print_term(other)),
                        Some(op),
                    ));
                }
            };
            let (conflict, base_t, left_t, right_t, legacy_output_mode) = if let (
                Some(conflict_t),
                Some(base_t),
                Some(left_t),
                Some(right_t),
            ) = (
                m.get(&TermOrdKey(Term::symbol(":conflict-term"))),
                m.get(&TermOrdKey(Term::symbol(":base-term"))),
                m.get(&TermOrdKey(Term::symbol(":left-term"))),
                m.get(&TermOrdKey(Term::symbol(":right-term"))),
            ) {
                let conflict = match gc_vcs::Conflict::from_term(conflict_t) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/bad-conflict",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                (
                    conflict,
                    base_t.clone(),
                    left_t.clone(),
                    right_t.clone(),
                    false,
                )
            } else if let Some(Term::Str(conflict_h)) =
                m.get(&TermOrdKey(Term::symbol(":conflict-hash")))
            {
                let conflict_t = match store_get_term(store, conflict_h) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let conflict = match gc_vcs::Conflict::from_term(&conflict_t) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/bad-conflict",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let base_t = match store_get_term(store, &conflict.base) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let left_t = match store_get_term(store, &conflict.left) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let right_t = match store_get_term(store, &conflict.right) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                (conflict, base_t, left_t, right_t, false)
            } else if let Some(Term::Str(conflict_h)) =
                m.get(&TermOrdKey(Term::symbol(":conflict")))
            {
                let conflict_t = match store_get_term(store, conflict_h) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let conflict = match gc_vcs::Conflict::from_term(&conflict_t) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/bad-conflict",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let base_t = match store_get_term(store, &conflict.base) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let left_t = match store_get_term(store, &conflict.left) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let right_t = match store_get_term(store, &conflict.right) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/store-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                (conflict, base_t, left_t, right_t, true)
            } else {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :conflict/:conflict-hash or (:conflict-term + :base-term/:left-term/:right-term)"
                        .to_string(),
                    Some(op),
                ));
            };

            let strategy = match m.get(&TermOrdKey(Term::symbol(":strategy"))) {
                None | Some(Term::Nil) => None,
                Some(Term::Symbol(s)) => Some(s.clone()),
                Some(Term::Str(s)) => Some(s.clone()),
                Some(other) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!(
                            ":strategy must be symbol/string or nil, got {}",
                            print_term(other)
                        ),
                        Some(op),
                    ));
                }
            };
            let strategy = strategy.map(|s| match s.as_str() {
                ":left" | "left" => ":left".to_string(),
                ":right" | "right" => ":right".to_string(),
                ":base" | "base" => ":base".to_string(),
                other => other.to_string(),
            });
            if let Some(s) = &strategy
                && s != ":left"
                && s != ":right"
                && s != ":base"
            {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    format!("unsupported :strategy {s} (expected :left/:right/:base)"),
                    Some(op),
                ));
            }

            #[derive(Debug, Clone)]
            enum Resolution {
                Side(String),
                Hash(String),
                Delete,
            }
            let mut resolutions: BTreeMap<String, Resolution> = BTreeMap::new();
            if let Some(t) = m.get(&TermOrdKey(Term::symbol(":resolutions"))) {
                let Term::Map(rm) = t else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!(":resolutions must be map, got {}", print_term(t)),
                        Some(op),
                    ));
                };
                for (k, v) in rm {
                    let opk = match &k.0 {
                        Term::Symbol(s) => s.clone(),
                        Term::Str(s) => s.clone(),
                        other => {
                            return Ok(mk_error(
                                error_tok,
                                "core/vcs/bad-payload",
                                format!(
                                    ":resolutions keys must be symbol/string, got {}",
                                    print_term(other)
                                ),
                                Some(op),
                            ));
                        }
                    };
                    let res = match v {
                        Term::Nil => Resolution::Delete,
                        Term::Symbol(s) => match s.as_str() {
                            ":left" | "left" => Resolution::Side(":left".to_string()),
                            ":right" | "right" => Resolution::Side(":right".to_string()),
                            ":base" | "base" => Resolution::Side(":base".to_string()),
                            other => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/vcs/bad-payload",
                                    format!(
                                        ":resolutions/{opk} unsupported side {other} (expected :left/:right/:base)"
                                    ),
                                    Some(op),
                                ));
                            }
                        },
                        Term::Str(s) => match s.as_str() {
                            ":left" | "left" => Resolution::Side(":left".to_string()),
                            ":right" | "right" => Resolution::Side(":right".to_string()),
                            ":base" | "base" => Resolution::Side(":base".to_string()),
                            _ => {
                                if let Err(e) = gc_vcs::validate_hex_hash(s) {
                                    return Ok(mk_error(
                                        error_tok,
                                        "core/vcs/bad-payload",
                                        format!(":resolutions/{opk}: {e}"),
                                        Some(op),
                                    ));
                                }
                                Resolution::Hash(s.clone())
                            }
                        },
                        other => {
                            return Ok(mk_error(
                                error_tok,
                                "core/vcs/bad-payload",
                                format!(
                                    ":resolutions/{opk} must be side symbol, hex string, or nil; got {}",
                                    print_term(other)
                                ),
                                Some(op),
                            ));
                        }
                    };
                    resolutions.insert(opk, res);
                }
            }

            let base = match as_contract_snapshot(&base_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let left = match as_contract_snapshot(&left_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let right = match as_contract_snapshot(&right_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            if base.proto != left.proto || base.proto != right.proto {
                return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-conflict",
                    "proto mismatch across base/left/right".to_string(),
                    Some(op),
                ));
            }

            let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            keys.extend(base.overrides.keys().cloned());
            keys.extend(left.overrides.keys().cloned());
            keys.extend(right.overrides.keys().cloned());

            let mut merged: BTreeMap<String, String> = BTreeMap::new();
            let mut unresolved: Vec<Term> = Vec::new();

            for k in keys {
                let b = base.overrides.get(&k).cloned();
                let l = left.overrides.get(&k).cloned();
                let r = right.overrides.get(&k).cloned();

                let conflict_here = l != r && l != b && r != b;
                if !conflict_here {
                    let pick = if l == r {
                        l
                    } else if l == b {
                        r
                    } else if r == b {
                        l
                    } else {
                        None
                    };
                    if let Some(h) = pick {
                        merged.insert(k, h);
                    }
                    continue;
                }

                let chosen = resolutions
                    .get(&k)
                    .cloned()
                    .or_else(|| strategy.as_ref().map(|s| Resolution::Side(s.clone())));

                let picked = match chosen {
                    Some(Resolution::Side(s)) if s == ":left" => l,
                    Some(Resolution::Side(s)) if s == ":right" => r,
                    Some(Resolution::Side(s)) if s == ":base" => b,
                    Some(Resolution::Hash(h)) => Some(h),
                    Some(Resolution::Delete) => None,
                    _ => {
                        let mut mm = BTreeMap::new();
                        mm.insert(TermOrdKey(Term::symbol(":op")), Term::Str(k.clone()));
                        mm.insert(
                            TermOrdKey(Term::symbol(":base")),
                            b.clone().map(Term::Str).unwrap_or(Term::Nil),
                        );
                        mm.insert(
                            TermOrdKey(Term::symbol(":left")),
                            l.clone().map(Term::Str).unwrap_or(Term::Nil),
                        );
                        mm.insert(
                            TermOrdKey(Term::symbol(":right")),
                            r.clone().map(Term::Str).unwrap_or(Term::Nil),
                        );
                        unresolved.push(Term::Map(mm));
                        continue;
                    }
                };

                if let Some(h) = picked {
                    if !store.path_for(&h).exists() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("missing referenced artifact: {h}"),
                            Some(op),
                        ));
                    }
                    merged.insert(k, h);
                }
            }

            if !unresolved.is_empty() {
                let conflict_term = mk_conflict_artifact(
                    &conflict.kind,
                    &conflict.base,
                    &conflict.left,
                    &conflict.right,
                    unresolved,
                );
                if legacy_output_mode {
                    let conflict_bytes = print_term(&conflict_term);
                    let conflict_h = match store_put_with_budget(
                        store,
                        conflict_bytes.as_bytes(),
                        policy,
                        budget,
                        error_tok,
                        op,
                    ) {
                        Ok(h) => h,
                        Err(v) => return Ok(v),
                    };
                    if let Some(out_s) = &out_s {
                        let base_dir = effective_base_dir(pol)?;
                        let out_path = sandbox_path_write(
                            &base_dir,
                            out_s,
                            pol.map(|p| p.create_dirs).unwrap_or(false),
                        )?;
                        if let Err(e) =
                            atomic_write_text(&out_path, (conflict_bytes + "\n").as_bytes())
                        {
                            return Ok(mk_error(
                                error_tok,
                                "core/vcs/io-error",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    }
                    let mut out = BTreeMap::new();
                    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                    out.insert(TermOrdKey(Term::symbol(":conflict")), Term::Str(conflict_h));
                    return Ok(Value::Data(Term::Map(out)));
                }
                let mut out = BTreeMap::new();
                out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                out.insert(TermOrdKey(Term::symbol(":conflict-term")), conflict_term);
                return Ok(Value::Data(Term::Map(out)));
            }

            let merged_snapshot = gc_vcs::ContractSnapshot {
                proto: base.proto,
                overrides: merged,
            }
            .to_term();
            let (patch_term, values) = match vcs_diff_patch_term(store, &base_t, &merged_snapshot) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/diff-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            if legacy_output_mode {
                let merged_bytes = print_term(&merged_snapshot);
                let merged_h = match store_put_with_budget(
                    store,
                    merged_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };
                let patch_bytes = print_term(&patch_term);
                let patch_h = match store_put_with_budget(
                    store,
                    patch_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };
                if let Some(out_s) = &out_s {
                    let base_dir = effective_base_dir(pol)?;
                    let out_path = sandbox_path_write(
                        &base_dir,
                        out_s,
                        pol.map(|p| p.create_dirs).unwrap_or(false),
                    )?;
                    if let Err(e) = atomic_write_text(&out_path, (merged_bytes + "\n").as_bytes()) {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }
                let mut out = BTreeMap::new();
                out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
                out.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(merged_h));
                out.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(patch_h));
                out.insert(
                    TermOrdKey(Term::symbol(":values")),
                    Term::Vector(values.into_iter().map(Term::Str).collect()),
                );
                return Ok(Value::Data(Term::Map(out)));
            }
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":snapshot-term")), merged_snapshot);
            out.insert(TermOrdKey(Term::symbol(":patch-term")), patch_term);
            out.insert(
                TermOrdKey(Term::symbol(":values")),
                Term::Vector(values.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(out)))
        }
        "core/vcs-low::diff" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::diff".to_string())
            })?;

            let base_h = match payload_vcs_hash(payload, ":base") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let to_h = match payload_vcs_hash(payload, ":to") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let out_s = match payload_vcs_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let store_patch = payload_vcs_store(payload).unwrap_or(true);

            let base_t = match store_get_term(store, &base_h) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/store-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let to_t = match store_get_term(store, &to_h) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/store-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let (patch_term, values) = match vcs_diff_patch_term(store, &base_t, &to_t) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/diff-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let patch_bytes = print_term(&patch_term);
            let patch_h = if store_patch {
                match store_put_with_budget(
                    store,
                    patch_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                }
            } else {
                hash_bytes_hex(patch_bytes.as_bytes())
            };

            if let Some(out_s) = out_s {
                let base_dir = effective_base_dir(pol)?;
                let out_path = sandbox_path_write(
                    &base_dir,
                    &out_s,
                    pol.map(|p| p.create_dirs).unwrap_or(false),
                )?;
                if let Err(e) =
                    atomic_write_text(&out_path, (patch_bytes.clone() + "\n").as_bytes())
                {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(patch_h));
            m.insert(
                TermOrdKey(Term::symbol(":values")),
                Term::Vector(values.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::apply" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::apply".to_string())
            })?;

            let base_h = match payload_vcs_hash(payload, ":base") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let patch_s = match payload_vcs_patch(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let out_s = match payload_vcs_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let store_result = payload_vcs_store(payload).unwrap_or(true);

            let base_t = match store_get_term(store, &base_h) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/store-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let base_dir = effective_base_dir(pol)?;
            let patch_term = if gc_vcs::validate_hex_hash(&patch_s).is_ok() {
                match store_get_term(store, &patch_s) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }
            } else {
                let patch_path = match sandbox_path_read(&base_dir, &patch_s) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/caps/path-escape",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                let s = match std::fs::read_to_string(&patch_path) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                match gc_coreform::parse_term(&s) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/parse-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }
            };

            let patch = match gc_vcs::Patch::from_term(&patch_term) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-patch",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let cur = match vcs_apply_patch_term(store, &base_t, &patch) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/vcs/apply-error", e, Some(op)));
                }
            };

            let snap_bytes = print_term(&cur);
            let snap_h = if store_result {
                match store_put_with_budget(
                    store,
                    snap_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                }
            } else {
                hash_bytes_hex(snap_bytes.as_bytes())
            };

            if let Some(out_s) = out_s {
                let out_path = sandbox_path_write(
                    &base_dir,
                    &out_s,
                    pol.map(|p| p.create_dirs).unwrap_or(false),
                )?;
                if let Err(e) = atomic_write_text(&out_path, (snap_bytes.clone() + "\n").as_bytes())
                {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(snap_h));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/vcs-low::merge3" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs-low::merge3".to_string())
            })?;

            let out_s = match payload_vcs_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let base_h = match payload_vcs_hash(payload, ":base") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let left_h = match payload_vcs_hash(payload, ":left") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let right_h = match payload_vcs_hash(payload, ":right") {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let base_t = store_get_term(store, &base_h)
                .map_err(|e| EffectsError::Log(format!("merge3 base read error: {e}")))?;
            let left_t = store_get_term(store, &left_h)
                .map_err(|e| EffectsError::Log(format!("merge3 left read error: {e}")))?;
            let right_t = store_get_term(store, &right_h)
                .map_err(|e| EffectsError::Log(format!("merge3 right read error: {e}")))?;

            let base = match as_contract_snapshot(&base_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let left = match as_contract_snapshot(&left_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };
            let right = match as_contract_snapshot(&right_t) {
                Ok(s) => s,
                Err(msg) => {
                    return Ok(mk_error(error_tok, "core/vcs/bad-snapshot", msg, Some(op)));
                }
            };

            // Proto must be stable across all three snapshots for contract merge.
            if base.proto != left.proto || base.proto != right.proto {
                let conflict_term = mk_conflict_artifact(
                    ":contract-snapshot-merge3",
                    &base_h,
                    &left_h,
                    &right_h,
                    vec![Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":op")), Term::symbol(":proto")),
                            (
                                TermOrdKey(Term::symbol(":base")),
                                base.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":left")),
                                left.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                            (
                                TermOrdKey(Term::symbol(":right")),
                                right.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )],
                );
                let conflict_bytes = print_term(&conflict_term);
                let conflict_h = match store_put_with_budget(
                    store,
                    conflict_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };

                if let Some(out_s) = &out_s {
                    let base_dir = effective_base_dir(pol)?;
                    let out_path = sandbox_path_write(
                        &base_dir,
                        out_s,
                        pol.map(|p| p.create_dirs).unwrap_or(false),
                    )?;
                    if let Err(e) = atomic_write_text(&out_path, (conflict_bytes + "\n").as_bytes())
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }

                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                m.insert(TermOrdKey(Term::symbol(":conflict")), Term::Str(conflict_h));
                return Ok(Value::Data(Term::Map(m)));
            }

            let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            keys.extend(base.overrides.keys().cloned());
            keys.extend(left.overrides.keys().cloned());
            keys.extend(right.overrides.keys().cloned());

            let mut merged: BTreeMap<String, String> = BTreeMap::new();
            let mut conflicts: Vec<Term> = Vec::new();

            for k in keys {
                let b = base.overrides.get(&k).cloned();
                let l = left.overrides.get(&k).cloned();
                let r = right.overrides.get(&k).cloned();

                let pick = if l == r {
                    l.clone()
                } else if l == b {
                    r.clone()
                } else if r == b {
                    l.clone()
                } else {
                    None
                };

                if l == r || l == b || r == b {
                    if let Some(h) = pick {
                        merged.insert(k.clone(), h);
                    }
                    continue;
                }

                // One-side change from None / deletion can still be cleanly merged.
                // Treat absence as None; if one side differs from base and the other equals base, we already handled.
                // Remaining cases are conflicts.
                conflicts.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":op")), Term::Symbol(k.clone())),
                        (
                            TermOrdKey(Term::symbol(":base")),
                            b.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":left")),
                            l.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":right")),
                            r.clone().map(Term::Str).unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }

            if !conflicts.is_empty() {
                conflicts.sort_by_cached_key(print_term);
                let conflict_term = mk_conflict_artifact(
                    ":contract-snapshot-merge3",
                    &base_h,
                    &left_h,
                    &right_h,
                    conflicts,
                );
                let conflict_bytes = print_term(&conflict_term);
                let conflict_h = match store_put_with_budget(
                    store,
                    conflict_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };

                if let Some(out_s) = &out_s {
                    let base_dir = effective_base_dir(pol)?;
                    let out_path = sandbox_path_write(
                        &base_dir,
                        out_s,
                        pol.map(|p| p.create_dirs).unwrap_or(false),
                    )?;
                    if let Err(e) = atomic_write_text(&out_path, (conflict_bytes + "\n").as_bytes())
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/vcs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                }

                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                m.insert(TermOrdKey(Term::symbol(":conflict")), Term::Str(conflict_h));
                return Ok(Value::Data(Term::Map(m)));
            }

            let merged_snapshot = gc_vcs::ContractSnapshot {
                proto: base.proto,
                overrides: merged,
            }
            .to_term();
            let merged_bytes = print_term(&merged_snapshot);
            let merged_h = match store_put_with_budget(
                store,
                merged_bytes.as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            if let Some(out_s) = &out_s {
                let base_dir = effective_base_dir(pol)?;
                let out_path = sandbox_path_write(
                    &base_dir,
                    out_s,
                    pol.map(|p| p.create_dirs).unwrap_or(false),
                )?;
                if let Err(e) = atomic_write_text(&out_path, (merged_bytes + "\n").as_bytes()) {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(merged_h));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gc-low::plan" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gc-low::plan".to_string())
            })?;

            let base_dir = effective_base_dir(pol)?;
            let lock_s = payload_gc_lock(payload).unwrap_or_else(|| "genesis.lock".to_string());
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let depth = payload_gc_depth(payload).unwrap_or(200);
            let include_lock = payload_gc_include_lock(payload).unwrap_or(true);
            let include_refs = payload_gc_include_refs(payload).unwrap_or(true);

            let (refs_entries, lock_info, pins_info) = match gc_build_sources(
                refs,
                &base_dir,
                &lock_s,
                &pins_s,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            let (mut roots, roots_kind) = match gc_roots_plan_from_sources(
                &refs_entries,
                &lock_info,
                &pins_info,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            roots.sort();
            roots.dedup();

            let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for h in &roots {
                match sync_closure_local(store, h, depth, &mut live, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }

            let store_dir = store.root_dir();
            let _lk = gc_store_lock(store_dir)?;
            let (dead, dead_bytes, largest) = gc_store_dead_set(store_dir, &live)?;

            let largest_term: Vec<Term> = largest
                .into_iter()
                .map(|(h, b)| {
                    Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                            (
                                TermOrdKey(Term::symbol(":bytes")),
                                Term::Int((b as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )
                })
                .collect();

            let dead_sample: Vec<Term> = dead.iter().take(50).cloned().map(Term::Str).collect();

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":live")),
                Term::Int((live.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":dead")),
                Term::Int((dead.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":reclaim-bytes")),
                Term::Int((dead_bytes as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":roots")), Term::Vector(roots_kind));
            m.insert(
                TermOrdKey(Term::symbol(":largest")),
                Term::Vector(largest_term),
            );
            m.insert(
                TermOrdKey(Term::symbol(":dead-sample")),
                Term::Vector(dead_sample),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gc-low::run" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gc-low::run".to_string())
            })?;

            let base_dir = effective_base_dir(pol)?;
            let lock_s = payload_gc_lock(payload).unwrap_or_else(|| "genesis.lock".to_string());
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let depth = payload_gc_depth(payload).unwrap_or(200);
            let include_lock = payload_gc_include_lock(payload).unwrap_or(true);
            let include_refs = payload_gc_include_refs(payload).unwrap_or(true);
            let quarantine = payload_gc_quarantine(payload).unwrap_or(false);
            let quarantine_dir_s = payload_gc_quarantine_dir(payload);

            let (refs_entries, lock_info, pins_info) = match gc_build_sources(
                refs,
                &base_dir,
                &lock_s,
                &pins_s,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            let (mut roots, _) = match gc_roots_plan_from_sources(
                &refs_entries,
                &lock_info,
                &pins_info,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            roots.sort();
            roots.dedup();

            let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for h in &roots {
                match sync_closure_local(store, h, depth, &mut live, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }

            let store_dir = store.root_dir();
            let _lk = gc_store_lock(store_dir)?;
            let (dead, dead_bytes, _largest) = gc_store_dead_set(store_dir, &live)?;

            let quarantine_dir = if quarantine {
                Some(match quarantine_dir_s {
                    Some(s) => sandbox_path_write(&base_dir, &s, true).map_err(|e| {
                        EffectsError::Log(format!("quarantine dir path error: {e}"))
                    })?,
                    None => store_dir.parent().unwrap_or(store_dir).join("quarantine"),
                })
            } else {
                None
            };
            if let Some(qd) = &quarantine_dir {
                std::fs::create_dir_all(qd)?;
            }

            let mut deleted: u64 = 0;
            let mut quarantined: u64 = 0;
            for h in &dead {
                let p = store_dir.join(h);
                if !p.exists() {
                    continue;
                }
                if let Some(qd) = &quarantine_dir {
                    let qp = qd.join(h);
                    if qp.exists() {
                        continue;
                    }
                    std::fs::rename(&p, &qp)?;
                    quarantined = quarantined.saturating_add(1);
                } else {
                    std::fs::remove_file(&p)?;
                    deleted = deleted.saturating_add(1);
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":dead")),
                Term::Int((dead.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":deleted")),
                Term::Int((deleted as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":quarantined")),
                Term::Int((quarantined as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":reclaimed-bytes")),
                Term::Int((dead_bytes as i64).into()),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gc-low::pin" => {
            let base_dir = effective_base_dir(pol)?;
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let target = payload_gc_target(payload)?;

            let mut pins = gc_pins_load(&base_dir, &pins_s).unwrap_or_else(|_| GcPins::empty());
            if target.starts_with("refs/") {
                if !pins.keep_refs.iter().any(|r| r == &target) {
                    pins.keep_refs.push(target);
                }
            } else {
                let h = gc_normalize_hash(&target).ok_or_else(|| {
                    EffectsError::BadPayload("pin target must be hex hash or refs/...".to_string())
                })?;
                if !pins.keep.iter().any(|x| x == &h) {
                    pins.keep.push(h);
                }
            }
            pins.keep.sort();
            pins.keep.dedup();
            pins.keep_refs.sort();
            pins.keep_refs.dedup();

            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let pins_path = sandbox_path_write(&base_dir, &pins_s, create_dirs)?;
            gc_pins_write(&pins_path, &pins)?;

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":pins")), Term::Str(pins_s));
            m.insert(
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(pins.keep.iter().cloned().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(pins.keep_refs.iter().cloned().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gc-low::unpin" => {
            let base_dir = effective_base_dir(pol)?;
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let target = payload_gc_target(payload)?;

            let mut pins = gc_pins_load(&base_dir, &pins_s).unwrap_or_else(|_| GcPins::empty());
            if target.starts_with("refs/") {
                pins.keep_refs.retain(|r| r != &target);
            } else if let Some(h) = gc_normalize_hash(&target) {
                pins.keep.retain(|x| x != &h);
            } else {
                return Ok(mk_error(
                    error_tok,
                    "core/gc/bad-payload",
                    "unpin target must be hex hash or refs/...".to_string(),
                    Some(op),
                ));
            }
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let pins_path = sandbox_path_write(&base_dir, &pins_s, create_dirs)?;
            gc_pins_write(&pins_path, &pins)?;

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":pins")), Term::Str(pins_s));
            m.insert(
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(pins.keep.iter().cloned().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(pins.keep_refs.iter().cloned().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gc-low::purge" => {
            let base_dir = effective_base_dir(pol)?;
            let ttl_days = payload_gc_ttl_days(payload)
                .ok_or_else(|| EffectsError::BadPayload("missing :ttl-days int".to_string()))?;
            let quarantine_dir_s = payload_gc_quarantine_dir(payload);

            let qd = match quarantine_dir_s {
                Some(s) => sandbox_path_allow_missing(&base_dir, &s, false)?,
                None => {
                    let store = store.ok_or_else(|| {
                        EffectsError::Log(
                            "missing artifact store for core/gc-low::purge".to_string(),
                        )
                    })?;
                    store
                        .root_dir()
                        .parent()
                        .unwrap_or(store.root_dir())
                        .join("quarantine")
                }
            };
            if !qd.exists() {
                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
                m.insert(TermOrdKey(Term::symbol(":purged")), Term::Int(0.into()));
                return Ok(Value::Data(Term::Map(m)));
            }

            let now = std::time::SystemTime::now();
            let ttl = std::time::Duration::from_secs(ttl_days.saturating_mul(86_400));

            let mut purged: u64 = 0;
            for ent in std::fs::read_dir(&qd)? {
                let ent = ent?;
                let p = ent.path();
                let ft = ent.file_type()?;
                if !ft.is_file() {
                    continue;
                }
                let name = ent.file_name().to_string_lossy().to_string();
                if gc_vcs::validate_hex_hash(&name).is_err() {
                    continue;
                }
                let meta = ent.metadata()?;
                if let Ok(mtime) = meta.modified()
                    && let Ok(age) = now.duration_since(mtime)
                    && age >= ttl
                {
                    let _ = std::fs::remove_file(&p);
                    purged = purged.saturating_add(1);
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":purged")),
                Term::Int((purged as i64).into()),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gpk-low::export" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gpk-low::export".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/gpk-low::export".to_string())
            });
            let root_spec = match payload_gpk_root(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let out_path_s = match payload_gpk_out(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let out_path = sandbox_path_write(&base_dir, &out_path_s, create_dirs)?;

            let mode = match payload_gpk_mode(payload) {
                Ok(Some(m)) => m,
                Ok(None) => ":shallow".to_string(),
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mode = match mode.as_str() {
                ":shallow" => GpkMode::Shallow,
                ":full" => GpkMode::Full,
                other => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("unsupported :mode {other}"),
                        Some(op),
                    ));
                }
            };
            let depth = payload_gpk_depth(payload).unwrap_or(0);
            let include_evidence = match payload_gpk_include_evidence(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let include_deps = match payload_gpk_include_deps(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let embed_refnames = match payload_gpk_refs(payload) {
                Ok(xs) => xs,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let resolved_root = match resolve_gpk_root_for_export(
                store,
                refs.as_ref().ok().copied(),
                &root_spec,
                mode,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let root_term = match store_get_term(store, &resolved_root) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {resolved_root}"),
                        Some(op),
                    ));
                }
            };

            if mode == GpkMode::Shallow
                && let Err(e) = gc_vcs::Snapshot::from_term(&root_term)
            {
                return Ok(mk_error(
                    error_tok,
                    "core/gpk/bad-root",
                    format!("{e}"),
                    Some(op),
                ));
            }

            let root_snapshot_for_locked_deps = match mode {
                GpkMode::Shallow => Some(resolved_root.clone()),
                GpkMode::Full => {
                    if let Ok(c) = gc_vcs::Commit::from_term(&root_term) {
                        Some(c.result)
                    } else if gc_vcs::Snapshot::from_term(&root_term).is_ok() {
                        Some(resolved_root.clone())
                    } else {
                        None
                    }
                }
            };
            let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            match gpk_export_closure_local(
                store,
                &resolved_root,
                GpkClosureOptions {
                    depth: if mode == GpkMode::Shallow { 0 } else { depth },
                    mode,
                    include_evidence,
                    include_deps,
                    root_snapshot_for_locked_deps: root_snapshot_for_locked_deps.as_deref(),
                },
                &mut all,
                error_tok,
                op,
            ) {
                Ok(()) => {}
                Err(v) => return Ok(v),
            }
            let hashes: Vec<String> = all.into_iter().collect();

            let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
            for h in &hashes {
                if !store.path_for(h).exists() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {h}"),
                        Some(op),
                    ));
                }
                if store.verify_hex(h).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {h}"),
                        Some(op),
                    ));
                }
                let bytes = match store.get_bytes(h) {
                    Ok(b) => b,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                entries.push((h.clone(), bytes));
            }

            let root_b = match gc_vcs::hex_to_bytes32(&resolved_root) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/gpk/bad-root", e, Some(op)));
                }
            };

            let mut refs_section: Vec<(String, String)> = Vec::new();
            let bundle_version: u32 = if embed_refnames.is_empty() { 1 } else { 2 };
            if bundle_version == 2 {
                let refs = match refs {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/gpk/missing-refs-db",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                for name in &embed_refnames {
                    let cur = match refs.get(name) {
                        Ok(h) => h,
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/gpk/refs-io-error",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let Some(h) = cur else {
                        return Ok(mk_error(
                            error_tok,
                            "core/gpk/ref-not-found",
                            format!("ref not found: {name}"),
                            Some(op),
                        ));
                    };
                    refs_section.push((name.clone(), h));
                }
                refs_section.sort_by(|a, b| a.0.cmp(&b.0));
                refs_section.dedup_by(|a, b| a.0 == b.0);
            }

            let mut file = match std::fs::File::create(&out_path) {
                Ok(f) => f,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let bundle_h = {
                let mut hw = HashingWriter::new(&mut file);
                let refs_opt = if bundle_version == 2 {
                    Some(refs_section.as_slice())
                } else {
                    None
                };
                if let Err(e) =
                    gc_vcs::write_bundle(&mut hw, bundle_version, root_b, &entries, refs_opt)
                {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/write-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
                hw.finish_hex()
            };
            if let Err(e) = file.sync_all() {
                return Ok(mk_error(
                    error_tok,
                    "core/gpk/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":ok".to_string())),
                Term::Bool(true),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":bundle-h".to_string())),
                Term::Str(bundle_h),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":bundle-v".to_string())),
                Term::Int((bundle_version as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":root".to_string())),
                Term::Str(resolved_root),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":count".to_string())),
                Term::Int((hashes.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":include-evidence")),
                Term::Symbol(include_evidence.to_symbol().to_string()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":include-deps")),
                Term::Symbol(include_deps.to_symbol().to_string()),
            );
            if bundle_version == 2 {
                let out_refs: Vec<Term> = refs_section
                    .iter()
                    .map(|(n, h)| {
                        Term::Map(
                            [
                                (TermOrdKey(Term::symbol(":name")), Term::Str(n.clone())),
                                (TermOrdKey(Term::symbol(":hash")), Term::Str(h.clone())),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .collect();
                m.insert(TermOrdKey(Term::symbol(":refs")), Term::Vector(out_refs));
            }
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gpk-low::import" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gpk-low::import".to_string())
            })?;
            let set_refs = match payload_gpk_set_refs(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let in_path_s = match payload_gpk_in(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let refs_db = if set_refs.is_empty() {
                None
            } else {
                Some(refs.ok_or_else(|| {
                    EffectsError::Log("missing refs db for core/gpk-low::import".to_string())
                })?)
            };
            let base_dir = effective_base_dir(pol)?;
            let in_path = sandbox_path_read(&base_dir, &in_path_s)?;
            let mut f = match std::fs::File::open(&in_path) {
                Ok(f) => f,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let max_entries = match op_extra_positive_usize(pol, "max_bundle_entries") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let max_entry_bytes = match op_extra_positive_usize(pol, "max_bundle_entry_bytes") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let max_bundle_bytes = match op_extra_positive_usize(pol, "max_bundle_bytes") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let max_refs = match op_extra_positive_usize(pol, "max_bundle_refs") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            let mut limits = gc_vcs::GpkReadLimits::default_hard();
            if let Some(v) = max_entries {
                limits.max_entries = (v as u64).min(limits.max_entries);
            }
            if let Some(v) = max_entry_bytes {
                limits.max_entry_bytes = (v as u64).min(limits.max_entry_bytes);
            }
            if let Some(v) = max_bundle_bytes {
                limits.max_total_bytes = (v as u64).min(limits.max_total_bytes);
            }
            if let Some(v) = max_refs {
                limits.max_refs = (v as u64).min(limits.max_refs);
            }

            let bundle = match gc_vcs::read_bundle_with_limits(&mut f, &limits) {
                Ok(b) => b,
                Err(e) => {
                    if matches!(e, gc_vcs::GpkError::LimitExceeded(_)) {
                        return Ok(mk_error(
                            error_tok,
                            "core/caps/resource-limit",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/read-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let root_hex = gc_vcs::bytes32_to_hex(&bundle.root);

            for e in &bundle.entries {
                let expected = gc_vcs::bytes32_to_hex(&e.hash);
                let got =
                    match store_put_with_budget(store, &e.bytes, policy, budget, error_tok, op) {
                        Ok(h) => h,
                        Err(v) => return Ok(v),
                    };
                if got != expected {
                    return Ok(mk_error(
                        error_tok,
                        "core/gpk/hash-mismatch",
                        "bundle entry hash mismatch".to_string(),
                        Some(op),
                    ));
                }
            }

            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":ok".to_string())),
                Term::Bool(true),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":root".to_string())),
                Term::Str(root_hex),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":bundle-v".to_string())),
                Term::Int((bundle.version as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":count".to_string())),
                Term::Int((bundle.entries.len() as i64).into()),
            );
            if !bundle.refs.is_empty() {
                let mut rs: Vec<Term> = Vec::new();
                for rr in &bundle.refs {
                    rs.push(Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":name")),
                                Term::Str(rr.name.clone()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":hash")),
                                Term::Str(gc_vcs::bytes32_to_hex(&rr.hash)),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ));
                }
                m.insert(TermOrdKey(Term::symbol(":refs")), Term::Vector(rs));
            }
            if let Some(refs_db) = refs_db {
                let mut sorted = set_refs;
                sorted.sort_by(|a, b| a.name.cmp(&b.name));
                let mut ops: Vec<SetInput> = Vec::with_capacity(sorted.len());
                for sr in &sorted {
                    if let Err(v) = local_refs_validate_policy_gate(
                        store,
                        &sr.name,
                        sr.hash.as_deref(),
                        &sr.policy,
                        error_tok,
                        op,
                    ) {
                        return Ok(v);
                    }
                    ops.push(SetInput {
                        name: sr.name.clone(),
                        new_hash: sr.hash.clone(),
                        expected_old: sr.expected_old.clone(),
                    });
                }
                match refs_db.set_many(&ops)? {
                    SetManyResult::Updated => {
                        m.insert(
                            TermOrdKey(Term::symbol(":refs-updated")),
                            Term::Int((ops.len() as i64).into()),
                        );
                    }
                    SetManyResult::Conflict { name, current } => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/refs/conflict",
                            "ref update conflict".to_string(),
                            Some(op),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::Symbol(":refs/name".to_string())),
                                        Term::Str(name),
                                    ),
                                    (
                                        TermOrdKey(Term::Symbol(":refs/current".to_string())),
                                        current.map(Term::Str).unwrap_or(Term::Nil),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                }
            }
            Ok(Value::Data(Term::Map(m)))
        }
        "core/refs::get" => {
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::get".to_string())
            })?;
            let name = payload_refs_name(payload)?;
            let h = refs.get(&name)?;
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":name".to_string())),
                Term::Str(name),
            );
            m.insert(
                TermOrdKey(Term::Symbol(":hash".to_string())),
                h.map(Term::Str).unwrap_or(Term::Nil),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/refs::list" => {
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::list".to_string())
            })?;
            let prefix = payload_refs_prefix(payload)?;
            let xs = refs.list(prefix.as_deref())?;
            let mut out = Vec::new();
            for e in xs {
                let mut m = BTreeMap::new();
                m.insert(
                    TermOrdKey(Term::Symbol(":name".to_string())),
                    Term::Str(e.name),
                );
                m.insert(
                    TermOrdKey(Term::Symbol(":hash".to_string())),
                    e.hash.map(Term::Str).unwrap_or(Term::Nil),
                );
                out.push(Term::Map(m));
            }
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":refs".to_string())),
                Term::Vector(out),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/refs::set" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/refs::set".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::set".to_string())
            })?;

            let name = payload_refs_name(payload)?;
            let new_hash = payload_refs_hash(payload)?;
            let expected_old = payload_refs_expected_old(payload)?;
            let policy_h = payload_refs_policy_hash(payload)?;
            let result = match local_refs_set_policy_gated(
                store,
                refs,
                LocalRefSetRequest {
                    name: &name,
                    new_hash: new_hash.as_deref(),
                    expected_old: expected_old.as_ref().map(|x| x.as_deref()),
                    policy_h: &policy_h,
                },
                error_tok,
                op,
            ) {
                Ok(r) => r,
                Err(v) => return Ok(v),
            };

            match result {
                SetResult::Updated => {
                    let mut m = BTreeMap::new();
                    m.insert(
                        TermOrdKey(Term::Symbol(":ok".to_string())),
                        Term::Bool(true),
                    );
                    m.insert(
                        TermOrdKey(Term::Symbol(":name".to_string())),
                        Term::Str(name),
                    );
                    m.insert(
                        TermOrdKey(Term::Symbol(":hash".to_string())),
                        new_hash.map(Term::Str).unwrap_or(Term::Nil),
                    );
                    Ok(Value::Data(Term::Map(m)))
                }
                SetResult::Conflict { current } => Ok(mk_error_with_ctx(
                    error_tok,
                    "core/refs/conflict",
                    "ref update conflict".to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::Symbol(":refs/current".to_string())),
                            current.map(Term::Str).unwrap_or(Term::Nil),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                )),
            }
        }
        "core/refs::delete" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/refs::delete".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/refs::delete".to_string())
            })?;
            let name = payload_refs_name(payload)?;
            let expected_old = payload_refs_expected_old(payload)?;
            let policy_h = payload_refs_policy_hash(payload)?;
            let result = match local_refs_set_policy_gated(
                store,
                refs,
                LocalRefSetRequest {
                    name: &name,
                    new_hash: None,
                    expected_old: expected_old.as_ref().map(|x| x.as_deref()),
                    policy_h: &policy_h,
                },
                error_tok,
                op,
            ) {
                Ok(r) => r,
                Err(v) => return Ok(v),
            };

            match result {
                SetResult::Updated => {
                    let mut m = BTreeMap::new();
                    m.insert(
                        TermOrdKey(Term::Symbol(":ok".to_string())),
                        Term::Bool(true),
                    );
                    m.insert(
                        TermOrdKey(Term::Symbol(":name".to_string())),
                        Term::Str(name),
                    );
                    Ok(Value::Data(Term::Map(m)))
                }
                SetResult::Conflict { current } => Ok(mk_error_with_ctx(
                    error_tok,
                    "core/refs/conflict",
                    "ref delete conflict".to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::Symbol(":refs/current".to_string())),
                            current.map(Term::Str).unwrap_or(Term::Nil),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                )),
            }
        }
        "sys/time::now" => {
            if let Some(ms) = timeout_ms {
                let r = with_timeout(ms, || {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis())
                })?;
                return Ok(match r {
                    Some(t) => Value::Data(Term::Int(BigInt::from(t))),
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: sys/time::now"),
                        Some(op),
                    ),
                });
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            Ok(Value::Data(Term::Int(BigInt::from(t))))
        }
        "gfx/time::frame-tick" => {
            if let Some(ms) = timeout_ms {
                let r = with_timeout(ms, || {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis())
                })?;
                return Ok(match r {
                    Some(t) => {
                        let mut m = BTreeMap::new();
                        m.insert(
                            TermOrdKey(Term::Symbol(":time-ms".to_string())),
                            Term::Int(BigInt::from(t)),
                        );
                        Value::Data(Term::Map(m))
                    }
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: gfx/time::frame-tick"),
                        Some(op),
                    ),
                });
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":time-ms".to_string())),
                Term::Int(BigInt::from(t)),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "io/fs::read" => {
            let path_s = payload_path(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let max_read_bytes = match op_extra_positive_usize(pol, "max_bytes") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            if let Some(ms) = timeout_ms {
                let base_dir2 = base_dir.clone();
                let path_s2 = path_s.clone();
                let max_read_bytes2 = max_read_bytes;
                let r = with_timeout_cancellable(ms, move |cancel| {
                    let path = sandbox_path_read(&base_dir2, &path_s2)?;
                    let bytes =
                        read_file_with_optional_limit(&path, max_read_bytes2, Some(&cancel));
                    Ok((path, bytes))
                })?;
                return Ok(match r {
                    Some((_path, Ok(bytes))) => Value::Data(Term::Bytes(bytes.into())),
                    Some((path, Err(FsReadError::Io(e)))) => Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                    },
                    Some((path, Err(FsReadError::LimitExceeded { observed, limit }))) => {
                        mk_error_with_ctx(
                            error_tok,
                            "core/caps/resource-limit",
                            format!(
                                "file read exceeds configured limit ({observed} > {limit} bytes)"
                            ),
                            Some(op),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":path")),
                                        Term::Str(path_to_slash(
                                            path.strip_prefix(&base_dir).unwrap_or(&path),
                                        )),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":limit-bytes")),
                                        Term::Int((limit as i64).into()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        )
                    }
                    Some((_path, Err(FsReadError::Cancelled))) => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                });
            }
            let path = sandbox_path_read(&base_dir, &path_s)?;
            match read_file_with_optional_limit(&path, max_read_bytes, None) {
                Ok(bytes) => Ok(Value::Data(Term::Bytes(bytes.into()))),
                Err(FsReadError::Io(e)) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
                Err(FsReadError::LimitExceeded { observed, limit }) => Ok(mk_error_with_ctx(
                    error_tok,
                    "core/caps/resource-limit",
                    format!("file read exceeds configured limit ({observed} > {limit} bytes)"),
                    Some(op),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":path")),
                                Term::Str(path_to_slash(
                                    path.strip_prefix(&base_dir).unwrap_or(&path),
                                )),
                            ),
                            (
                                TermOrdKey(Term::symbol(":limit-bytes")),
                                Term::Int((limit as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                )),
                Err(FsReadError::Cancelled) => Ok(mk_error(
                    error_tok,
                    "core/caps/timeout",
                    "io/fs::read cancelled".to_string(),
                    Some(op),
                )),
            }
        }
        "io/fs::write" => {
            let path_s = payload_path(payload)?;
            let data = payload_data(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.is_some_and(|p| p.create_dirs);
            let path = sandbox_path_write(&base_dir, &path_s, create_dirs)?;
            match write_file_no_follow(&path, &data) {
                Ok(()) => Ok(Value::Data(Term::Nil)),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
            }
        }
        "gfx/gpu::create-buffer"
        | "core/task::channel-close"
        | "core/task::channel-open"
        | "core/task::channel-recv"
        | "core/task::channel-send"
        | "core/task::channel-status"
        | "core/task::spawn"
        | "core/task::await"
        | "core/task::cancel"
        | "core/task::status"
        | "core/task::scope"
        | "gfx/gpu::create-texture"
        | "gfx/gpu::create-sampler"
        | "gfx/gpu::create-shader-module"
        | "gfx/gpu::create-bind-group-layout"
        | "gfx/gpu::create-bind-group"
        | "gfx/gpu::create-pipeline-layout"
        | "gfx/gpu::create-render-pipeline"
        | "gfx/gpu::create-compute-pipeline"
        | "gpu/compute::create-buffer"
        | "gpu/compute::create-shader-module"
        | "gpu/compute::create-bind-group-layout"
        | "gpu/compute::create-bind-group"
        | "gpu/compute::create-pipeline-layout"
        | "gpu/compute::create-compute-pipeline"
        | "gpu/compute::create-kernel"
        | "gfx/gpu::destroy-resource"
        | "gpu/compute::destroy-resource"
        | "gfx/gpu::write-buffer"
        | "gpu/compute::write-buffer"
        | "gfx/gpu::write-texture"
        | "gfx/gpu::read-buffer"
        | "gpu/compute::read-buffer"
        | "gfx/gpu::read-texture"
        | "gfx/gpu::submit-frame-graph"
        | "gfx/gpu::submit-compute-graph"
        | "gpu/compute::submit"
        | "gfx/gpu::limits"
        | "gpu/compute::limits"
        | "gfx/gpu::features"
        | "gpu/compute::features"
        | "gfx/window::create-surface"
        | "gfx/window::resize-surface"
        | "gfx/window::set-title"
        | "gfx/window::request-redraw"
        | "gfx/window::surface-info"
        | "gfx/input::poll-events"
        | "gfx/input::set-cursor-mode"
        | "gfx/audio::enqueue"
        | "gfx/audio::set-master"
        | "editor/clipboard::get"
        | "editor/clipboard::set"
        | "editor/dialog::open"
        | "editor/dialog::save"
        | "editor/plugin::command"
        | "editor/task::spawn"
        | "editor/task::fmt-coreform"
        | "editor/task::lint-module"
        | "editor/task::optimize-module"
        | "editor/task::parse-module"
        | "editor/task::poll"
        | "editor/task::cancel"
        | "editor/task::test-pkg"
        | "editor/task::typecheck-pkg"
        | "editor/watch::subscribe"
        | "editor/watch::poll"
        | "editor/watch::unsubscribe" => Ok(mk_error(
            error_tok,
            "core/caps/not-supported",
            format!("capability not supported in this host runtime: {op}"),
            Some(op),
        )),
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        )),
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
