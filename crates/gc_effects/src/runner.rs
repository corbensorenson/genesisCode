use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
use crate::store::ArtifactStore;

type GcStoreLock = ExclusiveLock;

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TaskScheduleEvent {
    task_id: Option<String>,
    parent_task: Option<String>,
    schedule_step: Option<u64>,
    await_edge: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct TaskBudgetState {
    per_task: BTreeMap<String, TaskBudgetCounters>,
}

#[derive(Debug, Clone)]
struct TaskBudgetCounters {
    first_step: u64,
    last_step: u64,
    steps: u64,
}

#[derive(Debug, Clone, Default)]
struct TaskRuntime {
    next_task_id: u64,
    tasks: BTreeMap<String, TaskRecord>,
    queue: Vec<String>,
}

#[derive(Debug, Clone)]
struct TaskRecord {
    state: TaskState,
    payload: Term,
    parent_task: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState {
    Queued,
    Running,
    Done,
    Cancelled,
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
                    } else {
                        call_capability(
                            &req.op,
                            &req.payload,
                            pol,
                            policy,
                            store.as_ref(),
                            refs.as_ref(),
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

fn task_runtime_call(
    runtime: &mut TaskRuntime,
    policy: &CapsPolicy,
    op: &str,
    payload: &Term,
    error_tok: SealId,
) -> Option<Value> {
    match op {
        "core/task::scope" => {
            let scope = map_field_str_or_symbol(payload, ":scope")
                .map(Term::Str)
                .unwrap_or(Term::Nil);
            Some(Value::Data(task_map([
                (":scope", scope),
                (":state", Term::symbol(":entered")),
            ])))
        }
        "core/task::spawn" => {
            let task_id = format!("task-{:016x}", runtime.next_task_id);
            runtime.next_task_id = runtime.next_task_id.saturating_add(1);
            let parent_task = map_field_str_or_symbol(payload, ":parent-task")
                .or_else(|| map_field_str_or_symbol(payload, ":scope"));
            let task_payload = map_field(payload, ":payload").cloned().unwrap_or(Term::Nil);
            let running = runtime_running_count(runtime);
            let worker_budget = policy.task.max_workers.unwrap_or(u64::MAX);
            let state = if running < worker_budget {
                TaskState::Running
            } else {
                runtime.queue.push(task_id.clone());
                TaskState::Queued
            };
            runtime.tasks.insert(
                task_id.clone(),
                TaskRecord {
                    state: state.clone(),
                    payload: task_payload,
                    parent_task,
                },
            );
            Some(Value::Data(task_map([
                (":task-id", Term::Str(task_id)),
                (":state", task_state_term(&state)),
            ])))
        }
        "core/task::status" => {
            let Some(task_id) = map_field_str_or_symbol(payload, ":task-id") else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::status payload must include :task-id".to_string(),
                    Some(op),
                ));
            };
            let Some(rec) = runtime.tasks.get(&task_id) else {
                return Some(mk_error(
                    error_tok,
                    "core/task/not-found",
                    format!("unknown task-id: {task_id}"),
                    Some(op),
                ));
            };
            Some(Value::Data(task_map([
                (":task-id", Term::Str(task_id)),
                (":state", task_state_term(&rec.state)),
                (
                    ":parent-task",
                    rec.parent_task.clone().map(Term::Str).unwrap_or(Term::Nil),
                ),
            ])))
        }
        "core/task::cancel" => {
            let Some(task_id) = map_field_str_or_symbol(payload, ":task-id") else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::cancel payload must include :task-id".to_string(),
                    Some(op),
                ));
            };
            if !runtime.tasks.contains_key(&task_id) {
                return Some(mk_error(
                    error_tok,
                    "core/task/not-found",
                    format!("unknown task-id: {task_id}"),
                    Some(op),
                ));
            }
            let state_before = runtime
                .tasks
                .get(&task_id)
                .map(|r| r.state.clone())
                .expect("task-id existence checked");
            if state_before == TaskState::Queued {
                runtime.queue.retain(|q| q != &task_id);
            }
            if (state_before == TaskState::Running || state_before == TaskState::Queued)
                && let Some(rec) = runtime.tasks.get_mut(&task_id)
            {
                rec.state = TaskState::Cancelled;
            }
            if state_before == TaskState::Running {
                promote_queued_task(runtime, policy);
            }
            let rec = runtime
                .tasks
                .get(&task_id)
                .expect("task-id existence checked");
            Some(Value::Data(task_map([
                (":task-id", Term::Str(task_id)),
                (":state", task_state_term(&rec.state)),
                (
                    ":parent-task",
                    rec.parent_task.clone().map(Term::Str).unwrap_or(Term::Nil),
                ),
            ])))
        }
        "core/task::await" => {
            let Some(task_id) = map_field_str_or_symbol(payload, ":task-id") else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::await payload must include :task-id".to_string(),
                    Some(op),
                ));
            };
            if !runtime.tasks.contains_key(&task_id) {
                return Some(mk_error(
                    error_tok,
                    "core/task/not-found",
                    format!("unknown task-id: {task_id}"),
                    Some(op),
                ));
            }
            let state_before = runtime
                .tasks
                .get(&task_id)
                .map(|r| r.state.clone())
                .expect("task-id existence checked");
            if state_before == TaskState::Queued {
                runtime.queue.retain(|q| q != &task_id);
            }
            if (state_before == TaskState::Running || state_before == TaskState::Queued)
                && let Some(rec) = runtime.tasks.get_mut(&task_id)
            {
                rec.state = TaskState::Done;
            }
            if state_before == TaskState::Running {
                promote_queued_task(runtime, policy);
            }
            let rec = runtime
                .tasks
                .get(&task_id)
                .expect("task-id existence checked");
            let (result, error) = match rec.state {
                TaskState::Done => (rec.payload.clone(), Term::Nil),
                TaskState::Cancelled | TaskState::Running | TaskState::Queued => {
                    (Term::Nil, Term::Nil)
                }
            };
            Some(Value::Data(task_map([
                (":task-id", Term::Str(task_id)),
                (":state", task_state_term(&rec.state)),
                (
                    ":parent-task",
                    rec.parent_task.clone().map(Term::Str).unwrap_or(Term::Nil),
                ),
                (":result", result),
                (":error", error),
            ])))
        }
        _ => None,
    }
}

fn task_state_term(state: &TaskState) -> Term {
    match state {
        TaskState::Queued => Term::symbol(":queued"),
        TaskState::Running => Term::symbol(":running"),
        TaskState::Done => Term::symbol(":done"),
        TaskState::Cancelled => Term::symbol(":cancelled"),
    }
}

fn runtime_running_count(runtime: &TaskRuntime) -> u64 {
    runtime
        .tasks
        .values()
        .filter(|t| t.state == TaskState::Running)
        .count() as u64
}

fn runtime_queue_count(runtime: &TaskRuntime) -> u64 {
    runtime.queue.len() as u64
}

fn promote_queued_task(runtime: &mut TaskRuntime, policy: &CapsPolicy) {
    let worker_budget = policy.task.max_workers.unwrap_or(u64::MAX);
    while runtime_running_count(runtime) < worker_budget {
        let Some(next_idx) = runtime.queue.iter().position(
            |tid| matches!(runtime.tasks.get(tid), Some(rec) if rec.state == TaskState::Queued),
        ) else {
            break;
        };
        let tid = runtime.queue.remove(next_idx);
        if let Some(rec) = runtime.tasks.get_mut(&tid)
            && rec.state == TaskState::Queued
        {
            rec.state = TaskState::Running;
        }
    }
}

fn task_map<const N: usize>(pairs: [(&str, Term); N]) -> Term {
    Term::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (TermOrdKey(Term::symbol(k)), v))
            .collect(),
    )
}

fn task_schedule_event_for(
    i: u64,
    op: &str,
    payload: &Term,
    resp_val: &Value,
) -> TaskScheduleEvent {
    let mut out = TaskScheduleEvent::default();
    if is_task_like_op(op) {
        out.schedule_step = Some(i);
    }
    match op {
        "core/task::spawn" | "editor/task::spawn" => {
            out.parent_task = map_field_str_or_symbol(payload, ":parent-task")
                .or_else(|| map_field_str_or_symbol(payload, ":scope"));
            out.task_id = value_data_map_field(resp_val, ":task-id");
        }
        "core/task::await" => {
            let tid = map_field_str_or_symbol(payload, ":task-id");
            out.task_id = tid.clone();
            out.await_edge = tid;
        }
        "core/task::cancel" | "core/task::status" | "editor/task::poll" | "editor/task::cancel" => {
            out.task_id = map_field_str_or_symbol(payload, ":task-id");
        }
        "core/task::scope" => {
            out.parent_task = map_field_str_or_symbol(payload, ":scope");
        }
        _ => {}
    }
    out
}

fn task_target_id(op: &str, payload: &Term, resp_val: &Value) -> Option<String> {
    match op {
        "core/task::spawn" | "editor/task::spawn" => value_data_map_field(resp_val, ":task-id"),
        "core/task::await"
        | "core/task::cancel"
        | "core/task::status"
        | "editor/task::poll"
        | "editor/task::cancel" => map_field_str_or_symbol(payload, ":task-id"),
        _ => None,
    }
}

fn enforce_task_policy_limits(
    policy: &CapsPolicy,
    state: &mut TaskBudgetState,
    runtime: &TaskRuntime,
    i: u64,
    op: &str,
    payload: &Term,
    resp_val: &Value,
    error_tok: SealId,
) -> Option<Value> {
    if !is_task_like_op(op) {
        return None;
    }

    let tid = task_target_id(op, payload, resp_val);
    if let Some(tid) = &tid {
        let c = state
            .per_task
            .entry(tid.clone())
            .or_insert(TaskBudgetCounters {
                first_step: i,
                last_step: i,
                steps: 0,
            });
        c.steps = c.steps.saturating_add(1);
        c.last_step = i;
    }

    if let Some(max_tasks) = policy.task.max_tasks
        && (state.per_task.len() as u64) > max_tasks
    {
        return Some(task_limit_error(
            error_tok,
            "max_tasks",
            max_tasks,
            state.per_task.len() as u64,
            op,
            tid.as_deref(),
        ));
    }
    if let Some(max_workers) = policy.task.max_workers
        && runtime_running_count(runtime) > max_workers
    {
        return Some(task_limit_error(
            error_tok,
            "max_workers",
            max_workers,
            runtime_running_count(runtime),
            op,
            tid.as_deref(),
        ));
    }
    if let Some(max_queue) = policy.task.max_queue
        && runtime_queue_count(runtime) > max_queue
    {
        return Some(task_limit_error(
            error_tok,
            "max_queue",
            max_queue,
            runtime_queue_count(runtime),
            op,
            tid.as_deref(),
        ));
    }
    if let Some(max_steps) = policy.task.max_steps_per_task
        && let Some(tid) = &tid
        && let Some(c) = state.per_task.get(tid)
        && c.steps > max_steps
    {
        return Some(task_limit_error(
            error_tok,
            "max_steps_per_task",
            max_steps,
            c.steps,
            op,
            Some(tid),
        ));
    }
    if let Some(max_time_ms) = policy.task.max_time_ms_per_task
        && let Some(tid) = &tid
        && let Some(c) = state.per_task.get(tid)
    {
        // Deterministic logical elapsed time in effect-steps (not wall clock).
        let logical_elapsed = c.last_step.saturating_sub(c.first_step);
        if logical_elapsed > max_time_ms {
            return Some(task_limit_error(
                error_tok,
                "max_time_ms_per_task",
                max_time_ms,
                logical_elapsed,
                op,
                Some(tid),
            ));
        }
    }

    None
}

fn task_limit_error(
    error_tok: SealId,
    budget: &str,
    limit: u64,
    observed: u64,
    op: &str,
    task_id: Option<&str>,
) -> Value {
    mk_error_with_ctx(
        error_tok,
        "core/task/budget-exceeded",
        format!("task policy limit exceeded: {budget} observed {observed} > {limit} for {op}"),
        Some(op),
        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":task/budget")),
                    Term::Str(budget.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":task/limit")),
                    Term::Int(BigInt::from(limit)),
                ),
                (
                    TermOrdKey(Term::symbol(":task/observed")),
                    Term::Int(BigInt::from(observed)),
                ),
                (
                    TermOrdKey(Term::symbol(":task/id")),
                    task_id
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    )
}

fn is_task_like_op(op: &str) -> bool {
    op.starts_with("core/task::") || op.starts_with("editor/task::")
}

fn map_field<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn map_field_str_or_symbol(t: &Term, key: &str) -> Option<String> {
    match map_field(t, key) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn value_data_map_field(v: &Value, key: &str) -> Option<String> {
    let Value::Data(Term::Map(m)) = v else {
        return None;
    };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn unseal_effect_request(
    v: &Value,
    effect_tok: SealId,
) -> Result<(EffectRequest, SealId), EffectsError> {
    let Value::Sealed { token, payload } = v else {
        return Err(EffectsError::BadEffectSeal);
    };
    if *token != effect_tok {
        return Err(EffectsError::BadEffectSeal);
    }
    let Value::EffectRequest(r) = payload.as_ref() else {
        return Err(EffectsError::BadEffectSeal);
    };
    Ok((r.clone(), *token))
}

fn hash_request(op: &str, payload_h: [u8; 32], cont_h: [u8; 32]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(b"GCv0.2\0effect-req\0");
    h.update(op.as_bytes());
    h.update(b"\0");
    h.update(&payload_h);
    h.update(&cont_h);
    *h.finalize().as_bytes()
}

fn mk_caps_denied(error_tok: SealId, op: &str) -> Value {
    mk_error(
        error_tok,
        "core/caps/denied",
        format!("capability denied: {op}"),
        Some(op),
    )
}

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":error/code".to_string())),
        Term::Str(code.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/message".to_string())),
        Term::Str(msg),
    );
    let mut ctxm = BTreeMap::new();
    ctxm.insert(
        TermOrdKey(Term::Symbol(":subsystem".to_string())),
        Term::Str("effects".to_string()),
    );
    if let Some(op) = op {
        m.insert(
            TermOrdKey(Term::Symbol(":error/op".to_string())),
            Term::Symbol(op.to_string()),
        );
        ctxm.insert(
            TermOrdKey(Term::Symbol(":op".to_string())),
            Term::Symbol(op.to_string()),
        );
    }
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(ctxm),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(m))),
    }
}

fn mk_error_with_ctx(
    error_tok: SealId,
    code: &str,
    msg: String,
    op: Option<&str>,
    extra_ctx: Term,
) -> Value {
    let Value::Sealed { token, payload } = mk_error(error_tok, code, msg, op) else {
        unreachable!("mk_error must return sealed");
    };
    let Value::Data(Term::Map(mut m)) = *payload else {
        return Value::Sealed {
            token,
            payload: Box::new(Value::Data(Term::Map(BTreeMap::new()))),
        };
    };
    let mut ctxm = match m.remove(&TermOrdKey(Term::Symbol(":error/context".to_string()))) {
        Some(Term::Map(mm)) => mm,
        _ => BTreeMap::new(),
    };
    if let Term::Map(extra) = extra_ctx {
        for (k, v) in extra {
            ctxm.insert(k, v);
        }
    }
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(ctxm),
    );
    Value::Sealed {
        token,
        payload: Box::new(Value::Data(Term::Map(m))),
    }
}

fn logged_resp(
    policy: &CapsPolicy,
    op: &str,
    store: &Option<ArtifactStore>,
    v: &Value,
    error_tok: SealId,
) -> Result<LoggedResp, EffectsError> {
    let resp = logged_resp_inline(v, error_tok)?;
    externalize_resp(policy, op, store.as_ref(), resp)
}

fn logged_resp_inline(v: &Value, error_tok: SealId) -> Result<LoggedResp, EffectsError> {
    match v {
        Value::Data(t) => Ok(LoggedResp::Ok(t.clone())),
        Value::Sealed { token, payload } if *token == error_tok => {
            let Value::Data(t) = payload.as_ref() else {
                return Err(EffectsError::Log(
                    "sealed ERROR payload must be a datum for logging".to_string(),
                ));
            };
            Ok(LoggedResp::Error(t.clone()))
        }
        _ => Err(EffectsError::Log(format!(
            "response not serializable: {}",
            v.debug_repr()
        ))),
    }
}

fn externalize_resp(
    policy: &CapsPolicy,
    op: &str,
    store: Option<&ArtifactStore>,
    resp: LoggedResp,
) -> Result<LoggedResp, EffectsError> {
    let Some(max_inline) = policy.inline_max_bytes_for(op) else {
        return Ok(resp);
    };
    let Some(store) = store else {
        return Err(EffectsError::Log(
            "caps.toml sets log inline_max_bytes but no store_dir is configured".to_string(),
        ));
    };

    match resp {
        LoggedResp::Ok(Term::Bytes(b)) => {
            if b.len() <= max_inline {
                Ok(LoggedResp::Ok(Term::Bytes(b)))
            } else {
                let hex = store.put_bytes(&b)?;
                Ok(LoggedResp::OkBytesArtifact { artifact: hex })
            }
        }
        LoggedResp::Error(Term::Bytes(b)) => {
            if b.len() <= max_inline {
                Ok(LoggedResp::Error(Term::Bytes(b)))
            } else {
                let hex = store.put_bytes(&b)?;
                Ok(LoggedResp::ErrorBytesArtifact { artifact: hex })
            }
        }
        LoggedResp::Ok(t) => {
            let s = print_term(&t);
            if s.len() <= max_inline {
                Ok(LoggedResp::Ok(t))
            } else {
                let hex = store.put_bytes(s.as_bytes())?;
                Ok(LoggedResp::OkArtifact { artifact: hex })
            }
        }
        LoggedResp::Error(t) => {
            let s = print_term(&t);
            if s.len() <= max_inline {
                Ok(LoggedResp::Error(t))
            } else {
                let hex = store.put_bytes(s.as_bytes())?;
                Ok(LoggedResp::ErrorArtifact { artifact: hex })
            }
        }
        other => Ok(other),
    }
}

fn resp_from_log(
    resp: &LoggedResp,
    store: Option<&ArtifactStore>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    match resp {
        LoggedResp::Ok(t) => Ok(Value::Data(t.clone())),
        LoggedResp::Error(payload) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::Data(payload.clone())),
        }),
        LoggedResp::OkArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch("missing artifact store for :ok-artifact".to_string())
            })?;
            let bytes = store.get_bytes(artifact)?;
            let s = String::from_utf8(bytes).map_err(|_| {
                EffectsError::ReplayMismatch("artifact bytes are not utf-8 term".to_string())
            })?;
            let t = gc_coreform::parse_term(&s)
                .map_err(|e| EffectsError::ReplayMismatch(format!("bad artifact term: {e}")))?;
            Ok(Value::Data(t))
        }
        LoggedResp::ErrorArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch(
                    "missing artifact store for :error-artifact".to_string(),
                )
            })?;
            let bytes = store.get_bytes(artifact)?;
            let s = String::from_utf8(bytes).map_err(|_| {
                EffectsError::ReplayMismatch("artifact bytes are not utf-8 term".to_string())
            })?;
            let t = gc_coreform::parse_term(&s)
                .map_err(|e| EffectsError::ReplayMismatch(format!("bad artifact term: {e}")))?;
            Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::Data(t)),
            })
        }
        LoggedResp::OkBytesArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch(
                    "missing artifact store for :ok-bytes-artifact".to_string(),
                )
            })?;
            let bytes = store.get_bytes(artifact)?;
            Ok(Value::Data(Term::Bytes(bytes.into())))
        }
        LoggedResp::ErrorBytesArtifact { artifact } => {
            let store = store.ok_or_else(|| {
                EffectsError::ReplayMismatch(
                    "missing artifact store for :error-bytes-artifact".to_string(),
                )
            })?;
            let bytes = store.get_bytes(artifact)?;
            Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::Data(Term::Bytes(bytes.into()))),
            })
        }
    }
}

fn cap_term(op: &str, pol: Option<&OpPolicy>) -> Result<Term, EffectsError> {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":op".to_string())),
        Term::Symbol(op.to_string()),
    );
    if let Some(pol) = pol {
        if pol.create_dirs {
            m.insert(
                TermOrdKey(Term::Symbol(":create-dirs".to_string())),
                Term::Bool(true),
            );
        }
        if let Some(ms) = pol.timeout_ms {
            m.insert(
                TermOrdKey(Term::Symbol(":timeout-ms".to_string())),
                Term::Int((ms as i64).into()),
            );
        }
        if let Some(n) = pol.log_inline_max_bytes {
            m.insert(
                TermOrdKey(Term::Symbol(":log-inline-max-bytes".to_string())),
                Term::Int((n as i64).into()),
            );
        }
    }
    Ok(Term::Map(m))
}

fn call_capability(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let timeout_ms = pol.and_then(|p| p.timeout_ms).filter(|ms| *ms > 0);
    if timeout_ms.is_some() && op == "io/fs::write" {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            "timeout_ms is not supported for io/fs::write (mutating op)".to_string(),
            Some(op),
        ));
    }
    match op {
        "core/sync::pull" => {
            #[cfg(target_os = "wasi")]
            {
                let _ = payload;
                let _ = pol;
                let _ = policy;
                let _ = store;
                let _ = refs;
                return Ok(mk_error(
                    error_tok,
                    "core/sync/not-supported",
                    "core/sync is not supported on WASI (no networking in the bootstrap)"
                        .to_string(),
                    Some(op),
                ));
            }
            #[cfg(not(target_os = "wasi"))]
            {
                let store = store.ok_or_else(|| {
                    EffectsError::Log("missing artifact store for core/sync::pull".to_string())
                })?;
                let refs = refs.ok_or_else(|| {
                    EffectsError::Log("missing refs db for core/sync::pull".to_string())
                })?;

                let remote_s = match payload_sync_remote(payload) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
                    }
                };
                let depth = payload_sync_depth(payload).unwrap_or(0);
                let force = payload_sync_force(payload).unwrap_or(false);
                let refnames = match payload_sync_refs(payload) {
                    Ok(rs) => rs,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
                    }
                };
                let roots = match payload_sync_roots(payload) {
                    Ok(rs) => rs,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
                    }
                };
                if refnames.is_empty() && roots.is_empty() {
                    return Ok(mk_error(
                        error_tok,
                        "core/sync/bad-payload",
                        "pull requires :refs and/or :roots".to_string(),
                        Some(op),
                    ));
                }

                let sp = match sync_policy_from_op(pol) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                    }
                };
                let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
                    Ok(b) => b,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op)));
                    }
                };
                let client = match gc_registry::RegistryClient::new(
                    &base,
                    timeout_ms.map(std::time::Duration::from_millis),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/remote-error",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };

                let mut pulled: u64 = 0;
                let mut already: u64 = 0;
                let mut heads: Vec<Term> = Vec::new();

                // Pull by roots (hashes) first.
                for h in &roots {
                    let mut stats = SyncPullStats {
                        pulled: &mut pulled,
                        already: &mut already,
                        error_tok,
                        op,
                        transfer_workers: sp.transfer_workers,
                    };
                    match sync_pull_closure(&client, store, h, depth, &mut stats) {
                        Ok(()) => {}
                        Err(v) => return Ok(v),
                    }
                }

                // Pull and update refs.
                for rname in &refnames {
                    let h = match client.refs_get(rname) {
                        Ok(Some(h)) => h,
                        Ok(None) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/sync/ref-not-found",
                                format!("remote ref not found: {rname}"),
                                Some(op),
                            ));
                        }
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/sync/remote-error",
                                format!("{e}"),
                                Some(op),
                            ));
                        }
                    };
                    let mut stats = SyncPullStats {
                        pulled: &mut pulled,
                        already: &mut already,
                        error_tok,
                        op,
                        transfer_workers: sp.transfer_workers,
                    };
                    match sync_pull_closure(&client, store, &h, depth, &mut stats) {
                        Ok(()) => {}
                        Err(v) => return Ok(v),
                    }

                    // Update local refs unless it would clobber a different value.
                    let cur = refs.get(rname)?;
                    if !force
                        && let Some(curh) = &cur
                        && curh != &h
                    {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/refs/conflict",
                            "local ref differs; use force to overwrite".to_string(),
                            Some(op),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":refs/name")),
                                        Term::Str(rname.clone()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":refs/current")),
                                        cur.clone().map(Term::Str).unwrap_or(Term::Nil),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":refs/remote")),
                                        Term::Str(h.clone()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                    let _ = refs.set(rname, Some(&h), None)?;

                    heads.push(Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":name")), Term::Str(rname.clone())),
                            (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                        ]
                        .into_iter()
                        .collect(),
                    ));
                }

                heads.sort_by_cached_key(print_term);

                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
                m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
                m.insert(
                    TermOrdKey(Term::symbol(":pulled")),
                    Term::Int((pulled as i64).into()),
                );
                m.insert(
                    TermOrdKey(Term::symbol(":present")),
                    Term::Int((already as i64).into()),
                );
                m.insert(TermOrdKey(Term::symbol(":heads")), Term::Vector(heads));
                Ok(Value::Data(Term::Map(m)))
            }
        }

        "core/sync::push" => {
            #[cfg(target_os = "wasi")]
            {
                let _ = payload;
                let _ = pol;
                let _ = policy;
                let _ = store;
                let _ = refs;
                return Ok(mk_error(
                    error_tok,
                    "core/sync/not-supported",
                    "core/sync is not supported on WASI (no networking in the bootstrap)"
                        .to_string(),
                    Some(op),
                ));
            }
            #[cfg(not(target_os = "wasi"))]
            {
                let store = store.ok_or_else(|| {
                    EffectsError::Log("missing artifact store for core/sync::push".to_string())
                })?;

                let remote_s = match payload_sync_remote(payload) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
                    }
                };
                let depth = payload_sync_depth(payload).unwrap_or(0);
                let roots = match payload_sync_roots(payload) {
                    Ok(rs) => rs,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
                    }
                };
                if roots.is_empty() {
                    return Ok(mk_error(
                        error_tok,
                        "core/sync/bad-payload",
                        "push requires :roots".to_string(),
                        Some(op),
                    ));
                }
                let set_refs = match payload_sync_set_refs(payload) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/bad-payload", e, Some(op)));
                    }
                };
                for sr in &set_refs {
                    if let Err(v) = local_refs_validate_policy_gate(
                        store,
                        &sr.name,
                        Some(&sr.hash),
                        &sr.policy,
                        error_tok,
                        op,
                    ) {
                        return Ok(v);
                    }
                }

                let sp = match sync_policy_from_op(pol) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                    }
                };
                let base = match sync_normalize_and_check_remote(&sp, &remote_s) {
                    Ok(b) => b,
                    Err(e) => {
                        return Ok(mk_error(error_tok, "core/sync/remote-denied", e, Some(op)));
                    }
                };
                let client = match gc_registry::RegistryClient::new(
                    &base,
                    timeout_ms.map(std::time::Duration::from_millis),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/sync/remote-error",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };

                let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                for h in &roots {
                    match sync_closure_local(store, h, depth, &mut all, error_tok, op) {
                        Ok(()) => {}
                        Err(v) => return Ok(v),
                    }
                }
                let hashes: Vec<String> = all.into_iter().collect();

                let mut missing: Vec<String> = Vec::new();
                let mut present: u64 = 0;
                let has_chunks: Vec<Vec<String>> =
                    hashes.chunks(512).map(|chunk| chunk.to_vec()).collect();
                let has_results =
                    sync_parallel_store_has_chunks(&client, &has_chunks, sp.transfer_workers);
                for (chunk_i, chunk) in hashes.chunks(512).enumerate() {
                    let mp = match &has_results[chunk_i] {
                        Ok(m) => m,
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/sync/remote-error",
                                e.clone(),
                                Some(op),
                            ));
                        }
                    };
                    for h in chunk {
                        match mp.get(h) {
                            Some(true) => present = present.saturating_add(1),
                            _ => missing.push(h.clone()),
                        }
                    }
                }
                missing.sort();
                missing.dedup();

                let upload_results =
                    sync_parallel_upload_missing(&client, store, &missing, sp.transfer_workers);
                let mut uploaded: u64 = 0;
                for r in upload_results {
                    match r {
                        Ok(()) => uploaded = uploaded.saturating_add(1),
                        Err(e) => {
                            let (code, msg) = if e.starts_with("store-read:") {
                                ("core/store/not-found", e)
                            } else {
                                ("core/sync/remote-error", e)
                            };
                            return Ok(mk_error(error_tok, code, msg, Some(op)));
                        }
                    }
                }

                let mut refs_updated: u64 = 0;
                if !set_refs.is_empty() {
                    let mut set_refs_sorted = set_refs;
                    set_refs_sorted.sort_by(|a, b| a.name.cmp(&b.name));
                    for sr in &set_refs_sorted {
                        let req = gc_registry::RefsSetReq {
                            name: &sr.name,
                            hash: &sr.hash,
                            policy: &sr.policy,
                            expected_old: sr.expected_old.as_deref(),
                        };
                        match client.refs_set(&req) {
                            Ok(r) => {
                                if !r.ok {
                                    return Ok(mk_error(
                                        error_tok,
                                        "core/sync/refs-set-failed",
                                        "remote refs/set returned ok=false".to_string(),
                                        Some(op),
                                    ));
                                }
                                refs_updated = refs_updated.saturating_add(1);
                            }
                            Err(e) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/sync/remote-error",
                                    format!("{e}"),
                                    Some(op),
                                ));
                            }
                        }
                    }
                }

                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
                m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(base));
                m.insert(
                    TermOrdKey(Term::symbol(":total")),
                    Term::Int((hashes.len() as i64).into()),
                );
                m.insert(
                    TermOrdKey(Term::symbol(":present")),
                    Term::Int((present as i64).into()),
                );
                m.insert(
                    TermOrdKey(Term::symbol(":uploaded")),
                    Term::Int((uploaded as i64).into()),
                );
                m.insert(
                    TermOrdKey(Term::symbol(":refs-updated")),
                    Term::Int((refs_updated as i64).into()),
                );
                Ok(Value::Data(Term::Map(m)))
            }
        }

        "core/pkg::init" => {
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
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::add" => {
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

            l.set_requirement(&name, &selector, update_policy, registry);
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
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::list" => {
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
                    requirements.insert(
                        name,
                        gc_pkg::Requirement {
                            selector,
                            update_policy,
                            registry,
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
                    locked.insert(
                        name,
                        gc_pkg::LockedEntry {
                            commit,
                            snapshot,
                            registry,
                            source_selector,
                            resolved_ref,
                            exports_hash,
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

        "core/pkg::info" => {
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
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::lock" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::lock".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg::lock".to_string())
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

            if strict {
                for (name, le) in &out_locked {
                    let req = match l.requirements.get(name) {
                        Some(r) => r,
                        None => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/lock-invariant",
                                format!("missing requirement entry for locked dependency: {name}"),
                                Some(op),
                            ));
                        }
                    };
                    if le.source_selector != req.selector {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/lock-invariant",
                            format!("locked.source_selector mismatch for {name}"),
                            Some(op),
                        ));
                    }
                    match parse_selector(&req.selector) {
                        Some(Selector::Snapshot(_)) => {
                            if le.resolved_ref.is_some() {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/lock-invariant",
                                    format!(
                                        "snapshot selector must not set resolved_ref for {name}"
                                    ),
                                    Some(op),
                                ));
                            }
                        }
                        Some(Selector::Commit(sel_h)) => {
                            if le.resolved_ref.is_some() {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/lock-invariant",
                                    format!("commit selector must not set resolved_ref for {name}"),
                                    Some(op),
                                ));
                            }
                            let Some(locked_commit) = &le.commit else {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/lock-invariant",
                                    format!("commit selector resolved without commit for {name}"),
                                    Some(op),
                                ));
                            };
                            if !locked_commit.eq_ignore_ascii_case(&sel_h) {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/lock-invariant",
                                    format!("commit selector hash mismatch for {name}"),
                                    Some(op),
                                ));
                            }
                        }
                        Some(Selector::Ref(ref_name)) => {
                            if le.resolved_ref.as_deref() != Some(ref_name.as_str()) {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/lock-invariant",
                                    format!("ref selector resolved_ref mismatch for {name}"),
                                    Some(op),
                                ));
                            }
                            if le.commit.is_none() {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/pkg/lock-invariant",
                                    format!("ref selector resolved without commit for {name}"),
                                    Some(op),
                                ));
                            }
                        }
                        None => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-selector",
                                format!("unsupported selector: {}", req.selector),
                                Some(op),
                            ));
                        }
                    }

                    if !store.path_for(&le.snapshot).exists() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("artifact not found: {}", le.snapshot),
                            Some(op),
                        ));
                    }
                    if store.verify_hex(&le.snapshot).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {}", le.snapshot),
                            Some(op),
                        ));
                    }
                    let snap_term = match store_get_term(store, &le.snapshot) {
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
                    if let Err(e) = gc_vcs::Snapshot::from_term(&snap_term) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }

                    if let Some(commit_hex) = &le.commit
                        && let Err(v) = validate_commit_artifact_closure(
                            store,
                            name,
                            &le.snapshot,
                            commit_hex,
                            true,
                            error_tok,
                            op,
                        )
                    {
                        return Ok(v);
                    }
                }
            }
            l.locked = out_locked;

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
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            m.insert(
                TermOrdKey(Term::symbol(":locked-count")),
                Term::Int((l.locked.len() as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":strict")), Term::Bool(strict));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::update" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::update".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg::update".to_string())
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
                let sel = parse_selector(&req.selector);
                let should_update = req.update_policy == gc_pkg::UpdatePolicy::Auto
                    && matches!(sel, Some(Selector::Ref(_)));
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
            m.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            m.insert(
                TermOrdKey(Term::symbol(":updated")),
                Term::Int((updated as i64).into()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::install" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::install".to_string())
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
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg::verify" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::verify".to_string())
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

        "core/pkg::snapshot" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg::snapshot".to_string())
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
                let store_hex = match store.put_bytes(module_bytes.as_bytes()) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
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
            let snap_hex = match store.put_bytes(print_term(&snapshot).as_bytes()) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
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

        "core/pkg::publish" => {
            #[cfg(target_os = "wasi")]
            {
                let _ = payload;
                let _ = pol;
                let _ = policy;
                let _ = store;
                let _ = refs;
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/not-supported",
                    "core/pkg::publish is not supported on WASI (no networking in the bootstrap)"
                        .to_string(),
                    Some(op),
                ));
            }
            #[cfg(not(target_os = "wasi"))]
            {
                let store = store.ok_or_else(|| {
                    EffectsError::Log("missing artifact store for core/pkg::publish".to_string())
                })?;
                let refs = refs.ok_or_else(|| {
                    EffectsError::Log("missing refs db for core/pkg::publish".to_string())
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
                    if let Err(e) = gc_vcs::Evidence::from_term(&ev_term) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-evidence",
                            e.to_string(),
                            Some(op),
                        ));
                    }
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
                    error_tok,
                )?;

                let out = match sync_out {
                    Value::Data(Term::Map(mut m)) => {
                        m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(commit_hex));
                        m.insert(TermOrdKey(Term::symbol(":ref")), Term::Str(refname));
                        Value::Data(Term::Map(m))
                    }
                    other => other,
                };
                Ok(out)
            }
        }
        "core/store::put" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/store::put".to_string())
            })?;
            let art = payload_store_artifact(payload)?;
            let bytes = print_term(&art);
            let h = match store.put_bytes(bytes.as_bytes()) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
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
                #[cfg(target_os = "wasi")]
                {
                    if policy.store.remote.is_some() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-supported",
                            "remote store is not supported on WASI bootstrap".to_string(),
                            Some(op),
                        ));
                    }
                }
                #[cfg(not(target_os = "wasi"))]
                match store_remote_client(policy, timeout_ms, error_tok, op) {
                    Ok(Some((client, _base))) => {
                        let mp = match client.store_has(std::slice::from_ref(&h)) {
                            Ok(m) => m,
                            Err(e) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/store/remote-error",
                                    e.to_string(),
                                    Some(op),
                                ));
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
            let h = payload_store_hash(payload)?;
            let p = store.path_for(&h);
            if !p.exists() {
                #[cfg(target_os = "wasi")]
                {
                    if policy.store.remote.is_some() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-supported",
                            "remote store is not supported on WASI bootstrap".to_string(),
                            Some(op),
                        ));
                    }
                }
                #[cfg(not(target_os = "wasi"))]
                match store_remote_client(policy, timeout_ms, error_tok, op) {
                    Ok(Some((client, _base))) => {
                        let bytes = match client.store_get_opt(&h) {
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
                                return Ok(mk_error(
                                    error_tok,
                                    "core/store/remote-error",
                                    e.to_string(),
                                    Some(op),
                                ));
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
                        match store.put_bytes(&bytes) {
                            Ok(stored_h) if stored_h == h => {}
                            Ok(_) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/store/hash-mismatch",
                                    "local store wrote under different hash".to_string(),
                                    Some(op),
                                ));
                            }
                            Err(e) => {
                                return Ok(mk_error(
                                    error_tok,
                                    "core/store/io-error",
                                    e.to_string(),
                                    Some(op),
                                ));
                            }
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
            let bytes = match std::fs::read(store.path_for(&h)) {
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
        "core/vcs::log" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs::log".to_string())
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
        "core/vcs::blame" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs::blame".to_string())
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
        "core/vcs::why" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs::why".to_string())
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
            let (conflict, base_t, left_t, right_t) =
                if let (Some(conflict_t), Some(base_t), Some(left_t), Some(right_t)) = (
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
                    (conflict, base_t.clone(), left_t.clone(), right_t.clone())
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
                    (conflict, base_t, left_t, right_t)
                } else {
                    return Ok(mk_error(
                    error_tok,
                    "core/vcs/bad-payload",
                    "missing :conflict-hash or (:conflict-term + :base-term/:left-term/:right-term)"
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
        "core/vcs::diff" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs::diff".to_string())
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
                match store.put_bytes(patch_bytes.as_bytes()) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
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
        "core/vcs::apply" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs::apply".to_string())
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
                    let code = if e.contains("patch op :rename is not supported yet") {
                        "core/vcs/unsupported"
                    } else {
                        "core/vcs/apply-error"
                    };
                    return Ok(mk_error(error_tok, code, e, Some(op)));
                }
            };

            let snap_bytes = print_term(&cur);
            let snap_h = if store_result {
                match store.put_bytes(snap_bytes.as_bytes()) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
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
        "core/vcs::merge3" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/vcs::merge3".to_string())
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
                let conflict_h = store.put_bytes(conflict_bytes.as_bytes())?;

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
                let conflict_h = store.put_bytes(conflict_bytes.as_bytes())?;

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
            let merged_h = store.put_bytes(merged_bytes.as_bytes())?;

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
        "core/vcs::resolve-conflict" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log(
                    "missing artifact store for core/vcs::resolve-conflict".to_string(),
                )
            })?;

            let conflict_h = match payload_vcs_hash(payload, ":conflict") {
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

            // Parse strategy (optional) and per-op resolutions.
            #[derive(Debug, Clone)]
            enum Resolution {
                Side(String),
                Hash(String),
                Delete,
            }
            let (strategy, resolutions) = {
                let Term::Map(m) = payload else {
                    return Ok(mk_error(
                        error_tok,
                        "core/vcs/bad-payload",
                        "payload must be a map".to_string(),
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

                (strategy, resolutions)
            };

            let conflict_t = match store_get_term(store, &conflict_h) {
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

            // Merge + apply resolutions for divergent ops.
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
                        let mut m = BTreeMap::new();
                        m.insert(TermOrdKey(Term::symbol(":op")), Term::Str(k.clone()));
                        m.insert(
                            TermOrdKey(Term::symbol(":base")),
                            b.clone().map(Term::Str).unwrap_or(Term::Nil),
                        );
                        m.insert(
                            TermOrdKey(Term::symbol(":left")),
                            l.clone().map(Term::Str).unwrap_or(Term::Nil),
                        );
                        m.insert(
                            TermOrdKey(Term::symbol(":right")),
                            r.clone().map(Term::Str).unwrap_or(Term::Nil),
                        );
                        unresolved.push(Term::Map(m));
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
                let conflict_hex = match store.put_bytes(print_term(&conflict_term).as_bytes()) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
                m.insert(
                    TermOrdKey(Term::symbol(":conflict")),
                    Term::Str(conflict_hex),
                );
                return Ok(Value::Data(Term::Map(m)));
            }

            let merged_snapshot = gc_vcs::ContractSnapshot {
                proto: base.proto,
                overrides: merged,
            }
            .to_term();
            let merged_bytes = print_term(&merged_snapshot);
            let merged_h = match store.put_bytes(merged_bytes.as_bytes()) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

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
            let patch_bytes = print_term(&patch_term);
            let patch_h = match store.put_bytes(patch_bytes.as_bytes()) {
                Ok(h) => h,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(op),
                    ));
                }
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
            m.insert(TermOrdKey(Term::symbol(":snapshot")), Term::Str(merged_h));
            m.insert(TermOrdKey(Term::symbol(":patch")), Term::Str(patch_h));
            m.insert(
                TermOrdKey(Term::symbol(":values")),
                Term::Vector(values.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "core/gc::plan" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gc::plan".to_string())
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
        "core/gc::run" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gc::run".to_string())
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
        "core/gc::pin" => {
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
        "core/gc::unpin" => {
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
        "core/gc::purge" => {
            let base_dir = effective_base_dir(pol)?;
            let ttl_days = payload_gc_ttl_days(payload)
                .ok_or_else(|| EffectsError::BadPayload("missing :ttl-days int".to_string()))?;
            let quarantine_dir_s = payload_gc_quarantine_dir(payload);

            let qd = match quarantine_dir_s {
                Some(s) => sandbox_path_allow_missing(&base_dir, &s, false)?,
                None => {
                    let store = store.ok_or_else(|| {
                        EffectsError::Log("missing artifact store for core/gc::purge".to_string())
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
        "core/gpk::export" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gpk::export".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/gpk::export".to_string())
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
        "core/gpk::import" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gpk::import".to_string())
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
                    EffectsError::Log("missing refs db for core/gpk::import".to_string())
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
            let bundle = match gc_vcs::read_bundle(&mut f) {
                Ok(b) => b,
                Err(e) => {
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
                let got = match store.put_bytes(&e.bytes) {
                    Ok(h) => h,
                    Err(err) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/io-error",
                            err.to_string(),
                            Some(op),
                        ));
                    }
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
            if let Some(ms) = timeout_ms {
                let base_dir2 = base_dir.clone();
                let path_s2 = path_s.clone();
                let r = with_timeout(ms, move || {
                    let path = sandbox_path_read(&base_dir2, &path_s2)?;
                    let bytes = std::fs::read(&path);
                    Ok((path, bytes))
                })?;
                return Ok(match r {
                    Some((_path, Ok(bytes))) => Value::Data(Term::Bytes(bytes.into())),
                    Some((path, Err(e))) => Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                    },
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                });
            }
            let path = sandbox_path_read(&base_dir, &path_s)?;
            match std::fs::read(&path) {
                Ok(bytes) => Ok(Value::Data(Term::Bytes(bytes.into()))),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
            }
        }
        "io/fs::write" => {
            let path_s = payload_path(payload)?;
            let data = payload_data(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.is_some_and(|p| p.create_dirs);
            let path = sandbox_path_write(&base_dir, &path_s, create_dirs)?;
            if path.exists() {
                let md = std::fs::symlink_metadata(&path)?;
                if md.file_type().is_symlink() {
                    let e = std::io::Error::other("refusing to write through symlink");
                    return Ok(Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                    });
                }
            }
            match std::fs::write(&path, data) {
                Ok(()) => Ok(Value::Data(Term::Nil)),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
            }
        }
        "gfx/gpu::create-buffer"
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
        | "gfx/gpu::destroy-resource"
        | "gfx/gpu::write-buffer"
        | "gfx/gpu::write-texture"
        | "gfx/gpu::read-buffer"
        | "gfx/gpu::read-texture"
        | "gfx/gpu::submit-frame-graph"
        | "gfx/gpu::submit-compute-graph"
        | "gfx/gpu::limits"
        | "gfx/gpu::features"
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

fn payload_store_hash(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/store payload must be a map".to_string(),
        ));
    };
    let Some(Term::Str(h)) = m.get(&TermOrdKey(Term::Symbol(":hash".to_string()))) else {
        return Err(EffectsError::Log(
            "core/store payload missing :hash string".to_string(),
        ));
    };
    Ok(h.clone())
}

fn payload_store_artifact(payload: &Term) -> Result<Term, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/store payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::Symbol(":artifact".to_string()))) else {
        return Err(EffectsError::Log(
            "core/store payload missing :artifact".to_string(),
        ));
    };
    Ok(t.clone())
}

fn store_get_term(store: &ArtifactStore, hex: &str) -> Result<Term, EffectsError> {
    let bytes = store.get_bytes(hex)?;
    let s = String::from_utf8(bytes)
        .map_err(|_| EffectsError::Log("artifact bytes are not utf-8 term".to_string()))?;
    gc_coreform::parse_term(&s).map_err(|e| EffectsError::Log(format!("bad artifact term: {e}")))
}

#[derive(Copy, Clone)]
struct LocalRefSetRequest<'a> {
    name: &'a str,
    new_hash: Option<&'a str>,
    expected_old: Option<Option<&'a str>>,
    policy_h: &'a str,
}

fn local_refs_set_policy_gated(
    store: &ArtifactStore,
    refs: &RefsDb,
    req: LocalRefSetRequest<'_>,
    error_tok: SealId,
    op: &str,
) -> Result<SetResult, Value> {
    local_refs_validate_policy_gate(store, req.name, req.new_hash, req.policy_h, error_tok, op)?;
    refs.set(req.name, req.new_hash, req.expected_old)
        .map_err(|e| mk_error(error_tok, "core/refs/io-error", e.to_string(), Some(op)))
}

fn local_refs_validate_policy_gate(
    store: &ArtifactStore,
    name: &str,
    new_hash: Option<&str>,
    policy_h: &str,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    let pol_term = match store_get_term(store, policy_h) {
        Ok(t) => t,
        Err(_) => {
            return Err(mk_error(
                error_tok,
                "core/refs/policy-not-found",
                format!("policy artifact not found: {policy_h}"),
                Some(op),
            ));
        }
    };
    let pol = match gc_vcs::Policy::from_term(&pol_term) {
        Ok(p) => p,
        Err(e) => {
            return Err(mk_error(
                error_tok,
                "core/refs/bad-policy",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    if pol.is_frozen_ref(name) {
        return Err(mk_error(
            error_tok,
            "core/refs/frozen",
            format!("ref is frozen by policy: {name}"),
            Some(op),
        ));
    }
    let class = match pol.class_for_ref(name) {
        Some(c) => c,
        None => {
            return Err(mk_error(
                error_tok,
                "core/refs/no-class",
                format!("policy has no matching class for ref {name}"),
                Some(op),
            ));
        }
    };

    if let Some(h) = new_hash {
        let commit_term = match store_get_term(store, h) {
            Ok(t) => t,
            Err(_) => {
                return Err(mk_error(
                    error_tok,
                    "core/refs/commit-not-found",
                    format!("commit artifact not found: {h}"),
                    Some(op),
                ));
            }
        };
        let commit = match gc_vcs::Commit::from_term(&commit_term) {
            Ok(c) => c,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/refs/bad-commit",
                    format!("{e}"),
                    Some(op),
                ));
            }
        };

        for req in &class.required_obligations {
            if !commit.obligations.iter().any(|o| o == req) {
                return Err(mk_error(
                    error_tok,
                    "core/refs/missing-obligation",
                    format!("commit missing required obligation: {req}"),
                    Some(op),
                ));
            }
        }
        if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
            return Err(mk_error(
                error_tok,
                "core/refs/missing-evidence",
                "commit has required obligations but no evidence".to_string(),
                Some(op),
            ));
        }
        for ev_h in &commit.evidence {
            if store.path_for(ev_h).exists() {
                if store.verify_hex(ev_h).is_err() {
                    return Err(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("evidence artifact corrupted: {ev_h}"),
                        Some(op),
                    ));
                }
            } else {
                return Err(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("evidence artifact not found: {ev_h}"),
                    Some(op),
                ));
            }
            let ev_t = match store_get_term(store, ev_h) {
                Ok(t) => t,
                Err(_) => {
                    return Err(mk_error(
                        error_tok,
                        "core/refs/bad-evidence",
                        format!("evidence artifact is not a valid CoreForm term: {ev_h}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = gc_vcs::Evidence::from_term(&ev_t) {
                return Err(mk_error(
                    error_tok,
                    "core/refs/bad-evidence",
                    format!("{e}"),
                    Some(op),
                ));
            }
        }

        if class.require_signatures {
            let signing_h = match gc_vcs::commit_signing_hash(&commit_term) {
                Ok(hh) => hh,
                Err(e) => {
                    return Err(mk_error(
                        error_tok,
                        "core/refs/bad-commit",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut valid: u64 = 0;
            let mut seen_pks: std::collections::BTreeSet<Vec<u8>> =
                std::collections::BTreeSet::new();
            for at_h in &commit.attestations {
                if store.path_for(at_h).exists() {
                    if store.verify_hex(at_h).is_err() {
                        return Err(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("attestation artifact corrupted: {at_h}"),
                            Some(op),
                        ));
                    }
                } else {
                    return Err(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("attestation artifact not found: {at_h}"),
                        Some(op),
                    ));
                }
                let at_t = match store_get_term(store, at_h) {
                    Ok(t) => t,
                    Err(_) => {
                        return Err(mk_error(
                            error_tok,
                            "core/refs/bad-attestation",
                            format!("attestation artifact is not a valid CoreForm term: {at_h}"),
                            Some(op),
                        ));
                    }
                };
                let at = match gc_vcs::Attestation::from_term(&at_t) {
                    Ok(a) => a,
                    Err(e) => {
                        return Err(mk_error(
                            error_tok,
                            "core/refs/bad-attestation",
                            format!("{e}"),
                            Some(op),
                        ));
                    }
                };
                let pk_vec = at.pk.to_vec();
                if seen_pks.contains(&pk_vec) {
                    continue;
                }
                if gc_vcs::verify_commit_attestation(&at, &signing_h, &class.allowed_public_keys)
                    .is_ok()
                {
                    seen_pks.insert(pk_vec);
                    valid = valid.saturating_add(1);
                }
            }
            if valid < class.min_signatures {
                return Err(mk_error(
                    error_tok,
                    "core/refs/insufficient-signatures",
                    format!(
                        "need {} valid signatures, got {valid}",
                        class.min_signatures
                    ),
                    Some(op),
                ));
            }
        }
    }
    Ok(())
}

fn payload_refs_name(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":name".to_string()))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log(
            "core/refs payload missing :name".to_string(),
        )),
    }
}

fn payload_refs_prefix(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":prefix".to_string()))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        _ => Err(EffectsError::Log(
            "core/refs payload :prefix must be string or nil".to_string(),
        )),
    }
}

fn payload_refs_hash(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":hash".to_string()))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        _ => Err(EffectsError::Log(
            "core/refs payload :hash must be string or nil".to_string(),
        )),
    }
}

fn payload_refs_expected_old(payload: &Term) -> Result<Option<Option<String>>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":expected-old".to_string()))) {
        None => Ok(None),
        Some(Term::Nil) => Ok(Some(None)),
        Some(Term::Str(s)) => Ok(Some(Some(s.clone()))),
        _ => Err(EffectsError::Log(
            "core/refs payload :expected-old must be string, nil, or absent".to_string(),
        )),
    }
}

fn payload_refs_policy_hash(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::Log(
            "core/refs payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::Symbol(":policy".to_string()))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err(EffectsError::Log(
            "core/refs payload missing :policy".to_string(),
        )),
    }
}

type TimeoutJob = Box<dyn FnOnce() + Send + 'static>;

struct TimeoutPool {
    tx: std::sync::mpsc::Sender<TimeoutJob>,
}

impl TimeoutPool {
    fn worker_count() -> usize {
        let env_n = std::env::var("GENESIS_TIMEOUT_WORKERS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(4);
        env_n.clamp(1, 32)
    }

    fn new() -> Result<Self, EffectsError> {
        use std::sync::{Arc, Mutex, mpsc};
        let (tx, rx) = mpsc::channel::<TimeoutJob>();
        let rx = Arc::new(Mutex::new(rx));

        let mut started = 0usize;
        for i in 0..Self::worker_count() {
            let rx2 = Arc::clone(&rx);
            let name = format!("gc-timeout-{i}");
            let spawned = std::thread::Builder::new().name(name).spawn(move || {
                loop {
                    let msg = {
                        let guard = rx2.lock();
                        match guard {
                            Ok(g) => g.recv(),
                            Err(_) => return,
                        }
                    };
                    match msg {
                        Ok(job) => job(),
                        Err(_) => return,
                    }
                }
            });
            if spawned.is_ok() {
                started = started.saturating_add(1);
            }
        }
        if started == 0 {
            return Err(EffectsError::Log(
                "timeout pool failed to start worker threads".to_string(),
            ));
        }
        Ok(Self { tx })
    }

    fn submit(&self, job: TimeoutJob) -> Result<(), TimeoutJob> {
        self.tx.send(job).map_err(|e| e.0)
    }
}

fn timeout_pool() -> Option<&'static TimeoutPool> {
    static POOL: std::sync::OnceLock<TimeoutPool> = std::sync::OnceLock::new();
    if let Some(p) = POOL.get() {
        return Some(p);
    }
    let built = TimeoutPool::new().ok()?;
    if POOL.set(built).is_ok() {
        return POOL.get();
    }
    POOL.get()
}

fn with_timeout<T, F>(timeout_ms: u64, f: F) -> Result<Option<T>, EffectsError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, EffectsError> + Send + 'static,
{
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    let mut f_slot = Some(f);
    let job: TimeoutJob = Box::new(move || {
        let run = f_slot.take().expect("timeout closure taken");
        let r = run();
        let _ = tx.send(r);
    });
    let mut fallback_job = Some(job);
    let submitted = match timeout_pool() {
        Some(pool) => match pool.submit(fallback_job.take().expect("timeout job")) {
            Ok(()) => true,
            Err(job) => {
                fallback_job = Some(job);
                false
            }
        },
        None => false,
    };
    if !submitted {
        let job = fallback_job.expect("timeout fallback job");
        std::thread::spawn(move || job());
    }
    match rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
        Ok(r) => r.map(Some),
        Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(EffectsError::Log(
            "capability thread disconnected".to_string(),
        )),
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::with_timeout;
    use crate::EffectsError;

    #[test]
    fn with_timeout_returns_some_for_fast_jobs() {
        let out = with_timeout(100, || -> Result<u64, EffectsError> { Ok(7) }).unwrap();
        assert_eq!(out, Some(7));
    }

    #[test]
    fn with_timeout_returns_none_for_slow_jobs() {
        let out = with_timeout(1, || -> Result<u64, EffectsError> {
            std::thread::sleep(std::time::Duration::from_millis(25));
            Ok(7)
        })
        .unwrap();
        assert_eq!(out, None);
    }
}

fn io_error_payload(op: &str, base_dir: &Path, path: &Path, e: &std::io::Error) -> Term {
    // Avoid leaking absolute paths and normalize separators for stability.
    let rel = path.strip_prefix(base_dir).unwrap_or(path);
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":error/code".to_string())),
        Term::Str("io/error".to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/message".to_string())),
        Term::Str(e.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/op".to_string())),
        Term::Symbol(op.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(
            [
                (
                    TermOrdKey(Term::Symbol(":subsystem".to_string())),
                    Term::Str("effects".to_string()),
                ),
                (
                    TermOrdKey(Term::Symbol(":op".to_string())),
                    Term::Symbol(op.to_string()),
                ),
                (
                    TermOrdKey(Term::Symbol(":path".to_string())),
                    Term::Str(path_to_slash(rel)),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    );
    Term::Map(m)
}

fn path_to_slash(p: &Path) -> String {
    let mut out = String::new();
    for (i, c) in p.components().enumerate() {
        if i != 0 {
            out.push('/');
        }
        out.push_str(&c.as_os_str().to_string_lossy());
    }
    out
}

fn payload_path(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":path".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :path".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":path must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_pkg_path(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":pkg".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :pkg".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":pkg must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_root(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":root".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :root".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":root must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_out(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":out".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :out".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":out must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_in(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":in".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :in".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":in must be string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_gpk_mode(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(":mode"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Symbol(s)) => Ok(Some(s.clone())),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(EffectsError::BadPayload(format!(
            ":mode must be symbol/string or nil, got {}",
            print_term(other)
        ))),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GpkMode {
    Shallow,
    Full,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GpkIncludeEvidence {
    Required,
    All,
    None,
}

impl GpkIncludeEvidence {
    fn from_token(s: &str) -> Option<Self> {
        match s {
            "required" | ":required" => Some(Self::Required),
            "all" | ":all" => Some(Self::All),
            "none" | ":none" => Some(Self::None),
            _ => None,
        }
    }

    fn to_symbol(self) -> &'static str {
        match self {
            Self::Required => ":required",
            Self::All => ":all",
            Self::None => ":none",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GpkIncludeDeps {
    None,
    Locked,
    All,
}

impl GpkIncludeDeps {
    fn from_token(s: &str) -> Option<Self> {
        match s {
            "none" | ":none" => Some(Self::None),
            "locked" | ":locked" => Some(Self::Locked),
            "all" | ":all" => Some(Self::All),
            _ => None,
        }
    }

    fn to_symbol(self) -> &'static str {
        match self {
            Self::None => ":none",
            Self::Locked => ":locked",
            Self::All => ":all",
        }
    }
}

fn payload_gpk_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

fn payload_gpk_refs(payload: &Term) -> Result<Vec<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(EffectsError::BadPayload(format!(
            ":refs must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            Term::Symbol(s) => out.push(s.clone()),
            other => {
                return Err(EffectsError::BadPayload(format!(
                    ":refs entries must be strings/symbols, got {}",
                    print_term(other)
                )));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct GpkSetRef {
    name: String,
    hash: Option<String>,
    policy: String,
    expected_old: Option<Option<String>>,
}

fn payload_gpk_set_refs(payload: &Term) -> Result<Vec<GpkSetRef>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":set-refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(EffectsError::BadPayload(format!(
            ":set-refs must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::with_capacity(xs.len());
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for x in xs {
        let Term::Map(mm) = x else {
            return Err(EffectsError::BadPayload(format!(
                ":set-refs entries must be maps, got {}",
                print_term(x)
            )));
        };
        let name = match mm.get(&TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) if !s.trim().is_empty() => s.clone(),
            _ => {
                return Err(EffectsError::BadPayload(
                    "set-ref missing :name string".to_string(),
                ));
            }
        };
        if !seen.insert(name.clone()) {
            return Err(EffectsError::BadPayload(format!(
                "duplicate set-ref target: {name}"
            )));
        }
        let hash = match mm.get(&TermOrdKey(Term::symbol(":hash"))) {
            None | Some(Term::Nil) => None,
            Some(Term::Str(s)) if s == "nil" => None,
            Some(Term::Str(s)) => {
                if gc_vcs::validate_hex_hash(s).is_err() {
                    return Err(EffectsError::BadPayload(
                        "set-ref :hash must be 64-hex or nil".to_string(),
                    ));
                }
                Some(s.to_ascii_lowercase())
            }
            Some(other) => {
                return Err(EffectsError::BadPayload(format!(
                    "set-ref :hash must be string or nil, got {}",
                    print_term(other)
                )));
            }
        };
        let policy = match mm.get(&TermOrdKey(Term::symbol(":policy"))) {
            Some(Term::Str(s)) if !s.trim().is_empty() => {
                if gc_vcs::validate_hex_hash(s).is_err() {
                    return Err(EffectsError::BadPayload(
                        "set-ref :policy must be 64-hex".to_string(),
                    ));
                }
                s.to_ascii_lowercase()
            }
            _ => {
                return Err(EffectsError::BadPayload(
                    "set-ref missing :policy string".to_string(),
                ));
            }
        };
        let expected_old = match mm.get(&TermOrdKey(Term::symbol(":expected-old"))) {
            None => None,
            Some(Term::Nil) => Some(None),
            Some(Term::Str(s)) if s == "nil" => Some(None),
            Some(Term::Str(s)) => {
                if gc_vcs::validate_hex_hash(s).is_err() {
                    return Err(EffectsError::BadPayload(
                        "set-ref :expected-old must be 64-hex, nil, or absent".to_string(),
                    ));
                }
                Some(Some(s.to_ascii_lowercase()))
            }
            Some(other) => {
                return Err(EffectsError::BadPayload(format!(
                    "set-ref :expected-old must be string, nil, or absent, got {}",
                    print_term(other)
                )));
            }
        };
        out.push(GpkSetRef {
            name,
            hash,
            policy,
            expected_old,
        });
    }
    Ok(out)
}

fn payload_gpk_include_evidence(payload: &Term) -> Result<GpkIncludeEvidence, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m.get(&TermOrdKey(Term::symbol(":include-evidence")));
    let Some(v) = v else {
        return Ok(GpkIncludeEvidence::Required);
    };
    let token = match v {
        Term::Str(s) | Term::Symbol(s) => s.as_str(),
        other => {
            return Err(EffectsError::BadPayload(format!(
                ":include-evidence must be symbol/string, got {}",
                print_term(other)
            )));
        }
    };
    GpkIncludeEvidence::from_token(token).ok_or_else(|| {
        EffectsError::BadPayload(format!(
            ":include-evidence must be one of required|all|none, got {token}"
        ))
    })
}

fn payload_gpk_include_deps(payload: &Term) -> Result<GpkIncludeDeps, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m.get(&TermOrdKey(Term::symbol(":include-deps")));
    let Some(v) = v else {
        return Ok(GpkIncludeDeps::Locked);
    };
    let token = match v {
        Term::Str(s) | Term::Symbol(s) => s.as_str(),
        other => {
            return Err(EffectsError::BadPayload(format!(
                ":include-deps must be symbol/string, got {}",
                print_term(other)
            )));
        }
    };
    GpkIncludeDeps::from_token(token).ok_or_else(|| {
        EffectsError::BadPayload(format!(
            ":include-deps must be one of none|locked|all, got {token}"
        ))
    })
}

fn payload_data(payload: &Term) -> Result<Vec<u8>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::Symbol(":data".to_string())))
        .ok_or_else(|| EffectsError::BadPayload("missing :data".to_string()))?;
    match v {
        Term::Bytes(b) => Ok(b.to_vec()),
        Term::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(EffectsError::BadPayload(format!(
            ":data must be bytes or string, got {}",
            print_term(v)
        ))),
    }
}

fn payload_vcs_root(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":root")))
        .ok_or_else(|| EffectsError::BadPayload("missing :root".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        Term::Symbol(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":root must be string/symbol, got {}",
            print_term(v)
        ))),
    }
}

fn payload_vcs_max(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":max"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

fn payload_vcs_out(payload: &Term) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(":out"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(EffectsError::BadPayload(format!(
            ":out must be string or nil, got {}",
            print_term(other)
        ))),
    }
}

fn payload_vcs_store(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":store"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn payload_vcs_patch(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":patch")))
        .ok_or_else(|| EffectsError::BadPayload("missing :patch".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        Term::Symbol(s) => Ok(s.clone()),
        other => Err(EffectsError::BadPayload(format!(
            ":patch must be string/symbol, got {}",
            print_term(other)
        ))),
    }
}

fn payload_vcs_hash(payload: &Term, key: &str) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| EffectsError::BadPayload(format!("missing {key}")))?;
    match v {
        Term::Str(s) => {
            gc_vcs::validate_hex_hash(s)
                .map_err(|e| EffectsError::BadPayload(format!("{key}: {e}")))?;
            Ok(s.clone())
        }
        other => Err(EffectsError::BadPayload(format!(
            "{key} must be hex string, got {}",
            print_term(other)
        ))),
    }
}

fn payload_vcs_opt_hash(payload: &Term, key: &str) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            gc_vcs::validate_hex_hash(s)
                .map_err(|e| EffectsError::BadPayload(format!("{key}: {e}")))?;
            Ok(Some(s.clone()))
        }
        Some(other) => Err(EffectsError::BadPayload(format!(
            "{key} must be hex string or nil, got {}",
            print_term(other)
        ))),
    }
}

fn payload_vcs_sym(payload: &Term, key: &str) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| EffectsError::BadPayload(format!("missing {key}")))?;
    match v {
        Term::Symbol(s) => Ok(s.clone()),
        Term::Str(s) => Ok(s.clone()),
        other => Err(EffectsError::BadPayload(format!(
            "{key} must be symbol/string, got {}",
            print_term(other)
        ))),
    }
}

fn payload_vcs_opt_sym_or_str(payload: &Term, key: &str) -> Result<Option<String>, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Symbol(s)) => Ok(Some(s.clone())),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(EffectsError::BadPayload(format!(
            "{key} must be symbol/string or nil, got {}",
            print_term(other)
        ))),
    }
}

fn vcs_load_commit(
    store: &ArtifactStore,
    commit_h: &str,
) -> Result<(gc_vcs::Commit, Term), String> {
    let t = store_get_term(store, commit_h).map_err(|e| e.to_string())?;
    let c = gc_vcs::Commit::from_term(&t).map_err(|e| format!("bad commit {commit_h}: {e}"))?;
    Ok((c, t))
}

fn vcs_snapshot_symbol_ref(
    store: &ArtifactStore,
    snapshot_h: &str,
    sym: &str,
) -> Result<Option<String>, String> {
    let t = store_get_term(store, snapshot_h).map_err(|e| e.to_string())?;
    let snap =
        gc_vcs::Snapshot::from_term(&t).map_err(|e| format!("bad snapshot {snapshot_h}: {e}"))?;
    match snap.kind {
        gc_vcs::SnapshotKind::Module(m) => Ok(m.defs.get(sym).cloned()),
        gc_vcs::SnapshotKind::Contract(c) => Ok(c.overrides.get(sym).cloned()),
        gc_vcs::SnapshotKind::Workspace(w) => Ok(w.modules.get(sym).cloned()),
        gc_vcs::SnapshotKind::Package(p) => {
            for me in p.modules {
                if me.path == sym {
                    return Ok(Some(me.hash_hex));
                }
            }
            Ok(None)
        }
    }
}

fn vcs_find_commit_for_snapshot(
    store: &ArtifactStore,
    refs: &RefsDb,
    snapshot_h: &str,
) -> Result<Option<String>, String> {
    use std::collections::HashSet;

    let refs = refs.list(None).map_err(|e| e.to_string())?;
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = refs
        .into_iter()
        .filter_map(|r| r.hash)
        .filter(|h| gc_vcs::validate_hex_hash(h).is_ok())
        .collect();

    stack.sort();
    stack.dedup();

    while let Some(h) = stack.pop() {
        if !visited.insert(h.clone()) {
            continue;
        }
        let (c, _) = vcs_load_commit(store, &h)?;
        if c.result == snapshot_h {
            return Ok(Some(h));
        }
        for parent in c.parents.iter().rev() {
            stack.push(parent.clone());
        }
    }
    Ok(None)
}

fn vcs_blame_commit_for_symbol(
    store: &ArtifactStore,
    start_commit_h: &str,
    sym: &str,
    value_h: &str,
) -> Result<String, String> {
    use std::collections::HashSet;

    let mut cur = start_commit_h.to_string();
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        if !seen.insert(cur.clone()) {
            return Ok(cur);
        }
        let (c, _) = vcs_load_commit(store, &cur)?;
        let mut next_parent: Option<String> = None;
        for p in &c.parents {
            let (pc, _) = vcs_load_commit(store, p)?;
            let pref = vcs_snapshot_symbol_ref(store, &pc.result, sym)?;
            if pref.as_deref() == Some(value_h) {
                next_parent = Some(p.clone());
                break;
            }
        }
        match next_parent {
            Some(n) => cur = n,
            None => return Ok(cur),
        }
    }
}

// -----------------------------------------------------------------------------
// VCS merge3 (contract snapshots)
// -----------------------------------------------------------------------------

fn hash_bytes_hex(bytes: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(bytes);
    h.finalize().to_hex().to_string()
}

fn as_contract_snapshot(t: &Term) -> Result<gc_vcs::ContractSnapshot, String> {
    let snap = gc_vcs::Snapshot::from_term(t).map_err(|e| e.to_string())?;
    match snap.kind {
        gc_vcs::SnapshotKind::Contract(c) => Ok(c),
        _ => Err("expected :vcs/snapshot with :kind :contract".to_string()),
    }
}

fn vcs_diff_patch_term(
    store: &ArtifactStore,
    base_t: &Term,
    to_t: &Term,
) -> Result<(Term, Vec<String>), EffectsError> {
    fn store_value(store: &ArtifactStore, v: &Term) -> Result<String, EffectsError> {
        store.put_bytes(print_term(v).as_bytes())
    }

    fn mk_op(op_sym: &str, path: &[gc_vcs::PathStep], value: Option<&str>) -> Term {
        let mut m = BTreeMap::new();
        m.insert(TermOrdKey(Term::symbol(":op")), Term::symbol(op_sym));
        m.insert(
            TermOrdKey(Term::symbol(":path")),
            gc_vcs::path_to_term(path),
        );
        if let Some(vh) = value {
            m.insert(
                TermOrdKey(Term::symbol(":value")),
                Term::Str(vh.to_string()),
            );
        }
        Term::Map(m)
    }

    fn diff_rec(
        store: &ArtifactStore,
        path: &mut Vec<gc_vcs::PathStep>,
        a: &Term,
        b: &Term,
        ops: &mut Vec<Term>,
        values: &mut Vec<String>,
    ) -> Result<(), EffectsError> {
        if a == b {
            return Ok(());
        }
        match (a, b) {
            (Term::Map(ma), Term::Map(mb)) => {
                let mut keys: std::collections::BTreeSet<TermOrdKey> =
                    std::collections::BTreeSet::new();
                keys.extend(ma.keys().cloned());
                keys.extend(mb.keys().cloned());
                for k in keys {
                    let av = ma.get(&k);
                    let bv = mb.get(&k);
                    match (av, bv) {
                        (Some(x), Some(y)) => {
                            path.push(gc_vcs::PathStep::Map(k.0.clone()));
                            diff_rec(store, path, x, y, ops, values)?;
                            path.pop();
                        }
                        (None, Some(y)) => {
                            let vh = store_value(store, y)?;
                            values.push(vh.clone());
                            let mut p2 = path.clone();
                            p2.push(gc_vcs::PathStep::Map(k.0.clone()));
                            ops.push(mk_op(":insert", &p2, Some(&vh)));
                        }
                        (Some(_), None) => {
                            let mut p2 = path.clone();
                            p2.push(gc_vcs::PathStep::Map(k.0.clone()));
                            ops.push(mk_op(":delete", &p2, None));
                        }
                        (None, None) => {}
                    }
                }
                Ok(())
            }
            (Term::Vector(_), Term::Vector(_))
            | (Term::Pair(_, _), Term::Pair(_, _))
            | (Term::Vector(_), _)
            | (_, Term::Vector(_))
            | (Term::Pair(_, _), _)
            | (_, Term::Pair(_, _)) => {
                // Conservative: replace whole node when shape differs or container contents differ.
                let vh = store_value(store, b)?;
                values.push(vh.clone());
                ops.push(mk_op(":replace", path, Some(&vh)));
                Ok(())
            }
            _ => {
                let vh = store_value(store, b)?;
                values.push(vh.clone());
                ops.push(mk_op(":replace", path, Some(&vh)));
                Ok(())
            }
        }
    }

    let mut ops: Vec<Term> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    let mut path: Vec<gc_vcs::PathStep> = Vec::new();
    diff_rec(store, &mut path, base_t, to_t, &mut ops, &mut values)?;
    ops.sort_by_cached_key(print_term);

    let patch_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/patch"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ops")), Term::Vector(ops)),
        ]
        .into_iter()
        .collect(),
    );
    Ok((patch_term, values))
}

fn vcs_apply_patch_term(
    store: &ArtifactStore,
    base_t: &Term,
    patch: &gc_vcs::Patch,
) -> Result<Term, String> {
    fn update_at(
        t: &Term,
        path: &[gc_vcs::PathStep],
        f: &dyn Fn(&Term) -> Result<Term, String>,
    ) -> Result<Term, String> {
        if path.is_empty() {
            return f(t);
        }
        match &path[0] {
            gc_vcs::PathStep::Map(k) => {
                let Term::Map(m) = t else {
                    return Err("expected map".to_string());
                };
                let kk = TermOrdKey(k.clone());
                let child = m
                    .get(&kk)
                    .ok_or_else(|| format!("missing map key {}", print_term(k)))?;
                let new_child = update_at(child, &path[1..], f)?;
                let mut mm = m.clone();
                mm.insert(kk, new_child);
                Ok(Term::Map(mm))
            }
            gc_vcs::PathStep::Vec(i) | gc_vcs::PathStep::Form(i) => {
                let Term::Vector(xs) = t else {
                    return Err("expected vector".to_string());
                };
                if *i >= xs.len() {
                    return Err(format!("vector index out of range: {i}"));
                }
                let mut ys = xs.clone();
                let new_child = update_at(&ys[*i], &path[1..], f)?;
                ys[*i] = new_child;
                Ok(Term::Vector(ys))
            }
            gc_vcs::PathStep::PairCar => {
                let Term::Pair(a, d) = t else {
                    return Err("expected pair".to_string());
                };
                let new_a = update_at(a, &path[1..], f)?;
                Ok(Term::Pair(Box::new(new_a), d.clone()))
            }
            gc_vcs::PathStep::PairCdr => {
                let Term::Pair(a, d) = t else {
                    return Err("expected pair".to_string());
                };
                let new_d = update_at(d, &path[1..], f)?;
                Ok(Term::Pair(a.clone(), Box::new(new_d)))
            }
        }
    }

    fn replace_at(t: &Term, path: &[gc_vcs::PathStep], new_term: Term) -> Result<Term, String> {
        update_at(t, path, &|_cur| Ok(new_term.clone()))
    }

    fn insert_at(t: &Term, path: &[gc_vcs::PathStep], new_term: Term) -> Result<Term, String> {
        let (last, parent) = path.split_last().ok_or_else(|| "empty path".to_string())?;
        update_at(t, parent, &|cur| match last {
            gc_vcs::PathStep::Map(k) => {
                let Term::Map(m) = cur else {
                    return Err("expected map".to_string());
                };
                let kk = TermOrdKey(k.clone());
                if m.contains_key(&kk) {
                    return Err(format!("map key already present {}", print_term(k)));
                }
                let mut mm = m.clone();
                mm.insert(kk, new_term.clone());
                Ok(Term::Map(mm))
            }
            gc_vcs::PathStep::Vec(i) | gc_vcs::PathStep::Form(i) => {
                let Term::Vector(xs) = cur else {
                    return Err("expected vector".to_string());
                };
                if *i > xs.len() {
                    return Err(format!("vector insert index out of range: {i}"));
                }
                let mut ys = xs.clone();
                ys.insert(*i, new_term.clone());
                Ok(Term::Vector(ys))
            }
            _ => Err("insert requires :map or :vec/:form final step".to_string()),
        })
    }

    fn delete_at(t: &Term, path: &[gc_vcs::PathStep]) -> Result<Term, String> {
        let (last, parent) = path.split_last().ok_or_else(|| "empty path".to_string())?;
        update_at(t, parent, &|cur| match last {
            gc_vcs::PathStep::Map(k) => {
                let Term::Map(m) = cur else {
                    return Err("expected map".to_string());
                };
                let kk = TermOrdKey(k.clone());
                if !m.contains_key(&kk) {
                    return Err(format!("missing map key {}", print_term(k)));
                }
                let mut mm = m.clone();
                mm.remove(&kk);
                Ok(Term::Map(mm))
            }
            gc_vcs::PathStep::Vec(i) | gc_vcs::PathStep::Form(i) => {
                let Term::Vector(xs) = cur else {
                    return Err("expected vector".to_string());
                };
                if *i >= xs.len() {
                    return Err(format!("vector index out of range: {i}"));
                }
                let mut ys = xs.clone();
                ys.remove(*i);
                Ok(Term::Vector(ys))
            }
            _ => Err("delete requires :map or :vec/:form final step".to_string()),
        })
    }

    let mut cur = base_t.clone();
    for opx in &patch.ops {
        match opx {
            gc_vcs::PatchOp::Replace { path, value } => {
                let vterm = store_get_term(store, value)
                    .map_err(|e| format!("patch value read error: {e}"))?;
                cur = replace_at(&cur, path, vterm)?;
            }
            gc_vcs::PatchOp::Insert { path, value } => {
                let vterm = store_get_term(store, value)
                    .map_err(|e| format!("patch value read error: {e}"))?;
                cur = insert_at(&cur, path, vterm)?;
            }
            gc_vcs::PatchOp::Delete { path } => {
                cur = delete_at(&cur, path)?;
            }
            gc_vcs::PatchOp::Rename { .. } => {
                return Err("patch op :rename is not supported yet".to_string());
            }
        }
    }
    Ok(cur)
}

fn mk_conflict_artifact(
    kind: &str,
    base: &str,
    left: &str,
    right: &str,
    conflicts: Vec<Term>,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/conflict"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":kind")), Term::symbol(kind)),
            (
                TermOrdKey(Term::symbol(":base")),
                Term::Str(base.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":left")),
                Term::Str(left.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":right")),
                Term::Str(right.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":conflicts")),
                Term::Vector(conflicts),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn effective_base_dir(pol: Option<&OpPolicy>) -> Result<PathBuf, EffectsError> {
    if let Some(pol) = pol
        && let Some(base) = &pol.base_dir
    {
        return Ok(base.clone());
    }
    Ok(std::env::current_dir()?)
}

fn sandbox_path_read(base_dir: &Path, input: &str) -> Result<PathBuf, EffectsError> {
    let base = std::fs::canonicalize(base_dir)?;
    let p = PathBuf::from(input);
    // Reject any `..` to prevent traversal. We allow absolute paths only within base.
    for c in p.components() {
        if matches!(c, std::path::Component::ParentDir) {
            return Err(EffectsError::BadPayload(
                "path must not contain '..'".to_string(),
            ));
        }
    }
    let candidate = if p.is_absolute() { p } else { base.join(p) };
    // Resolve symlinks and ensure the result stays inside the base.
    let resolved = std::fs::canonicalize(&candidate)?;
    if !resolved.starts_with(&base) {
        return Err(EffectsError::BadPayload(format!(
            "path escapes base dir: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

fn sandbox_path_write(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<PathBuf, EffectsError> {
    let base = std::fs::canonicalize(base_dir)?;
    let p = PathBuf::from(input);
    for c in p.components() {
        if matches!(c, std::path::Component::ParentDir) {
            return Err(EffectsError::BadPayload(
                "path must not contain '..'".to_string(),
            ));
        }
    }
    let candidate = if p.is_absolute() { p } else { base.join(p) };
    // Validate parent directory is under base once directories exist.
    if let Some(parent) = candidate.parent() {
        if create_dirs {
            std::fs::create_dir_all(parent)?;
        }
        let parent_resolved = std::fs::canonicalize(parent)?;
        if !parent_resolved.starts_with(&base) {
            return Err(EffectsError::BadPayload(format!(
                "path escapes base dir: {}",
                candidate.display()
            )));
        }
    }
    Ok(candidate)
}

fn atomic_write_text(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_i: u64 = 0;
    let tmp_path = loop {
        let cand = dir.join(format!(
            ".tmp-{}-{}-{}",
            std::process::id(),
            path.file_name().and_then(|s| s.to_str()).unwrap_or("lock"),
            tmp_i
        ));
        tmp_i = tmp_i.saturating_add(1);
        match OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(mut f) => {
                f.write_all(bytes)?;
                f.sync_all()?;
                break cand;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };

    match std::fs::rename(&tmp_path, path) {
        Ok(()) => {
            #[cfg(unix)]
            {
                let d = std::fs::File::open(dir)?;
                d.sync_all()?;
            }
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

#[derive(Debug, Clone)]
enum Selector {
    Commit(String),
    Snapshot(String),
    Ref(String),
}

fn parse_selector(s: &str) -> Option<Selector> {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("commit:") {
        return Some(Selector::Commit(rest.trim().to_string()));
    }
    if let Some(rest) = t.strip_prefix("snapshot:") {
        return Some(Selector::Snapshot(rest.trim().to_string()));
    }
    if let Some(rest) = t.strip_prefix("ref:") {
        return Some(Selector::Ref(rest.trim().to_string()));
    }
    if t.starts_with("refs/") {
        return Some(Selector::Ref(t.to_string()));
    }
    if gc_vcs::validate_hex_hash(t).is_ok() {
        return Some(Selector::Commit(t.to_string()));
    }
    None
}

fn validate_commit_artifact_closure(
    store: &ArtifactStore,
    dep_name: &str,
    snapshot_hex: &str,
    commit_hex: &str,
    require_evidence_for_obligations: bool,
    error_tok: SealId,
    op: &str,
) -> Result<u64, Value> {
    let mut checked: u64 = 0;
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut ensure_hash = |h: &str| -> Result<(), Value> {
        if !store.path_for(h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        if seen.insert(h.to_string()) {
            checked = checked.saturating_add(1);
        }
        Ok(())
    };

    ensure_hash(commit_hex)?;
    let commit_term = match store_get_term(store, commit_hex) {
        Ok(t) => t,
        Err(_) => {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {commit_hex}"),
                Some(op),
            ));
        }
    };
    let c = match gc_vcs::Commit::from_term(&commit_term) {
        Ok(c) => c,
        Err(e) => {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-commit",
                e.to_string(),
                Some(op),
            ));
        }
    };
    if c.result != snapshot_hex {
        return Err(mk_error(
            error_tok,
            "core/pkg/commit-snapshot-mismatch",
            format!("commit.result != locked.snapshot for {dep_name}"),
            Some(op),
        ));
    }
    if let Some(base) = c.base.as_deref() {
        ensure_hash(base)?;
    }
    ensure_hash(&c.patch)?;
    ensure_hash(&c.result)?;

    if require_evidence_for_obligations && !c.obligations.is_empty() && c.evidence.is_empty() {
        return Err(mk_error(
            error_tok,
            "core/pkg/missing-evidence",
            format!("commit has obligations but no evidence for {dep_name}"),
            Some(op),
        ));
    }

    for evh in &c.evidence {
        ensure_hash(evh)?;
        let ev_term = match store_get_term(store, evh) {
            Ok(t) => t,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/bad-evidence",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        if let Err(e) = gc_vcs::Evidence::from_term(&ev_term) {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-evidence",
                e.to_string(),
                Some(op),
            ));
        }
    }
    for at_h in &c.attestations {
        ensure_hash(at_h)?;
        let at_term = match store_get_term(store, at_h) {
            Ok(t) => t,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/bad-attestation",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        if let Err(e) = gc_vcs::Attestation::from_term(&at_term) {
            return Err(mk_error(
                error_tok,
                "core/pkg/bad-attestation",
                e.to_string(),
                Some(op),
            ));
        }
    }
    Ok(checked)
}

fn resolve_requirement(
    store: &ArtifactStore,
    refs: &RefsDb,
    _name: &str,
    req: &gc_pkg::Requirement,
    error_tok: SealId,
    op: &str,
) -> Result<gc_pkg::LockedEntry, Value> {
    let sel = parse_selector(&req.selector).ok_or_else(|| {
        mk_error(
            error_tok,
            "core/pkg/bad-selector",
            format!("unsupported selector: {}", req.selector),
            Some(op),
        )
    })?;

    match sel {
        Selector::Snapshot(h) => {
            if let Err(e) = gc_vcs::validate_hex_hash(&h) {
                return Err(mk_error(error_tok, "core/pkg/bad-selector", e, Some(op)));
            }
            Ok(gc_pkg::LockedEntry {
                commit: None,
                snapshot: h,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: None,
                exports_hash: None,
            })
        }
        Selector::Commit(h) => {
            if let Err(e) = gc_vcs::validate_hex_hash(&h) {
                return Err(mk_error(error_tok, "core/pkg/bad-selector", e, Some(op)));
            }
            if !store.path_for(&h).exists() {
                return Err(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("artifact not found: {h}"),
                    Some(op),
                ));
            }
            let t = store_get_term(store, &h)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let c = gc_vcs::Commit::from_term(&t)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            Ok(gc_pkg::LockedEntry {
                commit: Some(h),
                snapshot: c.result,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: None,
                exports_hash: None,
            })
        }
        Selector::Ref(rn) => {
            let h = refs
                .get(&rn)
                .map_err(|e| mk_error(error_tok, "core/refs/io-error", e.to_string(), Some(op)))?;
            let Some(commit_hex) = h else {
                return Err(mk_error(
                    error_tok,
                    "core/pkg/ref-not-found",
                    format!("ref not found: {rn}"),
                    Some(op),
                ));
            };
            if !store.path_for(&commit_hex).exists() {
                return Err(mk_error(
                    error_tok,
                    "core/store/not-found",
                    format!("artifact not found: {commit_hex}"),
                    Some(op),
                ));
            }
            let t = store_get_term(store, &commit_hex)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            let c = gc_vcs::Commit::from_term(&t)
                .map_err(|e| mk_error(error_tok, "core/pkg/bad-commit", e.to_string(), Some(op)))?;
            Ok(gc_pkg::LockedEntry {
                commit: Some(commit_hex),
                snapshot: c.result,
                registry: req.registry.clone(),
                source_selector: req.selector.clone(),
                resolved_ref: Some(rn),
                exports_hash: None,
            })
        }
    }
}

fn payload_pkg_lock(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":lock"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        None => Ok("genesis.lock".to_string()),
        Some(other) => Err(format!(":lock must be string, got {}", print_term(other))),
    }
}

fn payload_pkg_workspace(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":workspace"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :workspace string".to_string()),
    }
}

fn payload_pkg_policy(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":policy"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_pkg_registry_default(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":registry-default"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_pkg_name(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":name"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :name string".to_string()),
    }
}

fn payload_pkg_selector(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":selector"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :selector string".to_string()),
    }
}

fn payload_pkg_registry(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":registry"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_pkg_update_policy(payload: &Term) -> Result<Option<gc_pkg::UpdatePolicy>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":update-policy"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => match s.as_str() {
            "auto" => Ok(Some(gc_pkg::UpdatePolicy::Auto)),
            "manual" => Ok(Some(gc_pkg::UpdatePolicy::Manual)),
            other => Err(format!(
                ":update-policy must be 'manual' or 'auto', got {other}"
            )),
        },
        Some(other) => Err(format!(
            ":update-policy must be string or nil, got {}",
            print_term(other)
        )),
    }
}

fn payload_pkg_bool(payload: &Term, key: &str) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn payload_pkg_publish_remote(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":remote"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :remote string".to_string()),
    }
}

fn payload_pkg_publish_ref(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":ref"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :ref string".to_string()),
    }
}

fn payload_pkg_publish_policy(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":policy"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :policy string".to_string()),
    }
}

fn payload_pkg_publish_expected_old(payload: &Term) -> Result<Option<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":expected-old"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            ":expected-old must be string or nil, got {}",
            print_term(other)
        )),
    }
}

fn payload_pkg_publish_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

fn payload_pkg_publish_commit(payload: &Term) -> Result<Option<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":commit"))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            ":commit must be string or nil, got {}",
            print_term(other)
        )),
    }
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone)]
struct SyncPolicy {
    remote_allow: Vec<String>,
    allow_http: bool,
    transfer_workers: usize,
}

#[cfg(not(target_os = "wasi"))]
fn sync_policy_from_op(pol: Option<&OpPolicy>) -> Result<SyncPolicy, String> {
    let mut remote_allow: Vec<String> = Vec::new();
    let mut allow_http = false;
    let mut transfer_workers: usize = 4;
    if let Some(pol) = pol {
        if let Some(v) = pol.extra.get("remote_allow")
            && let Some(arr) = v.as_array()
        {
            for x in arr {
                let s = x
                    .as_str()
                    .ok_or_else(|| "remote_allow entries must be strings".to_string())?;
                let t = s.trim();
                if !t.is_empty() {
                    remote_allow.push(t.to_string());
                }
            }
        }
        if let Some(v) = pol.extra.get("allow_http")
            && let Some(b) = v.as_bool()
        {
            allow_http = b;
        }
        if let Some(v) = pol.extra.get("transfer_workers")
            && let Some(n) = v.as_integer()
            && n > 0
        {
            transfer_workers = (n as usize).clamp(1, 64);
        }
    }
    if remote_allow.is_empty() {
        return Err("sync requires per-op remote_allow allowlist in caps.toml".to_string());
    }
    Ok(SyncPolicy {
        remote_allow,
        allow_http,
        transfer_workers,
    })
}

#[cfg(not(target_os = "wasi"))]
fn sync_normalize_and_check_remote(sp: &SyncPolicy, remote: &str) -> Result<String, String> {
    let base = gc_registry::normalize_remote_base(remote).map_err(|e| format!("{e}"))?;
    if base.scheme() == "http" && !sp.allow_http {
        return Err("http remotes are disabled by policy (set allow_http=true)".to_string());
    }
    let base_s = base.as_str().to_string();
    for p in &sp.remote_allow {
        if base_s.starts_with(p) {
            return Ok(base_s);
        }
    }
    Err("remote is not in policy remote_allow allowlist".to_string())
}

#[cfg(not(target_os = "wasi"))]
fn store_normalize_and_check_remote(policy: &CapsPolicy, remote: &str) -> Result<String, String> {
    let base = gc_registry::normalize_remote_base(remote).map_err(|e| format!("{e}"))?;
    if base.scheme() == "http" && !policy.store.allow_http {
        return Err("http remotes are disabled by policy (set store.allow_http=true)".to_string());
    }
    if policy.store.remote_allow.is_empty() {
        return Err("store remote requires store.remote_allow allowlist in caps.toml".to_string());
    }
    let base_s = base.as_str().to_string();
    for p in &policy.store.remote_allow {
        if base_s.starts_with(p) {
            return Ok(base_s);
        }
    }
    Err("store remote is not in policy store.remote_allow allowlist".to_string())
}

#[cfg(not(target_os = "wasi"))]
fn store_remote_client(
    policy: &CapsPolicy,
    timeout_ms: Option<u64>,
    error_tok: SealId,
    op: &str,
) -> Result<Option<(gc_registry::RegistryClient, String)>, Value> {
    let Some(remote) = &policy.store.remote else {
        return Ok(None);
    };
    let base = match store_normalize_and_check_remote(policy, remote) {
        Ok(b) => b,
        Err(e) => {
            return Err(mk_error(error_tok, "core/store/remote-denied", e, Some(op)));
        }
    };
    let client = match gc_registry::RegistryClient::new(
        &base,
        timeout_ms.map(std::time::Duration::from_millis),
    ) {
        Ok(c) => c,
        Err(e) => {
            return Err(mk_error(
                error_tok,
                "core/store/remote-error",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    Ok(Some((client, base)))
}

#[cfg(not(target_os = "wasi"))]
fn payload_sync_remote(payload: &Term) -> Result<String, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    match m.get(&TermOrdKey(Term::symbol(":remote"))) {
        Some(Term::Str(s)) => Ok(s.clone()),
        _ => Err("missing :remote string".to_string()),
    }
}

#[cfg(not(target_os = "wasi"))]
fn payload_sync_refs(payload: &Term) -> Result<Vec<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":refs must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            other => {
                return Err(format!(
                    ":refs entries must be strings, got {}",
                    print_term(other)
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(not(target_os = "wasi"))]
fn payload_sync_roots(payload: &Term) -> Result<Vec<String>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":roots"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":roots must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            other => {
                return Err(format!(
                    ":roots entries must be strings, got {}",
                    print_term(other)
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(not(target_os = "wasi"))]
fn payload_sync_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

#[cfg(not(target_os = "wasi"))]
fn payload_sync_force(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":force"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug, Clone)]
struct SyncSetRef {
    name: String,
    hash: String,
    policy: String,
    expected_old: Option<String>,
}

#[cfg(not(target_os = "wasi"))]
fn payload_sync_set_refs(payload: &Term) -> Result<Vec<SyncSetRef>, String> {
    let Term::Map(m) = payload else {
        return Err("payload must be a map".to_string());
    };
    let Some(t) = m.get(&TermOrdKey(Term::symbol(":set-refs"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(":set-refs must be vector, got {}", print_term(t)));
    };
    let mut out = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for x in xs {
        let Term::Map(mm) = x else {
            return Err(format!(
                ":set-refs entries must be maps, got {}",
                print_term(x)
            ));
        };
        let name = match mm.get(&TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) if !s.trim().is_empty() => s.clone(),
            _ => return Err("set-ref missing :name string".to_string()),
        };
        if !seen.insert(name.clone()) {
            return Err(format!("duplicate set-ref target: {name}"));
        }
        let hash = match mm.get(&TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(s)) if gc_vcs::validate_hex_hash(s).is_ok() => s.to_ascii_lowercase(),
            Some(Term::Str(_)) => return Err("set-ref :hash must be 64-hex".to_string()),
            _ => return Err("set-ref missing :hash string".to_string()),
        };
        let policy = match mm.get(&TermOrdKey(Term::symbol(":policy"))) {
            Some(Term::Str(s)) if gc_vcs::validate_hex_hash(s).is_ok() => s.to_ascii_lowercase(),
            Some(Term::Str(_)) => return Err("set-ref :policy must be 64-hex".to_string()),
            _ => return Err("set-ref missing :policy string".to_string()),
        };
        let expected_old = match mm.get(&TermOrdKey(Term::symbol(":expected-old"))) {
            None | Some(Term::Nil) => None,
            Some(Term::Str(s)) if s == "nil" => Some("nil".to_string()),
            Some(Term::Str(s)) if gc_vcs::validate_hex_hash(s).is_ok() => {
                Some(s.to_ascii_lowercase())
            }
            Some(Term::Str(_)) => {
                return Err("set-ref :expected-old must be 64-hex or nil".to_string());
            }
            Some(other) => {
                return Err(format!(
                    "set-ref :expected-old must be string or nil, got {}",
                    print_term(other)
                ));
            }
        };
        out.push(SyncSetRef {
            name,
            hash,
            policy,
            expected_old,
        });
    }
    Ok(out)
}

#[cfg(not(target_os = "wasi"))]
struct SyncPullStats<'a> {
    pulled: &'a mut u64,
    already: &'a mut u64,
    error_tok: SealId,
    op: &'a str,
    transfer_workers: usize,
}

#[cfg(not(target_os = "wasi"))]
fn sync_pull_closure(
    client: &gc_registry::RegistryClient,
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    stats: &mut SyncPullStats<'_>,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};

    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;
    let batch_cap = (stats.transfer_workers.max(1) * 8).max(8);

    while !q.is_empty() {
        let mut batch: Vec<(String, u64)> = Vec::new();
        while batch.len() < batch_cap {
            let Some((h, dleft)) = q.pop_front() else {
                break;
            };
            if !seen.insert(h.clone()) {
                continue;
            }
            obj_count = obj_count.saturating_add(1);
            if obj_count > 50_000 {
                return Err(mk_error(
                    stats.error_tok,
                    "core/sync/too-many-objects",
                    "closure exceeded 50k objects".to_string(),
                    Some(stats.op),
                ));
            }
            batch.push((h, dleft));
        }
        if batch.is_empty() {
            continue;
        }

        let mut missing_hashes: Vec<String> = Vec::new();
        for (h, _) in &batch {
            if store.path_for(h).exists() {
                if store.verify_hex(h).is_err() {
                    return Err(mk_error(
                        stats.error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {h}"),
                        Some(stats.op),
                    ));
                }
                *stats.already = stats.already.saturating_add(1);
            } else {
                missing_hashes.push(h.clone());
            }
        }

        if !missing_hashes.is_empty() {
            let dl_results =
                sync_parallel_store_get_bytes(client, &missing_hashes, stats.transfer_workers);
            for (i, h) in missing_hashes.iter().enumerate() {
                let bytes = match &dl_results[i] {
                    Ok(b) => b,
                    Err(e) => {
                        return Err(mk_error(
                            stats.error_tok,
                            "core/sync/remote-error",
                            e.clone(),
                            Some(stats.op),
                        ));
                    }
                };
                let got = store.put_bytes(bytes).map_err(|e| {
                    mk_error(
                        stats.error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(stats.op),
                    )
                })?;
                if got != *h {
                    return Err(mk_error(
                        stats.error_tok,
                        "core/sync/hash-mismatch",
                        "remote bytes hash mismatch".to_string(),
                        Some(stats.op),
                    ));
                }
                *stats.pulled = stats.pulled.saturating_add(1);
            }
        }

        for (h, dleft) in batch {
            let t = match store_get_term(store, &h) {
                Ok(t) => t,
                Err(_) => continue,
            };

            // Commit closure: commit, base, patch, result snapshot, evidence, attestations, parents.
            if let Ok(c) = gc_vcs::Commit::from_term(&t) {
                if let Some(b) = c.base {
                    q.push_back((b, dleft));
                }
                q.push_back((c.patch, dleft));
                q.push_back((c.result, dleft));
                for x in c.evidence {
                    q.push_back((x, dleft));
                }
                for x in c.attestations {
                    q.push_back((x, dleft));
                }
                if dleft > 0 {
                    for p in c.parents {
                        q.push_back((p, dleft - 1));
                    }
                }
                continue;
            }

            // Patch closure: follow referenced values.
            if let Ok(p) = gc_vcs::Patch::from_term(&t) {
                for x in p.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Evidence closure: follow any referenced inputs/outputs/data.
            if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
                for x in e.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Conflict closure: follow referenced snapshots and referenced handler/value hashes.
            if let Ok(c) = gc_vcs::Conflict::from_term(&t) {
                for x in c.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Snapshot closure: shallow refs.
            if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
                for x in s.shallow_refs() {
                    q.push_back((x, dleft));
                }
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "wasi"))]
fn sync_parallel_store_get_bytes(
    client: &gc_registry::RegistryClient,
    hashes: &[String],
    workers: usize,
) -> Vec<Result<Vec<u8>, String>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if hashes.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(hashes.len());
    if workers <= 1 {
        return hashes
            .iter()
            .map(|h| client.store_get(h).map_err(|e| format!("{e}")))
            .collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<Result<Vec<u8>, String>>>>> =
        Arc::new(Mutex::new((0..hashes.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= hashes.len() {
                        break;
                    }
                    let res = c.store_get(&hashes[i]).map_err(|e| format!("{e}"));
                    let mut g = out.lock().expect("sync get results lock");
                    g[i] = Some(res);
                }
            });
        }
    });
    let mut g = out.lock().expect("sync get results lock");
    g.drain(..).map(|x| x.expect("filled")).collect()
}

#[cfg(not(target_os = "wasi"))]
fn sync_parallel_store_has_chunks(
    client: &gc_registry::RegistryClient,
    chunks: &[Vec<String>],
    workers: usize,
) -> Vec<Result<BTreeMap<String, bool>, String>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if chunks.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(chunks.len());
    if workers <= 1 {
        return chunks
            .iter()
            .map(|chunk| client.store_has(chunk).map_err(|e| format!("{e}")))
            .collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<Result<BTreeMap<String, bool>, String>>>>> =
        Arc::new(Mutex::new((0..chunks.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= chunks.len() {
                        break;
                    }
                    let res = c.store_has(&chunks[i]).map_err(|e| format!("{e}"));
                    let mut g = out.lock().expect("sync has results lock");
                    g[i] = Some(res);
                }
            });
        }
    });
    let mut g = out.lock().expect("sync has results lock");
    g.drain(..).map(|x| x.expect("filled")).collect()
}

#[cfg(not(target_os = "wasi"))]
fn sync_parallel_upload_missing(
    client: &gc_registry::RegistryClient,
    store: &ArtifactStore,
    missing: &[String],
    workers: usize,
) -> Vec<Result<(), String>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if missing.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(missing.len());
    if workers <= 1 {
        return missing
            .iter()
            .map(|h| {
                let bytes = store.get_bytes(h).map_err(|e| format!("store-read:{e}"))?;
                client.store_put(h, &bytes).map_err(|e| format!("{e}"))
            })
            .collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<Result<(), String>>>>> =
        Arc::new(Mutex::new((0..missing.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            let s = store.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= missing.len() {
                        break;
                    }
                    let h = &missing[i];
                    let res = s
                        .get_bytes(h)
                        .map_err(|e| format!("store-read:{e}"))
                        .and_then(|bytes| c.store_put(h, &bytes).map_err(|e| format!("{e}")));
                    let mut g = out.lock().expect("sync put results lock");
                    g[i] = Some(res);
                }
            });
        }
    });
    let mut g = out.lock().expect("sync put results lock");
    g.drain(..).map(|x| x.expect("filled")).collect()
}

fn sync_closure_local(
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    out: &mut std::collections::BTreeSet<String>,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};
    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;

    while let Some((h, dleft)) = q.pop_front() {
        if !seen.insert(h.clone()) {
            continue;
        }
        obj_count = obj_count.saturating_add(1);
        if obj_count > 50_000 {
            return Err(mk_error(
                error_tok,
                "core/sync/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(op),
            ));
        }
        if !store.path_for(&h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(&h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        out.insert(h.clone());

        let t = match store_get_term(store, &h) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(c) = gc_vcs::Commit::from_term(&t) {
            if let Some(b) = c.base {
                q.push_back((b, dleft));
            }
            q.push_back((c.patch, dleft));
            q.push_back((c.result, dleft));
            for x in c.evidence {
                q.push_back((x, dleft));
            }
            for x in c.attestations {
                q.push_back((x, dleft));
            }
            if dleft > 0 {
                for p in c.parents {
                    q.push_back((p, dleft - 1));
                }
            }
            continue;
        }
        if let Ok(p) = gc_vcs::Patch::from_term(&t) {
            for x in p.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
            for x in e.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(c) = gc_vcs::Conflict::from_term(&t) {
            for x in c.refs() {
                q.push_back((x, dleft));
            }
            continue;
        }
        if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
            for x in s.shallow_refs() {
                q.push_back((x, dleft));
            }
        }
    }
    Ok(())
}

fn resolve_gpk_root_for_export(
    store: &ArtifactStore,
    refs: Option<&RefsDb>,
    root_spec: &str,
    mode: GpkMode,
    error_tok: SealId,
    op: &str,
) -> Result<String, Value> {
    let mut root = root_spec.trim().to_string();
    if let Some(s) = root.strip_prefix("h:") {
        root = s.to_string();
    }
    if gc_vcs::validate_hex_hash(&root).is_ok() {
        return Ok(root.to_ascii_lowercase());
    }
    if let Some(s) = root.strip_prefix("ref:") {
        root = s.to_string();
    }
    if !root.starts_with("refs/") {
        return Err(mk_error(
            error_tok,
            "core/gpk/bad-root",
            "root must be a hash or refs/...".to_string(),
            Some(op),
        ));
    }
    let refs = refs.ok_or_else(|| {
        mk_error(
            error_tok,
            "core/gpk/missing-refs-db",
            "refs db required when root is a ref".to_string(),
            Some(op),
        )
    })?;
    let resolved = refs
        .get(&root)
        .map_err(|e| mk_error(error_tok, "core/gpk/refs-io-error", e.to_string(), Some(op)))?;
    let Some(hash) = resolved else {
        return Err(mk_error(
            error_tok,
            "core/gpk/ref-not-found",
            format!("ref not found: {root}"),
            Some(op),
        ));
    };
    let hash = hash.to_ascii_lowercase();
    let root_term = match store_get_term(store, &hash) {
        Ok(t) => t,
        Err(_) => {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {hash}"),
                Some(op),
            ));
        }
    };
    if mode == GpkMode::Shallow && gc_vcs::Snapshot::from_term(&root_term).is_err() {
        return Err(mk_error(
            error_tok,
            "core/gpk/bad-root",
            "shallow export root must resolve to a :vcs/snapshot".to_string(),
            Some(op),
        ));
    }
    Ok(hash)
}

#[derive(Copy, Clone, Debug)]
struct GpkClosureOptions<'a> {
    depth: u64,
    mode: GpkMode,
    include_evidence: GpkIncludeEvidence,
    include_deps: GpkIncludeDeps,
    root_snapshot_for_locked_deps: Option<&'a str>,
}

fn gpk_export_closure_local(
    store: &ArtifactStore,
    root: &str,
    opts: GpkClosureOptions<'_>,
    out: &mut std::collections::BTreeSet<String>,
    error_tok: SealId,
    op: &str,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};

    let mut helper_ctx = EvalCtx::new();
    let helper_prelude = build_prelude(&mut helper_ctx);
    let helper_ref_plan_fn = helper_prelude
        .env
        .get("core/vcs/reach::artifact-ref-plan")
        .ok_or_else(|| {
            mk_error(
                error_tok,
                "core/gpk/planner-missing",
                "missing prelude binding core/vcs/reach::artifact-ref-plan".to_string(),
                Some(op),
            )
        })?;

    let mut q: VecDeque<(String, u64, bool)> = VecDeque::new();
    q.push_back((root.to_string(), opts.depth, true));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;

    while let Some((h, dleft, is_root)) = q.pop_front() {
        if !seen.insert(h.clone()) {
            continue;
        }
        obj_count = obj_count.saturating_add(1);
        if obj_count > 50_000 {
            return Err(mk_error(
                error_tok,
                "core/sync/too-many-objects",
                "closure exceeded 50k objects".to_string(),
                Some(op),
            ));
        }
        if !store.path_for(&h).exists() {
            return Err(mk_error(
                error_tok,
                "core/store/not-found",
                format!("artifact not found: {h}"),
                Some(op),
            ));
        }
        if store.verify_hex(&h).is_err() {
            return Err(mk_error(
                error_tok,
                "core/store/corruption",
                format!("artifact store corruption: {h}"),
                Some(op),
            ));
        }
        out.insert(h.clone());

        let t = match store_get_term(store, &h) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let include_commit_evidence = match opts.include_evidence {
            GpkIncludeEvidence::None => false,
            GpkIncludeEvidence::Required => is_root,
            GpkIncludeEvidence::All => true,
        };
        let follow_deps = match opts.include_deps {
            GpkIncludeDeps::None => false,
            GpkIncludeDeps::Locked => opts
                .root_snapshot_for_locked_deps
                .map(|hh| hh.eq_ignore_ascii_case(&h))
                .unwrap_or(false),
            GpkIncludeDeps::All => true,
        };

        let mut opts_map = BTreeMap::new();
        opts_map.insert(
            TermOrdKey(Term::symbol(":include-evidence")),
            Term::Bool(include_commit_evidence),
        );
        opts_map.insert(
            TermOrdKey(Term::symbol(":include-deps")),
            Term::Bool(follow_deps),
        );
        opts_map.insert(
            TermOrdKey(Term::symbol(":include-parents")),
            Term::Bool(opts.mode == GpkMode::Full && dleft > 0),
        );
        let opts_term = Term::Map(opts_map);

        let plan_term = helper_ref_plan_fn
            .clone()
            .apply(&mut helper_ctx, Value::Data(t.clone()))
            .and_then(|f| f.apply(&mut helper_ctx, Value::Data(opts_term)))
            .map(|v| v.to_term_for_log(helper_ctx.protocol.map(|p| p.error)))
            .map_err(|e| {
                mk_error(
                    error_tok,
                    "core/gpk/planner-error",
                    format!("core/vcs/reach::artifact-ref-plan failed: {e}"),
                    Some(op),
                )
            })?;
        let (refs_to_follow, parent_refs) = gpk_ref_plan_from_term(&plan_term);
        for x in refs_to_follow {
            q.push_back((x, dleft, false));
        }

        if dleft > 0 {
            for p in parent_refs {
                q.push_back((p, dleft - 1, false));
            }
        }
    }
    Ok(())
}

fn gpk_ref_hashes_from_term(t: &Term) -> Vec<String> {
    let Term::Vector(xs) = t else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for x in xs {
        let s = match x {
            Term::Str(s) | Term::Symbol(s) => s,
            _ => continue,
        };
        if gc_vcs::validate_hex_hash(s).is_ok() {
            out.push(s.to_ascii_lowercase());
        }
    }
    out
}

fn gpk_ref_plan_from_term(t: &Term) -> (Vec<String>, Vec<String>) {
    let Term::Map(m) = t else {
        return (Vec::new(), Vec::new());
    };
    let refs = m
        .get(&TermOrdKey(Term::symbol(":refs")))
        .map(gpk_ref_hashes_from_term)
        .unwrap_or_default();
    let parents = m
        .get(&TermOrdKey(Term::symbol(":parents")))
        .map(gpk_ref_hashes_from_term)
        .unwrap_or_default();
    (refs, parents)
}

fn gc_build_sources(
    refs: Option<&RefsDb>,
    base_dir: &Path,
    lock_s: &str,
    pins_s: &str,
    include_lock: bool,
    include_refs: bool,
    error_tok: SealId,
    op: &str,
) -> Result<(Vec<Term>, Term, Term), Value> {
    let mut ref_entries: Vec<Term> = Vec::new();
    if include_refs && let Some(rdb) = refs {
        match rdb.list(None) {
            Ok(list) => {
                for r in list {
                    let mut m = BTreeMap::new();
                    m.insert(TermOrdKey(Term::symbol(":name")), Term::Str(r.name));
                    m.insert(
                        TermOrdKey(Term::symbol(":hash")),
                        r.hash.map(Term::Str).unwrap_or(Term::Nil),
                    );
                    ref_entries.push(Term::Map(m));
                }
            }
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/gc/refs-io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
        }
    }

    let mut lock_entries_term: Vec<Term> = Vec::new();
    let mut lock_artifacts_term: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
    if include_lock
        && let Ok(lock_path) = sandbox_path_read(base_dir, lock_s)
        && lock_path.exists()
    {
        match gc_pkg::GenesisLock::load(&lock_path) {
            Ok(lk) => {
                for (_, le) in lk.locked {
                    let mut m = BTreeMap::new();
                    m.insert(
                        TermOrdKey(Term::symbol(":commit")),
                        le.commit.map(Term::Str).unwrap_or(Term::Nil),
                    );
                    m.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(le.snapshot),
                    );
                    lock_entries_term.push(Term::Map(m));
                }
                for (k, v) in lk.artifacts {
                    lock_artifacts_term.insert(TermOrdKey(Term::Str(k)), Term::Str(v));
                }
            }
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/gc/bad-lock",
                    format!("{e}"),
                    Some(op),
                ));
            }
        }
    }
    let lock_info = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock_s.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":locked")),
                Term::Vector(lock_entries_term),
            ),
            (
                TermOrdKey(Term::symbol(":artifacts")),
                Term::Map(lock_artifacts_term),
            ),
        ]
        .into_iter()
        .collect(),
    );

    let pins = match gc_pins_load(base_dir, pins_s) {
        Ok(p) => p,
        Err(e) => return Err(mk_error(error_tok, "core/gc/bad-pins", e, Some(op))),
    };
    let mut keep_refs_term: Vec<Term> = Vec::new();
    for rname in &pins.keep_refs {
        let Some(rdb) = refs else {
            return Err(mk_error(
                error_tok,
                "core/gc/missing-refs-db",
                "pins.keep_refs requires refs db".to_string(),
                Some(op),
            ));
        };
        let cur = match rdb.get(rname) {
            Ok(h) => h,
            Err(e) => {
                return Err(mk_error(
                    error_tok,
                    "core/gc/refs-io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
        };
        let Some(h) = cur else {
            return Err(mk_error(
                error_tok,
                "core/gc/ref-not-found",
                format!("pinned ref not found: {rname}"),
                Some(op),
            ));
        };
        keep_refs_term.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(rname.clone())),
                (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
            ]
            .into_iter()
            .collect(),
        ));
    }
    let pins_info = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(pins.keep.into_iter().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(keep_refs_term),
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok((ref_entries, lock_info, pins_info))
}

fn gc_roots_plan_from_sources(
    refs_entries: &[Term],
    lock_info: &Term,
    pins_info: &Term,
    include_lock: bool,
    include_refs: bool,
    error_tok: SealId,
    op: &str,
) -> Result<(Vec<String>, Vec<Term>), Value> {
    let mut helper_ctx = EvalCtx::new();
    let helper_prelude = build_prelude(&mut helper_ctx);
    let roots_plan_fn = helper_prelude
        .env
        .get("core/gc/reach::roots-plan")
        .ok_or_else(|| {
            mk_error(
                error_tok,
                "core/gc/planner-missing",
                "missing prelude binding core/gc/reach::roots-plan".to_string(),
                Some(op),
            )
        })?;

    let plan_term = roots_plan_fn
        .apply(
            &mut helper_ctx,
            Value::Data(Term::Vector(refs_entries.to_vec())),
        )
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(lock_info.clone())))
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(pins_info.clone())))
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(Term::Bool(include_lock))))
        .and_then(|f| f.apply(&mut helper_ctx, Value::Data(Term::Bool(include_refs))))
        .map(|v| v.to_term_for_log(helper_ctx.protocol.map(|p| p.error)))
        .map_err(|e| {
            mk_error(
                error_tok,
                "core/gc/planner-error",
                format!("core/gc/reach::roots-plan failed: {e}"),
                Some(op),
            )
        })?;

    let Term::Map(m) = plan_term else {
        return Err(mk_error(
            error_tok,
            "core/gc/planner-error",
            "gc roots planner must return a map".to_string(),
            Some(op),
        ));
    };
    let roots = m
        .get(&TermOrdKey(Term::symbol(":roots")))
        .map(gpk_ref_hashes_from_term)
        .unwrap_or_default();
    let roots_meta = m
        .get(&TermOrdKey(Term::symbol(":roots-meta")))
        .and_then(|t| match t {
            Term::Vector(v) => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_default();

    Ok((roots, roots_meta))
}

// -----------------------------------------------------------------------------
// GC helpers (pins + store lock + store scan)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GcPins {
    keep: Vec<String>,
    keep_refs: Vec<String>,
}

impl GcPins {
    fn empty() -> Self {
        Self {
            keep: Vec::new(),
            keep_refs: Vec::new(),
        }
    }
}

fn gc_normalize_hash(s: &str) -> Option<String> {
    let ss = s.strip_prefix("h:").unwrap_or(s).trim();
    if gc_vcs::validate_hex_hash(ss).is_err() {
        return None;
    }
    Some(ss.to_ascii_lowercase())
}

fn gc_pins_load(base_dir: &Path, pins_path: &str) -> Result<GcPins, String> {
    let p = sandbox_path_allow_missing(base_dir, pins_path, false).map_err(|e| format!("{e}"))?;
    if !p.exists() {
        return Ok(GcPins::empty());
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("pins read failed: {e}"))?;
    let s = String::from_utf8(bytes).map_err(|_| "pins file is not utf-8".to_string())?;
    let v: toml::Value = toml::from_str(&s).map_err(|e| format!("pins toml parse: {e}"))?;

    let version = v.get("version").and_then(|x| x.as_integer()).unwrap_or(1);
    if version != 1 {
        return Err(format!("unsupported pins version: {version}"));
    }

    let pins_tbl = v
        .get("pins")
        .and_then(|x| x.as_table())
        .ok_or_else(|| "pins.toml missing [pins] table".to_string())?;

    let mut keep: Vec<String> = Vec::new();
    if let Some(arr) = pins_tbl.get("keep").and_then(|x| x.as_array()) {
        for x in arr {
            let Some(s) = x.as_str() else {
                return Err("pins.keep entries must be strings".to_string());
            };
            let Some(h) = gc_normalize_hash(s) else {
                return Err(format!("pins.keep contains invalid hash: {s}"));
            };
            keep.push(h);
        }
    }

    let mut keep_refs: Vec<String> = Vec::new();
    if let Some(arr) = pins_tbl.get("keep_refs").and_then(|x| x.as_array()) {
        for x in arr {
            let Some(s) = x.as_str() else {
                return Err("pins.keep_refs entries must be strings".to_string());
            };
            if !s.starts_with("refs/") {
                return Err(format!("pins.keep_refs must start with refs/: {s}"));
            }
            keep_refs.push(s.to_string());
        }
    }

    keep.sort();
    keep.dedup();
    keep_refs.sort();
    keep_refs.dedup();

    Ok(GcPins { keep, keep_refs })
}

fn gc_pins_write(path: &Path, pins: &GcPins) -> Result<(), EffectsError> {
    // Stable writer: fixed key order, single-line arrays.
    fn write_arr(buf: &mut String, xs: &[String]) {
        buf.push('[');
        for (i, x) in xs.iter().enumerate() {
            if i != 0 {
                buf.push_str(", ");
            }
            buf.push('"');
            for c in x.chars() {
                match c {
                    '\\' => buf.push_str("\\\\"),
                    '"' => buf.push_str("\\\""),
                    '\n' => buf.push_str("\\n"),
                    '\r' => buf.push_str("\\r"),
                    '\t' => buf.push_str("\\t"),
                    other => buf.push(other),
                }
            }
            buf.push('"');
        }
        buf.push(']');
    }

    let mut keep = pins.keep.clone();
    keep.sort();
    keep.dedup();
    let mut keep_refs = pins.keep_refs.clone();
    keep_refs.sort();
    keep_refs.dedup();

    let mut out = String::new();
    out.push_str("version = 1\n\n[pins]\nkeep = ");
    write_arr(&mut out, &keep);
    out.push('\n');
    out.push_str("keep_refs = ");
    write_arr(&mut out, &keep_refs);
    out.push('\n');

    atomic_write_text(path, out.as_bytes()).map_err(EffectsError::Io)
}

fn gc_store_lock(store_dir: &Path) -> Result<GcStoreLock, EffectsError> {
    std::fs::create_dir_all(store_dir)?;
    let lock_path = store_dir.join(".gc.lock");
    ExclusiveLock::acquire(&lock_path)
}

type GcDeadSet = (Vec<String>, u64, Vec<(String, u64)>);

fn gc_store_dead_set(
    store_dir: &Path,
    live: &std::collections::BTreeSet<String>,
) -> Result<GcDeadSet, EffectsError> {
    let mut dead: Vec<String> = Vec::new();
    let mut dead_bytes: u64 = 0;
    let mut largest: Vec<(String, u64)> = Vec::new();

    for ent in std::fs::read_dir(store_dir)? {
        let ent = ent?;
        let ft = ent.file_type()?;
        if !ft.is_file() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if gc_vcs::validate_hex_hash(&name).is_err() {
            continue;
        }
        if live.contains(&name) {
            continue;
        }
        let len = ent.metadata()?.len();
        dead_bytes = dead_bytes.saturating_add(len);
        dead.push(name.clone());
        largest.push((name, len));
    }

    dead.sort();
    // Largest list is deterministic: sort by size desc then hash asc.
    largest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if largest.len() > 25 {
        largest.truncate(25);
    }
    Ok((dead, dead_bytes, largest))
}

// Like sandbox_path_read, but supports paths that may not exist by validating the
// longest existing ancestor stays within base. This is used for GC quarantine/pins paths.
fn sandbox_path_allow_missing(
    base_dir: &Path,
    input: &str,
    create_dirs: bool,
) -> Result<PathBuf, EffectsError> {
    let base = std::fs::canonicalize(base_dir)?;
    let p = PathBuf::from(input);
    for c in p.components() {
        if matches!(c, std::path::Component::ParentDir) {
            return Err(EffectsError::BadPayload(
                "path must not contain '..'".to_string(),
            ));
        }
    }
    let candidate = if p.is_absolute() { p } else { base.join(p) };
    if create_dirs && let Some(parent) = candidate.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Find the longest existing ancestor to canonicalize for anti-symlink-escape.
    let mut cur = candidate.as_path();
    while !cur.exists() {
        let Some(p) = cur.parent() else { break };
        cur = p;
    }
    if cur.exists() {
        let resolved = std::fs::canonicalize(cur)?;
        if !resolved.starts_with(&base) {
            return Err(EffectsError::BadPayload(format!(
                "path escapes base dir: {}",
                resolved.display()
            )));
        }
    }
    // Lexical containment (no '..') ensures the final path cannot escape base.
    if !candidate.starts_with(&base) {
        return Err(EffectsError::BadPayload(format!(
            "path escapes base dir: {}",
            candidate.display()
        )));
    }
    Ok(candidate)
}

// -----------------------------------------------------------------------------
// GC payload parsing
// -----------------------------------------------------------------------------

fn payload_gc_lock(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":lock"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_gc_pins(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":pins"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_gc_depth(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":depth"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

fn payload_gc_include_lock(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":include-lock"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn payload_gc_include_refs(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":include-refs"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn payload_gc_quarantine(payload: &Term) -> Option<bool> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":quarantine"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn payload_gc_quarantine_dir(payload: &Term) -> Option<String> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":quarantine-dir"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn payload_gc_ttl_days(payload: &Term) -> Option<u64> {
    let Term::Map(m) = payload else { return None };
    match m.get(&TermOrdKey(Term::symbol(":ttl-days"))) {
        Some(Term::Int(i)) => i.to_u64(),
        _ => None,
    }
}

fn payload_gc_target(payload: &Term) -> Result<String, EffectsError> {
    let Term::Map(m) = payload else {
        return Err(EffectsError::BadPayload(
            "payload must be a map".to_string(),
        ));
    };
    let v = m
        .get(&TermOrdKey(Term::symbol(":target")))
        .ok_or_else(|| EffectsError::BadPayload("missing :target".to_string()))?;
    match v {
        Term::Str(s) => Ok(s.clone()),
        Term::Symbol(s) => Ok(s.clone()),
        _ => Err(EffectsError::BadPayload(format!(
            ":target must be string/symbol, got {}",
            print_term(v)
        ))),
    }
}
