use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_bigint::BigInt;

use super::runner_task_terms::value_data_map_field;
use super::*;
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
        "core/task::channel-open" => {
            out.task_id = value_data_map_field(resp_val, ":channel-id");
        }
        "core/task::channel-send"
        | "core/task::channel-recv"
        | "core/task::channel-close"
        | "core/task::channel-status" => {
            out.task_id = map_field_str_or_symbol(payload, ":channel-id");
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

#[expect(
    clippy::too_many_arguments,
    reason = "task policy limiter receives explicit runtime and protocol context"
)]
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

fn runtime_running_count(runtime: &TaskRuntime) -> u64 {
    runtime.running_count
}

fn runtime_queue_count(runtime: &TaskRuntime) -> u64 {
    runtime.queued_count
}

fn task_target_id(op: &str, payload: &Term, resp_val: &Value) -> Option<String> {
    match op {
        "core/task::spawn" | "editor/task::spawn" => value_data_map_field(resp_val, ":task-id"),
        "core/task::channel-open" => value_data_map_field(resp_val, ":channel-id"),
        "core/task::channel-send"
        | "core/task::channel-recv"
        | "core/task::channel-close"
        | "core/task::channel-status" => map_field_str_or_symbol(payload, ":channel-id"),
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
