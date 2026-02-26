use super::*;

#[path = "runner_editor_task_registry.rs"]
mod runner_editor_task_registry;
#[path = "runner_editor_task_workflows.rs"]
mod runner_editor_task_workflows;

pub(super) struct TaskExecution {
    pub(super) contract: Term,
    pub(super) partials: Vec<Term>,
    pub(super) result: Term,
}

pub(super) struct TaskOutcome {
    pub(super) partials: Vec<Term>,
    pub(super) result: Term,
}

impl TaskOutcome {
    fn immediate(result: Term) -> Self {
        Self {
            partials: Vec::new(),
            result,
        }
    }
}

pub(super) fn execute_editor_task(kind: &str, input: &Term) -> TaskExecution {
    runner_editor_task_registry::execute_editor_task(kind, input)
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
        TermOrdKey(Term::symbol(":defined-symbol-count")),
        Term::Int((symbols.len() as i64).into()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":defined-symbols")),
        Term::Vector(symbols.iter().cloned().map(Term::Str).collect::<Vec<_>>()),
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

pub(super) fn task_diagnostic_error(op: &str, code: &str, path: &str, msg: String) -> Term {
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

pub(super) fn result_ok(result: &Term) -> bool {
    let Some(m) = payload_map(result) else {
        return false;
    };
    matches!(
        m.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(Term::Bool(true))
    )
}

fn hash_module_hex(hash: [u8; 32]) -> String {
    gc_vcs::bytes32_to_hex(&hash)
}
