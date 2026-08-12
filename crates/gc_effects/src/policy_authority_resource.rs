use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey};
use num_traits::ToPrimitive;

use crate::error::EffectsError;

use super::super::{CapsPolicy, RuntimePolicy, TaskPolicy};
use super::{authority_error, hex32};

pub(super) struct AuthorizedResources {
    pub(super) task: TaskPolicy,
    pub(super) runtime: RuntimePolicy,
    pub(super) log_inline_max_bytes: Option<usize>,
    pub(super) log_max_artifact_bytes_per_run: Option<usize>,
    pub(super) store_max_run_bytes: Option<usize>,
    pub(super) policy_term: Term,
}

fn optional_table_int(table: Option<&toml::value::Table>, key: &str) -> Term {
    table
        .and_then(|values| values.get(key))
        .and_then(toml::Value::as_integer)
        .map(|number| Term::Int(number.into()))
        .unwrap_or(Term::Nil)
}

pub(super) fn request_term(document: &toml::value::Table) -> Term {
    let log = document.get("log").and_then(toml::Value::as_table);
    let runtime = document.get("runtime").and_then(toml::Value::as_table);
    let store = document.get("store").and_then(toml::Value::as_table);
    let task = document.get("task").and_then(toml::Value::as_table);
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":available-workers")),
                Term::Int(super::super::adaptive_default_task_workers().into()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/effect-resource-policy-request-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":log")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":inline-max-bytes")),
                            optional_table_int(log, "inline_max_bytes"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-artifact-bytes-per-run")),
                            optional_table_int(log, "max_artifact_bytes_per_run"),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":runtime")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":max-effect-ops")),
                            optional_table_int(runtime, "max_effect_ops"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-payload-bytes-per-op")),
                            optional_table_int(runtime, "max_payload_bytes_per_op"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-payload-bytes-per-run")),
                            optional_table_int(runtime, "max_payload_bytes_per_run"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-response-bytes-per-op")),
                            optional_table_int(runtime, "max_response_bytes_per_op"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-response-bytes-per-run")),
                            optional_table_int(runtime, "max_response_bytes_per_run"),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":store")),
                Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":max-run-bytes")),
                        optional_table_int(store, "max_run_bytes"),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":task")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":default-workers")),
                            optional_table_int(task, "default_workers"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-queue")),
                            optional_table_int(task, "max_queue"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-steps-per-task")),
                            optional_table_int(task, "max_steps_per_task"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-tasks")),
                            optional_table_int(task, "max_tasks"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-time-ms-per-task")),
                            optional_table_int(task, "max_time_ms_per_task"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-workers")),
                            optional_table_int(task, "max_workers"),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(2.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn exact_map<'a>(
    term: &'a Term,
    keys: &[&str],
    scope: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, EffectsError> {
    let Term::Map(map) = term else {
        return Err(authority_error(format!("{scope} must be a data map")));
    };
    let expected: BTreeSet<_> = keys
        .iter()
        .map(|key| TermOrdKey(Term::symbol(*key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(format!("{scope} field set mismatch")));
    }
    Ok(map)
}

fn map_field<'a>(map: &'a BTreeMap<TermOrdKey, Term>, key: &str) -> Result<&'a Term, EffectsError> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| authority_error(format!("resource result is missing {key}")))
}

fn optional_u64_field(
    map: &BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Result<Option<u64>, EffectsError> {
    match map_field(map, key)? {
        Term::Nil => Ok(None),
        Term::Int(value) => value.to_u64().map(Some).ok_or_else(|| {
            authority_error(format!("resource result {key} must fit a nonnegative u64"))
        }),
        _ => Err(authority_error(format!(
            "resource result {key} must be nil or an integer"
        ))),
    }
}

fn optional_usize_field(
    map: &BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Result<Option<usize>, EffectsError> {
    match map_field(map, key)? {
        Term::Nil => Ok(None),
        Term::Int(value) => value.to_usize().map(Some).ok_or_else(|| {
            authority_error(format!(
                "resource result {key} must fit a nonnegative platform usize"
            ))
        }),
        _ => Err(authority_error(format!(
            "resource result {key} must be nil or an integer"
        ))),
    }
}

fn optional_positive_usize_field(
    map: &BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Result<Option<usize>, EffectsError> {
    let value = optional_usize_field(map, key)?;
    if value == Some(0) {
        return Err(authority_error(format!(
            "resource result {key} must be nil or a positive platform usize"
        )));
    }
    Ok(value)
}

fn optional_u64_term(value: Option<u64>) -> Term {
    value
        .map(|number| Term::Int(number.into()))
        .unwrap_or(Term::Nil)
}

fn optional_usize_term(value: Option<usize>) -> Term {
    value
        .map(|number| Term::Int(number.into()))
        .unwrap_or(Term::Nil)
}

fn policy_term(
    task: &TaskPolicy,
    runtime: &RuntimePolicy,
    log_inline_max_bytes: Option<usize>,
    log_max_artifact_bytes_per_run: Option<usize>,
    store_max_run_bytes: Option<usize>,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":log")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":inline-max-bytes")),
                            optional_usize_term(log_inline_max_bytes),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-artifact-bytes-per-run")),
                            optional_usize_term(log_max_artifact_bytes_per_run),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":runtime")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":max-effect-ops")),
                            optional_u64_term(runtime.max_effect_ops),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-payload-bytes-per-op")),
                            optional_usize_term(runtime.max_payload_bytes_per_op),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-payload-bytes-per-run")),
                            optional_usize_term(runtime.max_payload_bytes_per_run),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-response-bytes-per-op")),
                            optional_usize_term(runtime.max_response_bytes_per_op),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-response-bytes-per-run")),
                            optional_usize_term(runtime.max_response_bytes_per_run),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":store")),
                Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":max-run-bytes")),
                        optional_usize_term(store_max_run_bytes),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":task")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":default-workers")),
                            Term::Int(task.default_workers.into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-queue")),
                            optional_u64_term(task.max_queue),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-steps-per-task")),
                            optional_u64_term(task.max_steps_per_task),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-tasks")),
                            optional_u64_term(task.max_tasks),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-time-ms-per-task")),
                            optional_u64_term(task.max_time_ms_per_task),
                        ),
                        (
                            TermOrdKey(Term::symbol(":max-workers")),
                            optional_u64_term(task.max_workers),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn legacy_policy_term(policy: &CapsPolicy) -> Term {
    policy_term(
        &policy.task,
        &policy.runtime,
        policy.log.inline_max_bytes,
        policy.log.max_artifact_bytes_per_run,
        policy.store.max_run_bytes,
    )
}

pub(super) fn decode_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<AuthorizedResources, EffectsError> {
    let map = exact_map(
        &term,
        &[
            ":kind",
            ":log",
            ":request-h",
            ":runtime",
            ":store",
            ":task",
            ":v",
        ],
        "resource result",
    )?;
    if !matches!(map_field(map, ":kind")?, Term::Str(kind) if kind == "genesis/effect-resource-policy-result-v0.2")
        || !matches!(map_field(map, ":v")?, Term::Int(version) if version == &2.into())
        || !matches!(map_field(map, ":request-h")?, Term::Str(actual) if actual == &hex32(request_hash))
    {
        return Err(authority_error("resource result identity mismatch"));
    }

    let log_map = exact_map(
        map_field(map, ":log")?,
        &[":inline-max-bytes", ":max-artifact-bytes-per-run"],
        "resource result :log",
    )?;
    let runtime_map = exact_map(
        map_field(map, ":runtime")?,
        &[
            ":max-effect-ops",
            ":max-payload-bytes-per-op",
            ":max-payload-bytes-per-run",
            ":max-response-bytes-per-op",
            ":max-response-bytes-per-run",
        ],
        "resource result :runtime",
    )?;
    let store_map = exact_map(
        map_field(map, ":store")?,
        &[":max-run-bytes"],
        "resource result :store",
    )?;
    let task_map = exact_map(
        map_field(map, ":task")?,
        &[
            ":default-workers",
            ":max-queue",
            ":max-steps-per-task",
            ":max-tasks",
            ":max-time-ms-per-task",
            ":max-workers",
        ],
        "resource result :task",
    )?;

    let default_workers = optional_u64_field(task_map, ":default-workers")?
        .filter(|workers| *workers > 0)
        .ok_or_else(|| authority_error("resource result :default-workers must be >= 1"))?;
    let task = TaskPolicy {
        default_workers,
        max_tasks: optional_u64_field(task_map, ":max-tasks")?,
        max_workers: optional_u64_field(task_map, ":max-workers")?,
        max_queue: optional_u64_field(task_map, ":max-queue")?,
        max_steps_per_task: optional_u64_field(task_map, ":max-steps-per-task")?,
        max_time_ms_per_task: optional_u64_field(task_map, ":max-time-ms-per-task")?,
    };
    let runtime = RuntimePolicy {
        max_effect_ops: optional_u64_field(runtime_map, ":max-effect-ops")?,
        max_payload_bytes_per_op: optional_usize_field(runtime_map, ":max-payload-bytes-per-op")?,
        max_payload_bytes_per_run: optional_usize_field(runtime_map, ":max-payload-bytes-per-run")?,
        max_response_bytes_per_op: optional_usize_field(runtime_map, ":max-response-bytes-per-op")?,
        max_response_bytes_per_run: optional_usize_field(
            runtime_map,
            ":max-response-bytes-per-run",
        )?,
    };
    let log_inline_max_bytes = optional_positive_usize_field(log_map, ":inline-max-bytes")?;
    let log_max_artifact_bytes_per_run =
        optional_positive_usize_field(log_map, ":max-artifact-bytes-per-run")?;
    let store_max_run_bytes = optional_positive_usize_field(store_map, ":max-run-bytes")?;
    let policy_term = policy_term(
        &task,
        &runtime,
        log_inline_max_bytes,
        log_max_artifact_bytes_per_run,
        store_max_run_bytes,
    );
    Ok(AuthorizedResources {
        task,
        runtime,
        log_inline_max_bytes,
        log_max_artifact_bytes_per_run,
        store_max_run_bytes,
        policy_term,
    })
}
