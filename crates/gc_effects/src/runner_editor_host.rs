use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, canonicalize_module, hash_module, parse_module, print_module};
use gc_kernel::{SealId, Value};
use gc_opt::optimize_module_with_report;
use gc_pkg::PackageManifest;

use crate::policy::OpPolicy;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};
use crate::runner_io_ops::path_to_slash;

#[path = "runner_editor_watch.rs"]
mod runner_editor_watch;
use runner_editor_watch::{WatchState, watch_state_new, watch_state_poll};
#[path = "runner_editor_tasks.rs"]
mod runner_editor_tasks;
use runner_editor_tasks::{execute_editor_task, result_ok, task_diagnostic_error};

#[derive(Debug, Clone)]
struct TaskState {
    contract: Term,
    kind: String,
    next_partial_idx: usize,
    partials: Vec<Term>,
    state: String,
    result: Term,
}

#[derive(Debug)]
pub(crate) struct EditorHostRuntime {
    clipboard_mime: String,
    clipboard_data: Term,
    next_watch: u64,
    watches: BTreeMap<String, WatchState>,
    next_task: u64,
    tasks: BTreeMap<String, TaskState>,
}

impl Default for EditorHostRuntime {
    fn default() -> Self {
        Self {
            clipboard_mime: "text/plain".to_string(),
            clipboard_data: Term::Str(String::new()),
            next_watch: 0,
            watches: BTreeMap::new(),
            next_task: 0,
            tasks: BTreeMap::new(),
        }
    }
}

pub(crate) fn editor_host_call(
    runtime: &mut EditorHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_editor_host_op(op) {
        return None;
    }
    if is_first_party_editor_op(op) && !has_explicit_bridge_profile(pol) {
        return Some(Value::data(first_party_editor_response(
            runtime, op, payload,
        )));
    }
    Some(match call_host_bridge("editor", op, payload, pol) {
        Ok(resp) => Value::data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn has_explicit_bridge_profile(pol: Option<&OpPolicy>) -> bool {
    let Some(pol) = pol else {
        return false;
    };
    let has_nonempty_str = |key: &str| {
        pol.extra
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };
    has_nonempty_str("bridge_cmd")
        || has_nonempty_str("wasi_bridge_response")
        || has_nonempty_str("wasi_bridge_response_file")
        || pol
            .extra
            .get("wasi_bridge_profile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

fn is_first_party_editor_op(op: &str) -> bool {
    matches!(
        op,
        "editor/clipboard::get"
            | "editor/clipboard::set"
            | "editor/dialog::open"
            | "editor/dialog::save"
            | "editor/watch::subscribe"
            | "editor/watch::poll"
            | "editor/watch::unsubscribe"
            | "editor/task::spawn"
            | "editor/task::poll"
            | "editor/task::cancel"
            | "editor/task::fmt-coreform"
            | "editor/task::lint-module"
            | "editor/task::optimize-module"
            | "editor/task::parse-module"
            | "editor/task::test-pkg"
            | "editor/task::typecheck-pkg"
    )
}

fn first_party_editor_response(runtime: &mut EditorHostRuntime, op: &str, payload: &Term) -> Term {
    match op {
        "editor/clipboard::get" => first_party_clipboard_get(runtime),
        "editor/clipboard::set" => first_party_clipboard_set(runtime, payload),
        "editor/dialog::open" => first_party_dialog_open(payload),
        "editor/dialog::save" => first_party_dialog_save(payload),
        "editor/watch::subscribe" => first_party_watch_subscribe(runtime, payload),
        "editor/watch::poll" => first_party_watch_poll(runtime, payload),
        "editor/watch::unsubscribe" => first_party_watch_unsubscribe(runtime, payload),
        "editor/task::spawn"
        | "editor/task::fmt-coreform"
        | "editor/task::lint-module"
        | "editor/task::optimize-module"
        | "editor/task::parse-module"
        | "editor/task::test-pkg"
        | "editor/task::typecheck-pkg" => first_party_task_spawn(runtime, op, payload),
        "editor/task::poll" => first_party_task_poll(runtime, payload),
        "editor/task::cancel" => first_party_task_cancel(runtime, payload),
        _ => editor_error(op, "editor/first-party-unsupported-op"),
    }
}

fn first_party_clipboard_get(runtime: &EditorHostRuntime) -> Term {
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":mime", Term::Str(runtime.clipboard_mime.clone())),
        (":data", runtime.clipboard_data.clone()),
    ])
}

fn first_party_clipboard_set(runtime: &mut EditorHostRuntime, payload: &Term) -> Term {
    let Some(m) = payload_map(payload) else {
        return editor_error(
            "editor/clipboard::set",
            "editor/first-party-invalid-payload",
        );
    };
    let mime = map_get_string(m, ":mime").unwrap_or_else(|| "text/plain".to_string());
    let data = m
        .get(&TermOrdKey(Term::symbol(":data")))
        .cloned()
        .or_else(|| map_get_string(m, ":value").map(Term::Str))
        .or_else(|| map_get_string(m, ":text").map(Term::Str))
        .unwrap_or(Term::Str(String::new()));
    runtime.clipboard_mime = mime.clone();
    runtime.clipboard_data = data;
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":mime", Term::Str(mime)),
    ])
}

fn first_party_dialog_open(payload: &Term) -> Term {
    if payload_bool(payload, ":cancel") {
        return map_term(vec![
            (":ok", Term::Bool(false)),
            (":backend", Term::Str("first-party-runtime".to_string())),
        ]);
    }
    let Some(path) = dialog_selected_path(payload, false) else {
        return map_term(vec![
            (":ok", Term::Bool(false)),
            (":backend", Term::Str("first-party-runtime".to_string())),
        ]);
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":paths", Term::Vector(vec![Term::Str(path)])),
    ])
}

fn first_party_dialog_save(payload: &Term) -> Term {
    if payload_bool(payload, ":cancel") {
        return map_term(vec![
            (":ok", Term::Bool(false)),
            (":backend", Term::Str("first-party-runtime".to_string())),
        ]);
    }
    let path = dialog_selected_path(payload, true).unwrap_or_else(|| "saved.gc".to_string());
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":path", Term::Str(path)),
    ])
}

fn dialog_selected_path(payload: &Term, save_mode: bool) -> Option<String> {
    let m = payload_map(payload)?;
    let selected = map_get_string(m, ":path")
        .or_else(|| map_get_string(m, ":default"))
        .or_else(|| map_get_string(m, ":default-name"))
        .or_else(|| {
            if save_mode {
                Some("saved.gc".to_string())
            } else {
                Some("opened.gc".to_string())
            }
        })?;
    if let Some(start_dir) = map_get_string(m, ":start-dir") {
        let base = Path::new(start_dir.trim());
        if base.as_os_str().is_empty() {
            return Some(selected);
        }
        return Some(path_to_slash(&base.join(selected)));
    }
    Some(selected)
}

fn first_party_watch_subscribe(runtime: &mut EditorHostRuntime, payload: &Term) -> Term {
    let Some(m) = payload_map(payload) else {
        return editor_error(
            "editor/watch::subscribe",
            "editor/first-party-invalid-payload",
        );
    };
    let root = map_get_string(m, ":root").unwrap_or_else(|| ".".to_string());
    let globs = map_get_string_vec(m, ":globs").unwrap_or_else(|| vec!["*".to_string()]);
    let watch_state = match watch_state_new(&root, globs.clone()) {
        Ok(state) => state,
        Err(msg) => {
            return task_diagnostic_error(
                "editor/watch::subscribe",
                "editor/first-party-watch-root-invalid",
                &root,
                msg,
            );
        }
    };
    runtime.next_watch = runtime.next_watch.saturating_add(1);
    let watch_id = format!("watch-first-party-{}", runtime.next_watch);
    runtime.watches.insert(watch_id.clone(), watch_state);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":watch-id", Term::Str(watch_id)),
        (":root", Term::Str(root)),
        (
            ":globs",
            Term::Vector(globs.into_iter().map(Term::Str).collect::<Vec<_>>()),
        ),
    ])
}

fn first_party_watch_poll(runtime: &mut EditorHostRuntime, payload: &Term) -> Term {
    let Some(m) = payload_map(payload) else {
        return editor_error("editor/watch::poll", "editor/first-party-invalid-payload");
    };
    let Some(watch_id) = map_get_string(m, ":watch-id") else {
        return editor_error("editor/watch::poll", "editor/first-party-missing-watch-id");
    };
    let Some(watch) = runtime.watches.get_mut(&watch_id) else {
        return editor_error("editor/watch::poll", "editor/first-party-watch-not-found");
    };
    let events = match watch_state_poll(watch) {
        Ok(events) => events,
        Err(msg) => {
            return task_diagnostic_error(
                "editor/watch::poll",
                "editor/first-party-watch-root-invalid",
                watch.root(),
                msg,
            );
        }
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":watch-id", Term::Str(watch_id)),
        (":events", Term::Vector(events)),
    ])
}

fn first_party_watch_unsubscribe(runtime: &mut EditorHostRuntime, payload: &Term) -> Term {
    let Some(m) = payload_map(payload) else {
        return editor_error(
            "editor/watch::unsubscribe",
            "editor/first-party-invalid-payload",
        );
    };
    let Some(watch_id) = map_get_string(m, ":watch-id") else {
        return editor_error(
            "editor/watch::unsubscribe",
            "editor/first-party-missing-watch-id",
        );
    };
    runtime.watches.remove(&watch_id);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":watch-id", Term::Str(watch_id)),
    ])
}

fn first_party_task_spawn(runtime: &mut EditorHostRuntime, op: &str, payload: &Term) -> Term {
    let task_kind = requested_task_kind(op, payload);
    let task_input = requested_task_input(op, payload);
    let execution = execute_editor_task(&task_kind, &task_input);
    let partial_count = execution.partials.len();
    let result = execution.result;
    let state = if !execution.partials.is_empty() {
        ":running".to_string()
    } else if result_ok(&result) {
        ":done".to_string()
    } else {
        ":failed".to_string()
    };
    runtime.next_task = runtime.next_task.saturating_add(1);
    let task_id = format!("task-first-party-{}", runtime.next_task);
    runtime.tasks.insert(
        task_id.clone(),
        TaskState {
            contract: execution.contract.clone(),
            kind: task_kind.clone(),
            next_partial_idx: 0,
            partials: execution.partials,
            state: state.clone(),
            result,
        },
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":task-id", Term::Str(task_id)),
        (":task-kind", Term::Str(task_kind)),
        (":task-contract", execution.contract),
        (":partial-count", Term::Int((partial_count as i64).into())),
        (":state", Term::symbol(state)),
    ])
}

fn first_party_task_poll(runtime: &mut EditorHostRuntime, payload: &Term) -> Term {
    let Some(m) = payload_map(payload) else {
        return editor_error("editor/task::poll", "editor/first-party-invalid-payload");
    };
    let Some(task_id) = map_get_string(m, ":task-id") else {
        return editor_error("editor/task::poll", "editor/first-party-missing-task-id");
    };
    let Some(task) = runtime.tasks.get_mut(&task_id) else {
        return editor_error("editor/task::poll", "editor/first-party-task-not-found");
    };
    let mut partial = Term::Nil;
    let mut partial_emitted = false;
    if task.state == ":running" {
        if let Some(next_partial) = task.partials.get(task.next_partial_idx).cloned() {
            task.next_partial_idx = task.next_partial_idx.saturating_add(1);
            partial = next_partial;
            partial_emitted = true;
        }
        if task.next_partial_idx >= task.partials.len() {
            task.state = if result_ok(&task.result) {
                ":done".to_string()
            } else {
                ":failed".to_string()
            };
        }
    }
    let result = if task.state == ":running" {
        Term::Nil
    } else {
        task.result.clone()
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":task-id", Term::Str(task_id)),
        (":task-kind", Term::Str(task.kind.clone())),
        (":state", Term::symbol(task.state.clone())),
        (":task-contract", task.contract.clone()),
        (":partial", partial),
        (":partial-emitted", Term::Bool(partial_emitted)),
        (
            ":partial-seq",
            Term::Int((task.next_partial_idx as i64).into()),
        ),
        (
            ":partial-total",
            Term::Int((task.partials.len() as i64).into()),
        ),
        (":result", result),
    ])
}

fn first_party_task_cancel(runtime: &mut EditorHostRuntime, payload: &Term) -> Term {
    let Some(m) = payload_map(payload) else {
        return editor_error("editor/task::cancel", "editor/first-party-invalid-payload");
    };
    let Some(task_id) = map_get_string(m, ":task-id") else {
        return editor_error("editor/task::cancel", "editor/first-party-missing-task-id");
    };
    runtime.tasks.remove(&task_id);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":task-id", Term::Str(task_id)),
        (":state", Term::symbol(":canceled")),
    ])
}

fn requested_task_kind(op: &str, payload: &Term) -> String {
    if op == "editor/task::spawn" {
        return payload_map(payload)
            .and_then(|m| map_get_string(m, ":task-kind"))
            .unwrap_or_else(|| "editor/task::spawn".to_string());
    }
    op.to_string()
}

fn requested_task_input(op: &str, payload: &Term) -> Term {
    if op == "editor/task::spawn" {
        return payload_map(payload)
            .and_then(|m| m.get(&TermOrdKey(Term::symbol(":input"))))
            .cloned()
            .unwrap_or(Term::Nil);
    }
    payload.clone()
}

fn payload_bool(payload: &Term, key: &str) -> bool {
    payload_map(payload)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(key))))
        .and_then(|t| match t {
            Term::Bool(v) => Some(*v),
            _ => None,
        })
        .unwrap_or(false)
}

fn editor_error(op: &str, code: &str) -> Term {
    map_term(vec![
        (":ok", Term::Bool(false)),
        (":error/code", Term::Str(code.to_string())),
        (":error/op", Term::symbol(op)),
    ])
}

fn payload_map(payload: &Term) -> Option<&BTreeMap<TermOrdKey, Term>> {
    match payload {
        Term::Map(m) => Some(m),
        _ => None,
    }
}

fn map_get_string(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.clone()),
            Term::Symbol(s) => Some(s.clone()),
            _ => None,
        })
}

fn map_get_string_vec(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<Vec<String>> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Vector(xs) => Some(
                xs.iter()
                    .filter_map(|x| match x {
                        Term::Str(s) => Some(s.clone()),
                        Term::Symbol(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
}

fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}

fn is_editor_host_op(op: &str) -> bool {
    matches!(
        op,
        "editor/clipboard::get"
            | "editor/clipboard::set"
            | "editor/dialog::open"
            | "editor/dialog::save"
            | "editor/watch::subscribe"
            | "editor/watch::poll"
            | "editor/watch::unsubscribe"
            | "editor/task::spawn"
            | "editor/task::poll"
            | "editor/task::cancel"
            | "editor/task::fmt-coreform"
            | "editor/task::lint-module"
            | "editor/task::optimize-module"
            | "editor/task::parse-module"
            | "editor/task::test-pkg"
            | "editor/task::typecheck-pkg"
    )
}

fn mk_error(error_tok: SealId, err: &BridgeError, op: Option<&str>) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(err.code.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/message")),
        Term::Str(err.message.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::data(Term::Map(mm))),
    }
}
