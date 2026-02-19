use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

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

#[derive(Debug, Default)]
pub(crate) struct TaskRuntime {
    next_task_id: u64,
    tasks: BTreeMap<String, TaskRecord>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug)]
struct TaskWorkerPool {
    tx: Option<mpsc::Sender<TaskJob>>,
    rx: Option<mpsc::Receiver<TaskCompletion>>,
    workers: Vec<JoinHandle<()>>,
}

impl Default for TaskWorkerPool {
    fn default() -> Self {
        Self {
            tx: None,
            rx: None,
            workers: Vec::new(),
        }
    }
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
}

#[derive(Debug, Clone)]
struct TaskCompletion {
    task_id: String,
    outcome: TaskOutcome,
}

#[derive(Debug, Clone)]
enum TaskOutcome {
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
                        outcome: execute_task_payload(payload, &cancel_flag),
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

impl TaskRuntime {
    fn wait_for_completion(&mut self, task_id: &str) -> Result<TaskCompletion, String> {
        if let Some(c) = self.completed.remove(task_id) {
            return Ok(c);
        }
        self.pool.wait_for_completion(task_id, &mut self.completed)
    }
}

impl TaskWorkerPool {
    fn ensure_started(&mut self, worker_count: usize) {
        if self.tx.is_some() || worker_count == 0 {
            return;
        }
        let (tx, rx) = mpsc::channel::<TaskJob>();
        let (done_tx, done_rx) = mpsc::channel::<TaskCompletion>();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        let shared_rx = Arc::new(Mutex::new(rx));
        let mut started_workers = 0usize;
        for idx in 0..worker_count {
            let shared_rx = Arc::clone(&shared_rx);
            let done_tx = done_tx.clone();
            let ready_tx = ready_tx.clone();
            let worker_result = thread::Builder::new()
                .name(format!("genesis-task-{idx}"))
                .spawn(move || worker_loop(shared_rx, done_tx, ready_tx));
            match worker_result {
                Ok(worker) => {
                    self.workers.push(worker);
                    started_workers = started_workers.saturating_add(1);
                }
                Err(_) => break,
            }
        }
        if started_workers == 0 {
            return;
        }
        for _ in 0..started_workers {
            let _ = ready_rx.recv_timeout(Duration::from_secs(1));
        }
        self.tx = Some(tx);
        self.rx = Some(done_rx);
    }

    fn dispatch(&self, job: TaskJob) -> Result<(), String> {
        match &self.tx {
            Some(tx) => tx
                .send(job)
                .map_err(|e| format!("task dispatch failed: {e}")),
            None => Err("task worker pool not initialized".to_string()),
        }
    }

    fn wait_for_completion(
        &mut self,
        task_id: &str,
        completed: &mut BTreeMap<String, TaskCompletion>,
    ) -> Result<TaskCompletion, String> {
        if let Some(c) = completed.remove(task_id) {
            return Ok(c);
        }
        let rx = self
            .rx
            .as_ref()
            .ok_or_else(|| "task worker completion channel not initialized".to_string())?;
        loop {
            let completion = rx
                .recv()
                .map_err(|e| format!("task completion channel closed: {e}"))?;
            if completion.task_id == task_id {
                return Ok(completion);
            }
            completed.insert(completion.task_id.clone(), completion);
        }
    }
}

fn worker_loop(
    shared_rx: Arc<Mutex<mpsc::Receiver<TaskJob>>>,
    done_tx: mpsc::Sender<TaskCompletion>,
    ready_tx: mpsc::Sender<()>,
) {
    let _ = ready_tx.send(());
    loop {
        let recv = match shared_rx.lock() {
            Ok(rx) => rx.recv(),
            Err(poisoned) => poisoned.into_inner().recv(),
        };
        let job = match recv {
            Ok(job) => job,
            Err(_) => break,
        };
        let outcome = execute_task_payload(job.payload, &job.cancel_flag);
        let _ = done_tx.send(TaskCompletion {
            task_id: job.task_id,
            outcome,
        });
    }
}

fn execute_task_payload(payload: Term, cancel_flag: &AtomicBool) -> TaskOutcome {
    if cancel_flag.load(Ordering::Acquire) {
        return TaskOutcome::Cancelled;
    }
    if let Some(ms) = map_field_int_u64(&payload, ":task/sleep-ms")
        .or_else(|| map_field_int_u64(&payload, ":sleep-ms"))
    {
        let mut remaining = ms;
        while remaining > 0 {
            if cancel_flag.load(Ordering::Acquire) {
                return TaskOutcome::Cancelled;
            }
            let chunk = remaining.min(10);
            thread::sleep(Duration::from_millis(chunk));
            remaining = remaining.saturating_sub(chunk);
        }
    }
    if cancel_flag.load(Ordering::Acquire) {
        return TaskOutcome::Cancelled;
    }
    if let Some(err) = map_field(&payload, ":task/error") {
        return TaskOutcome::Failed(err.clone());
    }
    if let Some(result) = map_field(&payload, ":task/result") {
        return TaskOutcome::Done(result.clone());
    }
    TaskOutcome::Done(payload)
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
        TaskState::Failed => Term::symbol(":failed"),
        TaskState::Cancelled => Term::symbol(":cancelled"),
    }
}

fn runtime_running_count(runtime: &TaskRuntime) -> u64 {
    runtime.running_count
}

fn runtime_queue_count(runtime: &TaskRuntime) -> u64 {
    runtime.queued_count
}

fn promote_queued_task(runtime: &mut TaskRuntime, policy: &CapsPolicy, op: &str) {
    let worker_budget = task_worker_budget(policy);
    if worker_budget == 0 {
        return;
    }
    runtime.pool.ensure_started(worker_budget as usize);
    while runtime_running_count(runtime) < worker_budget {
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

fn map_field_int_u64(t: &Term, key: &str) -> Option<u64> {
    match map_field(t, key) {
        Some(Term::Int(i)) if i.sign() != num_bigint::Sign::Minus => i.to_u64(),
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
