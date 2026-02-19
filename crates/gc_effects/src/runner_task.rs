use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_bigint::BigInt;

use crate::policy::CapsPolicy;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TaskScheduleEvent {
    pub(crate) task_id: Option<String>,
    pub(crate) parent_task: Option<String>,
    pub(crate) schedule_step: Option<u64>,
    pub(crate) await_edge: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TaskBudgetState {
    per_task: BTreeMap<String, TaskBudgetCounters>,
}

#[derive(Debug, Clone)]
struct TaskBudgetCounters {
    first_step: u64,
    last_step: u64,
    steps: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TaskRuntime {
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

pub(crate) fn task_runtime_call(
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
            let state_before = match runtime.tasks.get(&task_id) {
                Some(rec) => rec.state.clone(),
                None => {
                    return Some(mk_error(
                        error_tok,
                        "core/task/internal-state",
                        format!("task disappeared before cancel: {task_id}"),
                        Some(op),
                    ));
                }
            };
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
            let rec = runtime.tasks.get(&task_id);
            let Some(rec) = rec else {
                return Some(mk_error(
                    error_tok,
                    "core/task/internal-state",
                    format!("task disappeared during cancel: {task_id}"),
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
            let state_before = match runtime.tasks.get(&task_id) {
                Some(rec) => rec.state.clone(),
                None => {
                    return Some(mk_error(
                        error_tok,
                        "core/task/internal-state",
                        format!("task disappeared before await: {task_id}"),
                        Some(op),
                    ));
                }
            };
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
            let rec = runtime.tasks.get(&task_id);
            let Some(rec) = rec else {
                return Some(mk_error(
                    error_tok,
                    "core/task/internal-state",
                    format!("task disappeared during await: {task_id}"),
                    Some(op),
                ));
            };
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

pub(crate) fn task_schedule_event_for(
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

pub(crate) fn enforce_task_policy_limits(
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

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
    mk_error_with_ctx(error_tok, code, msg, op, Term::Nil)
}

fn mk_error_with_ctx(
    error_tok: SealId,
    code: &str,
    msg: String,
    op: Option<&str>,
    ctx: Term,
) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(code.to_string()),
    );
    mm.insert(TermOrdKey(Term::symbol(":error/message")), Term::Str(msg));
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    mm.insert(TermOrdKey(Term::symbol(":error/context")), ctx);
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
