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

#[derive(Debug, Clone)]
struct TaskState {
    kind: String,
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
        return Some(Value::Data(first_party_editor_response(
            runtime, op, payload,
        )));
    }
    Some(match call_host_bridge("editor", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
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
    let result = execute_editor_task(&task_kind, &task_input);
    let state = if result_ok(&result) {
        ":done".to_string()
    } else {
        ":failed".to_string()
    };
    runtime.next_task = runtime.next_task.saturating_add(1);
    let task_id = format!("task-first-party-{}", runtime.next_task);
    runtime.tasks.insert(
        task_id.clone(),
        TaskState {
            kind: task_kind.clone(),
            state: state.clone(),
            result,
        },
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":task-id", Term::Str(task_id)),
        (":task-kind", Term::Str(task_kind)),
        (":state", Term::symbol(state)),
    ])
}

fn first_party_task_poll(runtime: &EditorHostRuntime, payload: &Term) -> Term {
    let Some(m) = payload_map(payload) else {
        return editor_error("editor/task::poll", "editor/first-party-invalid-payload");
    };
    let Some(task_id) = map_get_string(m, ":task-id") else {
        return editor_error("editor/task::poll", "editor/first-party-missing-task-id");
    };
    let Some(task) = runtime.tasks.get(&task_id) else {
        return editor_error("editor/task::poll", "editor/first-party-task-not-found");
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":task-id", Term::Str(task_id)),
        (":task-kind", Term::Str(task.kind.clone())),
        (":state", Term::symbol(task.state.clone())),
        (":result", task.result.clone()),
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

fn execute_editor_task(kind: &str, input: &Term) -> Term {
    match kind {
        "editor/task::parse-module" => task_parse_module(input),
        "editor/task::fmt-coreform" => task_fmt_coreform(input),
        "editor/task::lint-module" => task_lint_module(input),
        "editor/task::optimize-module" => task_optimize_module(input),
        "editor/task::typecheck-pkg" => task_typecheck_pkg(input),
        "editor/task::test-pkg" => task_test_pkg(input),
        _ => task_diagnostic_error(
            kind,
            "editor/first-party-task-kind-unsupported",
            "<task>",
            format!("unsupported task kind: {kind}"),
        ),
    }
}

fn task_parse_module(input: &Term) -> Term {
    let (source, path) = match load_module_source(input, "editor/task::parse-module") {
        Ok(ok) => ok,
        Err(err) => return err,
    };
    let forms = match parse_module(&source) {
        Ok(forms) => forms,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::parse-module",
                "editor/task/parse-error",
                &path,
                err.to_string(),
            );
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(canon) => canon,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::parse-module",
                "editor/task/canonicalize-error",
                &path,
                err.to_string(),
            );
        }
    };
    let h = hash_module_hex(hash_module(&canon));
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":path", Term::Str(path)),
        (":module-h", Term::Str(h)),
        (":form-count", Term::Int((canon.len() as i64).into())),
        (":diagnostics", Term::Vector(Vec::new())),
        (":error-count", Term::Int(0_i64.into())),
        (":warn-count", Term::Int(0_i64.into())),
    ])
}

fn task_fmt_coreform(input: &Term) -> Term {
    let (source, path) = match load_module_source(input, "editor/task::fmt-coreform") {
        Ok(ok) => ok,
        Err(err) => return err,
    };
    let forms = match parse_module(&source) {
        Ok(forms) => forms,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::fmt-coreform",
                "editor/task/parse-error",
                &path,
                err.to_string(),
            );
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(canon) => canon,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::fmt-coreform",
                "editor/task/canonicalize-error",
                &path,
                err.to_string(),
            );
        }
    };
    let formatted = print_module(&canon);
    let formatted_h = hash_module_hex(hash_module(&canon));
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":path", Term::Str(path)),
        (":formatted", Term::Str(formatted)),
        (":formatted-h", Term::Str(formatted_h)),
        (":diagnostics", Term::Vector(Vec::new())),
        (":error-count", Term::Int(0_i64.into())),
        (":warn-count", Term::Int(0_i64.into())),
    ])
}

fn task_lint_module(input: &Term) -> Term {
    let (source, path) = match load_module_source(input, "editor/task::lint-module") {
        Ok(ok) => ok,
        Err(err) => return err,
    };
    let forms = match parse_module(&source) {
        Ok(forms) => forms,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::lint-module",
                "editor/task/parse-error",
                &path,
                err.to_string(),
            );
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(canon) => canon,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::lint-module",
                "editor/task/canonicalize-error",
                &path,
                err.to_string(),
            );
        }
    };
    let diagnostics = lint_module_diagnostics(&canon, &path);
    task_result_from_diagnostics(
        "editor/task::lint-module",
        diagnostics,
        vec![(
            TermOrdKey(Term::symbol(":module-h")),
            Term::Str(hash_module_hex(hash_module(&canon))),
        )],
    )
}

fn task_optimize_module(input: &Term) -> Term {
    let (source, path) = match load_module_source(input, "editor/task::optimize-module") {
        Ok(ok) => ok,
        Err(err) => return err,
    };
    let forms = match parse_module(&source) {
        Ok(forms) => forms,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::optimize-module",
                "editor/task/parse-error",
                &path,
                err.to_string(),
            );
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(canon) => canon,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::optimize-module",
                "editor/task/canonicalize-error",
                &path,
                err.to_string(),
            );
        }
    };
    let original_h = hash_module_hex(hash_module(&canon));
    let (optimized_raw, report) = optimize_module_with_report(&canon);
    let optimized = match canonicalize_module(optimized_raw) {
        Ok(optimized) => optimized,
        Err(err) => {
            return task_diagnostic_error(
                "editor/task::optimize-module",
                "editor/task/optimize-canonicalize-error",
                &path,
                err.to_string(),
            );
        }
    };
    let optimized_h = hash_module_hex(hash_module(&optimized));
    let mut optimize_stats = BTreeMap::new();
    optimize_stats.insert(
        TermOrdKey(Term::symbol(":egg-runs")),
        Term::Int((report.stats.egg_runs as i64).into()),
    );
    optimize_stats.insert(
        TermOrdKey(Term::symbol(":egg-iterations")),
        Term::Int((report.stats.iterations as i64).into()),
    );
    optimize_stats.insert(
        TermOrdKey(Term::symbol(":egg-eclasses")),
        Term::Int((report.stats.eclasses as i64).into()),
    );
    optimize_stats.insert(
        TermOrdKey(Term::symbol(":egg-enodes")),
        Term::Int((report.stats.enodes as i64).into()),
    );
    optimize_stats.insert(
        TermOrdKey(Term::symbol(":rewrites-applied")),
        Term::Map(
            report
                .stats
                .rewrites_applied
                .iter()
                .map(|(k, v)| {
                    (
                        TermOrdKey(Term::Str(k.clone())),
                        Term::Int((*v as i64).into()),
                    )
                })
                .collect(),
        ),
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":path", Term::Str(path)),
        (":module-h", Term::Str(original_h)),
        (":optimized-h", Term::Str(optimized_h)),
        (":changed", Term::Bool(report.changed)),
        (":optimized", Term::Str(print_module(&optimized))),
        (":optimizer", Term::Map(optimize_stats)),
        (":diagnostics", Term::Vector(Vec::new())),
        (":error-count", Term::Int(0_i64.into())),
        (":warn-count", Term::Int(0_i64.into())),
    ])
}

fn task_typecheck_pkg(input: &Term) -> Term {
    let analyzed = match analyze_package(input, false) {
        Ok(ok) => ok,
        Err(err) => return err,
    };
    let mut base = analyzed;
    base.insert(
        TermOrdKey(Term::symbol(":task-kind")),
        Term::symbol("editor/task::typecheck-pkg"),
    );
    Term::Map(base)
}

fn task_test_pkg(input: &Term) -> Term {
    let analyzed = match analyze_package(input, true) {
        Ok(ok) => ok,
        Err(err) => return err,
    };
    let mut base = analyzed;
    base.insert(
        TermOrdKey(Term::symbol(":task-kind")),
        Term::symbol("editor/task::test-pkg"),
    );
    let passed = matches!(
        base.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(Term::Bool(true))
    );
    base.insert(TermOrdKey(Term::symbol(":passed")), Term::Bool(passed));
    Term::Map(base)
}

fn analyze_package(
    input: &Term,
    include_test_symbol_checks: bool,
) -> Result<BTreeMap<TermOrdKey, Term>, Term> {
    let Some(input_map) = payload_map(input) else {
        return Err(task_diagnostic_error(
            "editor/task::pkg-analysis",
            "editor/task/invalid-input",
            "<pkg>",
            "expected map input".to_string(),
        ));
    };
    let Some(pkg_path) =
        map_get_string(input_map, ":pkg").or_else(|| map_get_string(input_map, ":path"))
    else {
        return Err(task_diagnostic_error(
            "editor/task::pkg-analysis",
            "editor/task/pkg-path-missing",
            "<pkg>",
            "missing :pkg or :path".to_string(),
        ));
    };
    let pkg_pathbuf = PathBuf::from(pkg_path.clone());
    let (manifest, dir) = match PackageManifest::load(&pkg_pathbuf) {
        Ok(ok) => ok,
        Err(err) => {
            return Err(task_diagnostic_error(
                "editor/task::pkg-analysis",
                "editor/task/pkg-load-error",
                &pkg_path,
                err.to_string(),
            ));
        }
    };

    let mut diagnostics = Vec::new();
    let mut symbols = BTreeSet::<String>::new();
    for me in &manifest.modules {
        let module_path = dir.join(&me.path);
        let src = match std::fs::read_to_string(&module_path) {
            Ok(src) => src,
            Err(err) => {
                diagnostics.push(diag_term(
                    ":error",
                    "editor/task/module-read-error",
                    &me.path,
                    err.to_string(),
                ));
                continue;
            }
        };
        let forms = match parse_module(&src) {
            Ok(forms) => forms,
            Err(err) => {
                diagnostics.push(diag_term(
                    ":error",
                    "editor/task/module-parse-error",
                    &me.path,
                    err.to_string(),
                ));
                continue;
            }
        };
        let canon = match canonicalize_module(forms) {
            Ok(canon) => canon,
            Err(err) => {
                diagnostics.push(diag_term(
                    ":error",
                    "editor/task/module-canonicalize-error",
                    &me.path,
                    err.to_string(),
                ));
                continue;
            }
        };
        for sym in module_def_symbols(&canon) {
            symbols.insert(sym);
        }
        if let Some(expected_hash) = &me.hash {
            let observed = hash_module_hex(hash_module(&canon));
            if expected_hash.to_ascii_lowercase() != observed {
                diagnostics.push(diag_term(
                    ":error",
                    "editor/task/module-hash-mismatch",
                    &me.path,
                    format!("expected {expected_hash} observed {observed}"),
                ));
            }
        }
    }

    if include_test_symbol_checks {
        let test_symbols = manifest
            .tests
            .iter()
            .chain(manifest.property_tests.iter())
            .chain(manifest.gfx.golden_tests.iter())
            .chain(manifest.gfx.frame_budget_tests.iter());
        for sym in test_symbols {
            if !symbols.contains(sym) {
                diagnostics.push(diag_term(
                    ":error",
                    "editor/task/test-symbol-missing",
                    &pkg_path,
                    format!("declared test symbol missing from modules: {sym}"),
                ));
            }
        }
    }

    let err_count = diagnostics
        .iter()
        .filter(|d| diag_level_is(d, ":error"))
        .count() as i64;
    let warn_count = diagnostics
        .iter()
        .filter(|d| diag_level_is(d, ":warn"))
        .count() as i64;
    let ok = err_count == 0;
    let mut out = BTreeMap::new();
    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    out.insert(
        TermOrdKey(Term::symbol(":path")),
        Term::Str(path_to_slash(&pkg_pathbuf)),
    );
    out.insert(
        TermOrdKey(Term::symbol(":module-count")),
        Term::Int((manifest.modules.len() as i64).into()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":test-declaration-count")),
        Term::Int(
            ((manifest.tests.len()
                + manifest.property_tests.len()
                + manifest.gfx.golden_tests.len()
                + manifest.gfx.frame_budget_tests.len()) as i64)
                .into(),
        ),
    );
    out.insert(
        TermOrdKey(Term::symbol(":diagnostics")),
        Term::Vector(diagnostics),
    );
    out.insert(
        TermOrdKey(Term::symbol(":error-count")),
        Term::Int(err_count.into()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":warn-count")),
        Term::Int(warn_count.into()),
    );
    Ok(out)
}

fn load_module_source(input: &Term, op: &str) -> Result<(String, String), Term> {
    let Some(input_map) = payload_map(input) else {
        return Err(task_diagnostic_error(
            op,
            "editor/task/invalid-input",
            "<module>",
            "expected map input".to_string(),
        ));
    };
    if let Some(src) = map_get_string(input_map, ":source") {
        let path = map_get_string(input_map, ":path").unwrap_or_else(|| "<memory>".to_string());
        return Ok((src, path));
    }
    let Some(path) = map_get_string(input_map, ":path") else {
        return Err(task_diagnostic_error(
            op,
            "editor/task/path-missing",
            "<module>",
            "missing :source or :path".to_string(),
        ));
    };
    match std::fs::read_to_string(&path) {
        Ok(source) => Ok((source, path)),
        Err(err) => Err(task_diagnostic_error(
            op,
            "editor/task/module-read-error",
            &path,
            err.to_string(),
        )),
    }
}

fn lint_module_diagnostics(forms: &[Term], path: &str) -> Vec<Term> {
    let mut diagnostics = Vec::new();
    let mut seen_defs = BTreeSet::<String>::new();
    let mut dup_defs = BTreeSet::<String>::new();
    let mut meta_defs = 0_i64;
    for form in forms {
        let Some(items) = form.as_proper_list() else {
            continue;
        };
        if items.len() != 3 {
            continue;
        }
        let Term::Symbol(head) = items[0] else {
            continue;
        };
        if head != "def" {
            continue;
        }
        let Term::Symbol(name) = items[1] else {
            continue;
        };
        if name == "::meta" {
            meta_defs = meta_defs.saturating_add(1);
        }
        if !seen_defs.insert(name.clone()) {
            dup_defs.insert(name.clone());
        }
    }
    if meta_defs == 0 {
        diagnostics.push(diag_term(
            ":warn",
            "editor/lint/missing-meta",
            path,
            "module is missing ::meta definition".to_string(),
        ));
    } else if meta_defs > 1 {
        diagnostics.push(diag_term(
            ":error",
            "editor/lint/duplicate-meta",
            path,
            "module defines ::meta more than once".to_string(),
        ));
    }
    for def in dup_defs {
        diagnostics.push(diag_term(
            ":error",
            "editor/lint/duplicate-def",
            path,
            format!("duplicate definition: {def}"),
        ));
    }
    diagnostics
}

fn module_def_symbols(forms: &[Term]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for form in forms {
        let Some(items) = form.as_proper_list() else {
            continue;
        };
        if items.len() != 3 {
            continue;
        }
        let Term::Symbol(head) = items[0] else {
            continue;
        };
        if head != "def" {
            continue;
        }
        let Term::Symbol(name) = items[1] else {
            continue;
        };
        out.insert(name.clone());
    }
    out
}

fn task_result_from_diagnostics(
    _op: &str,
    diagnostics: Vec<Term>,
    extra_entries: Vec<(TermOrdKey, Term)>,
) -> Term {
    let err_count = diagnostics
        .iter()
        .filter(|d| diag_level_is(d, ":error"))
        .count() as i64;
    let warn_count = diagnostics
        .iter()
        .filter(|d| diag_level_is(d, ":warn"))
        .count() as i64;
    let mut out = BTreeMap::new();
    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(err_count == 0));
    out.insert(
        TermOrdKey(Term::symbol(":diagnostics")),
        Term::Vector(diagnostics),
    );
    out.insert(
        TermOrdKey(Term::symbol(":error-count")),
        Term::Int(err_count.into()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":warn-count")),
        Term::Int(warn_count.into()),
    );
    for (k, v) in extra_entries {
        out.insert(k, v);
    }
    Term::Map(out)
}

fn task_diagnostic_error(op: &str, code: &str, path: &str, msg: String) -> Term {
    let diagnostics = vec![diag_term(":error", code, path, msg)];
    let mut out = BTreeMap::new();
    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
    out.insert(TermOrdKey(Term::symbol(":error/op")), Term::symbol(op));
    out.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(code.to_string()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":diagnostics")),
        Term::Vector(diagnostics),
    );
    out.insert(
        TermOrdKey(Term::symbol(":error-count")),
        Term::Int(1_i64.into()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":warn-count")),
        Term::Int(0_i64.into()),
    );
    Term::Map(out)
}

fn diag_term(level: &str, code: &str, path: &str, msg: String) -> Term {
    map_term(vec![
        (":level", Term::symbol(level)),
        (":code", Term::Str(code.to_string())),
        (":path", Term::Str(path.to_string())),
        (":msg", Term::Str(msg)),
    ])
}

fn diag_level_is(diag: &Term, expected_level: &str) -> bool {
    let Some(map) = payload_map(diag) else {
        return false;
    };
    matches!(
        map.get(&TermOrdKey(Term::symbol(":level"))),
        Some(Term::Symbol(level)) if level == expected_level
    )
}

fn result_ok(result: &Term) -> bool {
    payload_map(result)
        .and_then(|m| m.get(&TermOrdKey(Term::symbol(":ok"))))
        .and_then(|value| match value {
            Term::Bool(ok) => Some(*ok),
            _ => None,
        })
        .unwrap_or(false)
}

fn hash_module_hex(hash: [u8; 32]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in hash {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
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
            | "editor/plugin::command"
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
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
