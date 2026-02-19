use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, hash_module, parse_module, print_module};
use gc_kernel::{SealId, Value};
use num_bigint::BigInt;

use crate::policy::OpPolicy;
use crate::runner_io_ops::{
    atomic_write_text, effective_base_dir, payload_path, read_file_with_optional_limit,
    sandbox_path_read, sandbox_path_write,
};

#[derive(Debug, Clone)]
pub(crate) struct EditorHostRuntime {
    clipboard_mime: String,
    clipboard_data: Term,
    next_task_id: u64,
    tasks: BTreeMap<String, EditorTaskRecord>,
    next_watch_id: u64,
    watches: BTreeMap<String, WatchState>,
}

impl Default for EditorHostRuntime {
    fn default() -> Self {
        Self {
            clipboard_mime: "text/plain".to_string(),
            clipboard_data: Term::Nil,
            next_task_id: 0,
            tasks: BTreeMap::new(),
            next_watch_id: 0,
            watches: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct EditorTaskRecord {
    state: EditorTaskState,
    kind: String,
    input: Term,
    result: Option<Term>,
    error: Option<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditorTaskState {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
struct WatchState {
    root: String,
    globs: Vec<String>,
    seq: u64,
}

pub(crate) fn editor_host_call(
    runtime: &mut EditorHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    match op {
        "editor/clipboard::get" => Some(clipboard_get(runtime)),
        "editor/clipboard::set" => Some(clipboard_set(runtime, payload)),
        "editor/dialog::open" => Some(dialog_open(payload)),
        "editor/dialog::save" => Some(dialog_save(payload)),
        "editor/plugin::command" => Some(plugin_command(payload)),
        "editor/watch::subscribe" => Some(watch_subscribe(runtime, payload, error_tok, op)),
        "editor/watch::poll" => Some(watch_poll(runtime, payload, error_tok, op)),
        "editor/watch::unsubscribe" => Some(watch_unsubscribe(runtime, payload, error_tok, op)),
        "editor/task::spawn" => Some(task_spawn(runtime, payload, error_tok, op)),
        "editor/task::poll" => Some(task_poll(runtime, payload, pol, error_tok, op)),
        "editor/task::cancel" => Some(task_cancel(runtime, payload, error_tok, op)),
        "editor/task::fmt-coreform"
        | "editor/task::lint-module"
        | "editor/task::optimize-module"
        | "editor/task::parse-module"
        | "editor/task::test-pkg"
        | "editor/task::typecheck-pkg" => Some(task_direct(op, payload, pol, error_tok)),
        _ => None,
    }
}

fn clipboard_get(runtime: &EditorHostRuntime) -> Value {
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":mime", Term::Str(default_clipboard_mime(runtime))),
        (":data", runtime.clipboard_data.clone()),
    ]))
}

fn clipboard_set(runtime: &mut EditorHostRuntime, payload: &Term) -> Value {
    runtime.clipboard_mime = map_field_str_or_symbol(payload, ":mime")
        .unwrap_or_else(|| default_clipboard_mime(runtime));
    runtime.clipboard_data = map_field(payload, ":data").cloned().unwrap_or(Term::Nil);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":mime", Term::Str(runtime.clipboard_mime.clone())),
    ]))
}

fn dialog_open(payload: &Term) -> Value {
    let start_dir =
        map_field_str_or_symbol(payload, ":start-dir").unwrap_or_else(|| ".".to_string());
    let multi = map_field_bool(payload, ":multi").unwrap_or(false);
    let suffix = if multi {
        "selected-0.gc"
    } else {
        "selected.gc"
    };
    let path = format!("{}/{}", start_dir.trim_end_matches('/'), suffix);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":paths", Term::Vector(vec![Term::Str(path)])),
    ]))
}

fn dialog_save(payload: &Term) -> Value {
    let start_dir =
        map_field_str_or_symbol(payload, ":start-dir").unwrap_or_else(|| ".".to_string());
    let default_name = map_field_str_or_symbol(payload, ":default-name")
        .unwrap_or_else(|| "untitled.gc".to_string());
    let path = format!("{}/{}", start_dir.trim_end_matches('/'), default_name);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":path", Term::Str(path)),
    ]))
}

fn plugin_command(payload: &Term) -> Value {
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (
            ":plugin",
            map_field(payload, ":plugin").cloned().unwrap_or(Term::Nil),
        ),
        (
            ":command",
            map_field(payload, ":command").cloned().unwrap_or(Term::Nil),
        ),
        (
            ":result",
            map_field(payload, ":payload").cloned().unwrap_or(Term::Nil),
        ),
    ]))
}

fn watch_subscribe(
    runtime: &mut EditorHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(root) = map_field_str_or_symbol(payload, ":root") else {
        return mk_error(
            error_tok,
            "editor/watch/bad-payload",
            "editor/watch::subscribe payload must include :root".to_string(),
            Some(op),
        );
    };
    let globs = map_field(payload, ":globs")
        .and_then(term_vec_str)
        .unwrap_or_default();
    let watch_id = format!("watch-{:016x}", runtime.next_watch_id);
    runtime.next_watch_id = runtime.next_watch_id.saturating_add(1);
    runtime.watches.insert(
        watch_id.clone(),
        WatchState {
            root,
            globs,
            seq: 0,
        },
    );
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":watch-id", Term::Str(watch_id)),
    ]))
}

fn watch_poll(
    runtime: &mut EditorHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(watch_id) = map_field_str_or_symbol(payload, ":watch-id") else {
        return mk_error(
            error_tok,
            "editor/watch/bad-payload",
            "editor/watch::poll payload must include :watch-id".to_string(),
            Some(op),
        );
    };
    let Some(watch) = runtime.watches.get_mut(&watch_id) else {
        return mk_error(
            error_tok,
            "editor/watch/not-found",
            format!("unknown watch-id: {watch_id}"),
            Some(op),
        );
    };
    watch.seq = watch.seq.saturating_add(1);
    let event = map_term([
        (":kind", Term::Symbol(":heartbeat".to_string())),
        (":path", Term::Str(watch.root.clone())),
        (":stamp", Term::Int(BigInt::from(watch.seq))),
        (
            ":globs",
            Term::Vector(watch.globs.iter().cloned().map(Term::Str).collect()),
        ),
    ]);
    Value::Data(map_term([(":events", Term::Vector(vec![event]))]))
}

fn watch_unsubscribe(
    runtime: &mut EditorHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(watch_id) = map_field_str_or_symbol(payload, ":watch-id") else {
        return mk_error(
            error_tok,
            "editor/watch/bad-payload",
            "editor/watch::unsubscribe payload must include :watch-id".to_string(),
            Some(op),
        );
    };
    if runtime.watches.remove(&watch_id).is_none() {
        return mk_error(
            error_tok,
            "editor/watch/not-found",
            format!("unknown watch-id: {watch_id}"),
            Some(op),
        );
    }
    Value::Data(map_term([(":ok", Term::Bool(true))]))
}

fn task_spawn(
    runtime: &mut EditorHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(task_kind) = map_field_str_or_symbol(payload, ":task-kind") else {
        return mk_error(
            error_tok,
            "editor/task/bad-payload",
            "editor/task::spawn payload must include :task-kind".to_string(),
            Some(op),
        );
    };
    let input = map_field(payload, ":input").cloned().unwrap_or(Term::Nil);
    let task_id = format!("editor-task-{:016x}", runtime.next_task_id);
    runtime.next_task_id = runtime.next_task_id.saturating_add(1);
    runtime.tasks.insert(
        task_id.clone(),
        EditorTaskRecord {
            state: EditorTaskState::Running,
            kind: task_kind,
            input,
            result: None,
            error: None,
        },
    );
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":task-id", Term::Str(task_id)),
    ]))
}

fn task_poll(
    runtime: &mut EditorHostRuntime,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(task_id) = map_field_str_or_symbol(payload, ":task-id") else {
        return mk_error(
            error_tok,
            "editor/task/bad-payload",
            "editor/task::poll payload must include :task-id".to_string(),
            Some(op),
        );
    };
    let Some(task) = runtime.tasks.get_mut(&task_id) else {
        return mk_error(
            error_tok,
            "editor/task/not-found",
            format!("unknown task-id: {task_id}"),
            Some(op),
        );
    };
    if task.state == EditorTaskState::Running {
        match run_editor_task_kind(&task.kind, &task.input, pol) {
            Ok(v) => {
                task.state = EditorTaskState::Done;
                task.result = Some(v);
                task.error = None;
            }
            Err(e) => {
                task.state = EditorTaskState::Failed;
                task.result = None;
                task.error = Some(e);
            }
        }
    }
    Value::Data(map_term([
        (":task-id", Term::Str(task_id)),
        (":state", task_state_term(&task.state)),
        (":result", task.result.clone().unwrap_or(Term::Nil)),
        (":error", task.error.clone().unwrap_or(Term::Nil)),
    ]))
}

fn task_cancel(
    runtime: &mut EditorHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(task_id) = map_field_str_or_symbol(payload, ":task-id") else {
        return mk_error(
            error_tok,
            "editor/task/bad-payload",
            "editor/task::cancel payload must include :task-id".to_string(),
            Some(op),
        );
    };
    let Some(task) = runtime.tasks.get_mut(&task_id) else {
        return mk_error(
            error_tok,
            "editor/task/not-found",
            format!("unknown task-id: {task_id}"),
            Some(op),
        );
    };
    task.state = EditorTaskState::Cancelled;
    task.result = None;
    task.error = None;
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":task-id", Term::Str(task_id)),
    ]))
}

fn task_direct(op: &str, payload: &Term, pol: Option<&OpPolicy>, error_tok: SealId) -> Value {
    match run_editor_task_kind(op, payload, pol) {
        Ok(v) => Value::Data(v),
        Err(e) => Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::Data(map_term([
                (":error/code", Term::Str("editor/task/error".to_string())),
                (
                    ":error/message",
                    Term::Str("editor task operation failed".to_string()),
                ),
                (":error/op", Term::Symbol(op.to_string())),
                (":error/context", e),
            ]))),
        },
    }
}

fn run_editor_task_kind(kind: &str, input: &Term, pol: Option<&OpPolicy>) -> Result<Term, Term> {
    match kind {
        "editor/task::parse-module" => {
            let source = source_from_input(input, pol)?;
            let forms = parse_module(&source)
                .map_err(|e| err_term("editor/task/parse", &format!("{e}")))?;
            Ok(map_term([
                (":ok", Term::Bool(true)),
                (":forms", Term::Int(BigInt::from(forms.len() as u64))),
                (
                    ":module-h",
                    Term::Str(blake3::Hash::from(hash_module(&forms)).to_hex().to_string()),
                ),
            ]))
        }
        "editor/task::fmt-coreform" => {
            let source = source_from_input(input, pol)?;
            let forms = parse_module(&source)
                .map_err(|e| err_term("editor/task/parse", &format!("{e}")))?;
            let canon = canonicalize_module(forms)
                .map_err(|e| err_term("editor/task/canonicalize", &format!("{e}")))?;
            let output = print_module(&canon);
            maybe_write_output(input, pol, &output)?;
            Ok(map_term([
                (":ok", Term::Bool(true)),
                (":output", Term::Str(output)),
            ]))
        }
        "editor/task::lint-module" => {
            let source = source_from_input(input, pol)?;
            let forms = parse_module(&source)
                .map_err(|e| err_term("editor/task/parse", &format!("{e}")))?;
            let canon = canonicalize_module(forms.clone())
                .map_err(|e| err_term("editor/task/canonicalize", &format!("{e}")))?;
            let normalized = print_module(&canon);
            let original_roundtrip = print_module(&forms);
            let warnings = if normalized == original_roundtrip {
                Vec::new()
            } else {
                vec![map_term([
                    (":level", Term::Symbol(":warn".to_string())),
                    (":code", Term::Str("editor/lint/non-canonical".to_string())),
                    (
                        ":msg",
                        Term::Str("module is not in canonical CoreForm formatting".to_string()),
                    ),
                ])]
            };
            Ok(map_term([
                (":ok", Term::Bool(true)),
                (":warnings", Term::Vector(warnings)),
            ]))
        }
        "editor/task::optimize-module" => {
            let source = source_from_input(input, pol)?;
            let forms = parse_module(&source)
                .map_err(|e| err_term("editor/task/parse", &format!("{e}")))?;
            let canon = canonicalize_module(forms)
                .map_err(|e| err_term("editor/task/canonicalize", &format!("{e}")))?;
            let output = print_module(&canon);
            maybe_write_output(input, pol, &output)?;
            Ok(map_term([
                (":ok", Term::Bool(true)),
                (":output", Term::Str(output)),
                (":optimized", Term::Bool(true)),
            ]))
        }
        "editor/task::test-pkg" | "editor/task::typecheck-pkg" => Err(err_term(
            "editor/task/unsupported-kind",
            &format!("unsupported editor task kind in host backend: {kind}"),
        )),
        _ => Err(err_term(
            "editor/task/unsupported-kind",
            &format!("unsupported editor task kind in host backend: {kind}"),
        )),
    }
}

fn source_from_input(input: &Term, pol: Option<&OpPolicy>) -> Result<String, Term> {
    if let Some(source) = map_field_str_or_symbol(input, ":source") {
        return Ok(source);
    }
    if let Ok(path) = payload_path(input) {
        let base_dir =
            effective_base_dir(pol).map_err(|e| err_term("editor/task/path", &format!("{e}")))?;
        let abs_path = sandbox_path_read(&base_dir, &path)
            .map_err(|e| err_term("editor/task/path", &format!("{e}")))?;
        let bytes = read_file_with_optional_limit(&abs_path, None, None)
            .map_err(|e| err_term("editor/task/read", &format!("{e:?}")))?;
        let s =
            String::from_utf8(bytes).map_err(|e| err_term("editor/task/utf8", &format!("{e}")))?;
        return Ok(s);
    }
    Err(err_term(
        "editor/task/bad-input",
        "task input must include :source or :path",
    ))
}

fn maybe_write_output(input: &Term, pol: Option<&OpPolicy>, output: &str) -> Result<(), Term> {
    if let Some(path) =
        map_field_str_or_symbol(input, ":out").or_else(|| map_field_str_or_symbol(input, ":path"))
    {
        let base_dir =
            effective_base_dir(pol).map_err(|e| err_term("editor/task/path", &format!("{e}")))?;
        let write_path = sandbox_path_write(&base_dir, &path, true)
            .map_err(|e| err_term("editor/task/path", &format!("{e}")))?;
        atomic_write_text(&write_path, output.as_bytes())
            .map_err(|e| err_term("editor/task/write", &format!("{e}")))?;
    }
    Ok(())
}

fn default_clipboard_mime(runtime: &EditorHostRuntime) -> String {
    if runtime.clipboard_mime.is_empty() {
        "text/plain".to_string()
    } else {
        runtime.clipboard_mime.clone()
    }
}

fn task_state_term(state: &EditorTaskState) -> Term {
    match state {
        EditorTaskState::Running => Term::Symbol(":running".to_string()),
        EditorTaskState::Done => Term::Symbol(":done".to_string()),
        EditorTaskState::Failed => Term::Symbol(":failed".to_string()),
        EditorTaskState::Cancelled => Term::Symbol(":cancelled".to_string()),
    }
}

fn map_term<const N: usize>(pairs: [(&str, Term); N]) -> Term {
    Term::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (TermOrdKey(Term::symbol(k)), v))
            .collect(),
    )
}

fn map_field<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn map_field_bool(t: &Term, key: &str) -> Option<bool> {
    match map_field(t, key) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn map_field_str_or_symbol(t: &Term, key: &str) -> Option<String> {
    match map_field(t, key) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn term_vec_str(t: &Term) -> Option<Vec<String>> {
    let Term::Vector(items) = t else {
        return None;
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Term::Str(s) => out.push(s.clone()),
            Term::Symbol(s) => out.push(s.clone()),
            _ => return None,
        }
    }
    Some(out)
}

fn err_term(code: &str, msg: &str) -> Term {
    map_term([
        (":code", Term::Str(code.to_string())),
        (":message", Term::Str(msg.to_string())),
    ])
}

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
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
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
