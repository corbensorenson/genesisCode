use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_bigint::BigInt;

use crate::policy::CapsPolicy;
use crate::runner_task_exec::execute_task_payload;
#[path = "runner_task_policy.rs"]
mod runner_task_policy;
#[path = "runner_task_terms.rs"]
mod runner_task_terms;
#[path = "runner_task_workers.rs"]
mod runner_task_workers;
pub(crate) use runner_task_policy::{
    TaskBudgetState, enforce_task_policy_limits, task_schedule_event_for,
};
use runner_task_terms::{map_field, map_field_int_u64, map_field_str_or_symbol, task_map};

#[derive(Debug, Default)]
pub(crate) struct TaskRuntime {
    next_task_id: u64,
    next_channel_id: u64,
    tasks: BTreeMap<String, TaskRecord>,
    channels: BTreeMap<String, ChannelRecord>,
    queue: VecDeque<String>,
    running_count: u64,
    queued_count: u64,
    completed: BTreeMap<String, TaskCompletion>,
    pool: TaskWorkerPool,
}

#[derive(Debug, Clone)]
struct TaskRecord {
    state: TaskState,
    payload: Term,
    result: Option<Term>,
    error: Option<Term>,
    parent_task: Option<String>,
    cancel_flag: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Clone, Default)]
struct ChannelRecord {
    capacity: Option<usize>,
    queue: VecDeque<Term>,
    closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Default)]
struct TaskWorkerPool {
    tx: Option<mpsc::Sender<TaskJob>>,
    rx: Option<mpsc::Receiver<TaskCompletion>>,
    workers: Vec<JoinHandle<()>>,
}

impl Drop for TaskWorkerPool {
    fn drop(&mut self) {
        self.tx.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[derive(Debug)]
struct TaskJob {
    task_id: String,
    payload: Term,
    cancel_flag: Arc<AtomicBool>,
    policy: Arc<CapsPolicy>,
}

#[derive(Debug, Clone)]
struct TaskCompletion {
    task_id: String,
    outcome: TaskOutcome,
}

#[derive(Debug, Clone)]
pub(crate) enum TaskOutcome {
    Done(Term),
    Failed(Term),
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
        "core/task::channel-open" => {
            let capacity = map_field_int_u64(payload, ":capacity")
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n > 0);
            let channel_id = format!("chan-{:016x}", runtime.next_channel_id);
            runtime.next_channel_id = runtime.next_channel_id.saturating_add(1);
            runtime.channels.insert(
                channel_id.clone(),
                ChannelRecord {
                    capacity,
                    queue: VecDeque::new(),
                    closed: false,
                },
            );
            Some(Value::Data(task_map([
                (":channel-id", Term::Str(channel_id)),
                (
                    ":capacity",
                    capacity
                        .map(|n| Term::Int(BigInt::from(n)))
                        .unwrap_or(Term::Nil),
                ),
                (":size", Term::Int(BigInt::from(0))),
                (":state", Term::symbol(":open")),
            ])))
        }
        "core/task::channel-send" => {
            let Some(channel_id) = map_field_str_or_symbol(payload, ":channel-id") else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::channel-send payload must include :channel-id".to_string(),
                    Some(op),
                ));
            };
            let Some(value) = map_field(payload, ":value").cloned() else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::channel-send payload must include :value".to_string(),
                    Some(op),
                ));
            };
            let Some(channel) = runtime.channels.get_mut(&channel_id) else {
                return Some(mk_error(
                    error_tok,
                    "core/task/not-found",
                    format!("unknown channel-id: {channel_id}"),
                    Some(op),
                ));
            };
            if channel.closed {
                return Some(mk_error(
                    error_tok,
                    "core/task/channel-closed",
                    format!("channel is closed: {channel_id}"),
                    Some(op),
                ));
            }
            if let Some(limit) = channel.capacity
                && channel.queue.len() >= limit
            {
                return Some(mk_error(
                    error_tok,
                    "core/task/channel-full",
                    format!("channel capacity exceeded: {channel_id}"),
                    Some(op),
                ));
            }
            channel.queue.push_back(value);
            Some(Value::Data(task_map([
                (":channel-id", Term::Str(channel_id)),
                (":size", Term::Int(BigInt::from(channel.queue.len()))),
                (":state", Term::symbol(":open")),
            ])))
        }
        "core/task::channel-recv" => {
            let Some(channel_id) = map_field_str_or_symbol(payload, ":channel-id") else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::channel-recv payload must include :channel-id".to_string(),
                    Some(op),
                ));
            };
            let Some(channel) = runtime.channels.get_mut(&channel_id) else {
                return Some(mk_error(
                    error_tok,
                    "core/task/not-found",
                    format!("unknown channel-id: {channel_id}"),
                    Some(op),
                ));
            };
            if let Some(value) = channel.queue.pop_front() {
                let state = if channel.closed {
                    Term::symbol(":closed")
                } else {
                    Term::symbol(":open")
                };
                Some(Value::Data(task_map([
                    (":channel-id", Term::Str(channel_id)),
                    (":has-value", Term::Bool(true)),
                    (":value", value),
                    (":size", Term::Int(BigInt::from(channel.queue.len()))),
                    (":state", state),
                ])))
            } else {
                Some(Value::Data(task_map([
                    (":channel-id", Term::Str(channel_id)),
                    (":has-value", Term::Bool(false)),
                    (":value", Term::Nil),
                    (":size", Term::Int(BigInt::from(0))),
                    (
                        ":state",
                        if channel.closed {
                            Term::symbol(":closed")
                        } else {
                            Term::symbol(":open")
                        },
                    ),
                ])))
            }
        }
        "core/task::channel-close" => {
            let Some(channel_id) = map_field_str_or_symbol(payload, ":channel-id") else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::channel-close payload must include :channel-id".to_string(),
                    Some(op),
                ));
            };
            let Some(channel) = runtime.channels.get_mut(&channel_id) else {
                return Some(mk_error(
                    error_tok,
                    "core/task/not-found",
                    format!("unknown channel-id: {channel_id}"),
                    Some(op),
                ));
            };
            channel.closed = true;
            Some(Value::Data(task_map([
                (":channel-id", Term::Str(channel_id)),
                (":size", Term::Int(BigInt::from(channel.queue.len()))),
                (":state", Term::symbol(":closed")),
            ])))
        }
        "core/task::channel-status" => {
            let Some(channel_id) = map_field_str_or_symbol(payload, ":channel-id") else {
                return Some(mk_error(
                    error_tok,
                    "core/task/bad-payload",
                    "core/task::channel-status payload must include :channel-id".to_string(),
                    Some(op),
                ));
            };
            let Some(channel) = runtime.channels.get(&channel_id) else {
                return Some(mk_error(
                    error_tok,
                    "core/task/not-found",
                    format!("unknown channel-id: {channel_id}"),
                    Some(op),
                ));
            };
            Some(Value::Data(task_map([
                (":channel-id", Term::Str(channel_id)),
                (":size", Term::Int(BigInt::from(channel.queue.len()))),
                (
                    ":capacity",
                    channel
                        .capacity
                        .map(|n| Term::Int(BigInt::from(n)))
                        .unwrap_or(Term::Nil),
                ),
                (
                    ":state",
                    if channel.closed {
                        Term::symbol(":closed")
                    } else {
                        Term::symbol(":open")
                    },
                ),
            ])))
        }
        "core/task::spawn" => {
            let task_id = format!("task-{:016x}", runtime.next_task_id);
            runtime.next_task_id = runtime.next_task_id.saturating_add(1);
            let parent_task = map_field_str_or_symbol(payload, ":parent-task")
                .or_else(|| map_field_str_or_symbol(payload, ":scope"));
            let task_payload = map_field(payload, ":payload").cloned().unwrap_or(Term::Nil);
            runtime.tasks.insert(
                task_id.clone(),
                TaskRecord {
                    state: TaskState::Queued,
                    payload: task_payload,
                    result: None,
                    error: None,
                    parent_task,
                    cancel_flag: None,
                },
            );
            runtime.queue.push_back(task_id.clone());
            runtime.queued_count = runtime.queued_count.saturating_add(1);
            promote_queued_task(runtime, policy, op);
            let state = runtime
                .tasks
                .get(&task_id)
                .map(|t| task_state_term(&t.state))
                .unwrap_or(Term::symbol(":failed"));
            Some(Value::Data(task_map([
                (":task-id", Term::Str(task_id)),
                (":state", state),
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
                runtime.queued_count = runtime.queued_count.saturating_sub(1);
            }
            if state_before == TaskState::Running
                && let Some(flag) = runtime
                    .tasks
                    .get(&task_id)
                    .and_then(|rec| rec.cancel_flag.clone())
            {
                flag.store(true, Ordering::Release);
            }
            if (state_before == TaskState::Running || state_before == TaskState::Queued)
                && let Some(rec) = runtime.tasks.get_mut(&task_id)
            {
                rec.state = TaskState::Cancelled;
                rec.result = None;
                rec.error = None;
            }
            if state_before == TaskState::Running {
                runtime.running_count = runtime.running_count.saturating_sub(1);
            }
            if state_before == TaskState::Queued {
                promote_queued_task(runtime, policy, op);
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
            match state_before {
                TaskState::Queued => {
                    runtime.queued_count = runtime.queued_count.saturating_sub(1);
                    let payload = runtime
                        .tasks
                        .get(&task_id)
                        .map(|r| r.payload.clone())
                        .unwrap_or(Term::Nil);
                    let cancel_flag = AtomicBool::new(false);
                    let completion = TaskCompletion {
                        task_id: task_id.clone(),
                        outcome: execute_task_payload(payload, &cancel_flag, policy),
                    };
                    apply_completion(runtime, completion);
                    promote_queued_task(runtime, policy, op);
                }
                TaskState::Running => {
                    let completion = match runtime.wait_for_completion(&task_id) {
                        Ok(c) => c,
                        Err(e) => {
                            return Some(mk_error(
                                error_tok,
                                "core/task/internal-state",
                                e,
                                Some(op),
                            ));
                        }
                    };
                    apply_completion(runtime, completion);
                    promote_queued_task(runtime, policy, op);
                }
                TaskState::Done | TaskState::Failed | TaskState::Cancelled => {}
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
                TaskState::Done => (rec.result.clone().unwrap_or(Term::Nil), Term::Nil),
                TaskState::Failed => (Term::Nil, rec.error.clone().unwrap_or(Term::Nil)),
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

fn apply_completion(runtime: &mut TaskRuntime, completion: TaskCompletion) {
    let Some(rec) = runtime.tasks.get_mut(&completion.task_id) else {
        return;
    };
    if rec.state == TaskState::Running {
        runtime.running_count = runtime.running_count.saturating_sub(1);
    }
    if rec.state == TaskState::Cancelled {
        rec.cancel_flag = None;
        return;
    }
    match completion.outcome {
        TaskOutcome::Done(result) => {
            rec.state = TaskState::Done;
            rec.result = Some(result);
            rec.error = None;
        }
        TaskOutcome::Failed(error) => {
            rec.state = TaskState::Failed;
            rec.result = None;
            rec.error = Some(error);
        }
        TaskOutcome::Cancelled => {
            rec.state = TaskState::Cancelled;
            rec.result = None;
            rec.error = None;
        }
    }
    rec.cancel_flag = None;
}

fn task_state_term(state: &TaskState) -> Term {
    match state {
        TaskState::Queued => Term::symbol(":queued"),
        TaskState::Running => Term::symbol(":running"),
        TaskState::Done => Term::symbol(":done"),
        TaskState::Failed => Term::symbol(":failed"),
        TaskState::Cancelled => Term::symbol(":cancelled"),
    }
}

fn promote_queued_task(runtime: &mut TaskRuntime, policy: &CapsPolicy, op: &str) {
    let worker_budget = task_worker_budget(policy);
    if worker_budget == 0 {
        return;
    }
    runtime.pool.ensure_started(worker_budget as usize);
    while runtime.running_count < worker_budget {
        let Some(task_id) = runtime.queue.pop_front() else {
            break;
        };
        let Some((is_queued, payload)) = runtime
            .tasks
            .get(&task_id)
            .map(|rec| (rec.state == TaskState::Queued, rec.payload.clone()))
        else {
            continue;
        };
        if !is_queued {
            continue;
        }
        runtime.queued_count = runtime.queued_count.saturating_sub(1);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let job = TaskJob {
            task_id: task_id.clone(),
            payload,
            cancel_flag: Arc::clone(&cancel_flag),
            policy: Arc::new(policy.clone()),
        };
        let dispatch_result = runtime.pool.dispatch(job);
        if let Some(rec) = runtime.tasks.get_mut(&task_id) {
            match dispatch_result {
                Ok(()) => {
                    rec.state = TaskState::Running;
                    rec.result = None;
                    rec.error = None;
                    rec.cancel_flag = Some(cancel_flag);
                    runtime.running_count = runtime.running_count.saturating_add(1);
                }
                Err(e) => {
                    rec.state = TaskState::Failed;
                    rec.result = None;
                    rec.error = Some(Term::Str(format!("{op}: {e}")));
                    rec.cancel_flag = None;
                }
            }
        }
    }
}

fn task_worker_budget(policy: &CapsPolicy) -> u64 {
    match policy.task.max_workers {
        Some(v) => v,
        None => policy.task.default_workers.max(1),
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
