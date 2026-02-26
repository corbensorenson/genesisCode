use super::*;
use std::collections::BTreeMap;
use std::collections::VecDeque;

pub(super) fn task_build_pkg(input: &Term) -> TaskOutcome {
    let analyzed = match analyze_package(input, false) {
        Ok(ok) => ok,
        Err(err) => return TaskOutcome::immediate(err),
    };
    let pkg_path = map_get_map_string(&analyzed, ":path").unwrap_or_else(|| "<pkg>".to_string());
    let module_count = map_get_map_int(&analyzed, ":module-count").unwrap_or(0);
    let mut out = analyzed;
    out.insert(
        TermOrdKey(Term::symbol(":task-kind")),
        Term::symbol("editor/task::build-pkg"),
    );
    out.insert(
        TermOrdKey(Term::symbol(":build/mode")),
        Term::symbol(":deterministic-plan"),
    );
    out.insert(
        TermOrdKey(Term::symbol(":build/targets")),
        Term::Vector(vec![
            Term::symbol(":service-runtime"),
            Term::symbol(":edge"),
            Term::symbol(":ios"),
            Term::symbol(":android"),
        ]),
    );
    out.insert(
        TermOrdKey(Term::symbol(":build/artifact-h")),
        Term::Str(package_artifact_hash(&pkg_path, module_count)),
    );
    let partials = vec![
        workflow_partial(
            "editor/task::build-pkg",
            1,
            ":load-manifest",
            34,
            vec![
                (":path", Term::Str(pkg_path.clone())),
                (":module-count", Term::Int(module_count.into())),
            ],
        ),
        workflow_partial(
            "editor/task::build-pkg",
            2,
            ":validate-deps",
            67,
            vec![(":path", Term::Str(pkg_path.clone()))],
        ),
        workflow_partial(
            "editor/task::build-pkg",
            3,
            ":emit-plan",
            100,
            vec![(":path", Term::Str(pkg_path))],
        ),
    ];
    TaskOutcome {
        partials,
        result: Term::Map(out),
    }
}

pub(super) fn task_run_pkg(input: &Term) -> TaskOutcome {
    let analyzed = match analyze_package(input, false) {
        Ok(ok) => ok,
        Err(err) => return TaskOutcome::immediate(err),
    };
    let pkg_path = map_get_map_string(&analyzed, ":path").unwrap_or_else(|| "<pkg>".to_string());
    let entry = payload_map(input)
        .and_then(|m| map_get_string(m, ":entry"))
        .unwrap_or_else(|| "pkg/a::x".to_string());
    let symbols = map_get_map_string_vec(&analyzed, ":defined-symbols");
    if !symbols.iter().any(|sym| sym == &entry) {
        return TaskOutcome::immediate(task_diagnostic_error(
            "editor/task::run-pkg",
            "editor/task/run-entry-missing",
            &pkg_path,
            format!("entry symbol not found in package: {entry}"),
        ));
    }
    let mut out = analyzed;
    out.insert(
        TermOrdKey(Term::symbol(":task-kind")),
        Term::symbol("editor/task::run-pkg"),
    );
    out.insert(
        TermOrdKey(Term::symbol(":run/entry")),
        Term::Str(entry.clone()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":run/launch-contract")),
        map_term(vec![
            (":entry", Term::Str(entry.clone())),
            (":mode", Term::symbol(":deterministic-simulated-launch")),
            (":args", map_get_payload_vec(input, ":args")),
        ]),
    );
    let partials = vec![
        workflow_partial(
            "editor/task::run-pkg",
            1,
            ":load-runtime",
            30,
            vec![
                (":path", Term::Str(pkg_path.clone())),
                (":entry", Term::Str(entry.clone())),
            ],
        ),
        workflow_partial(
            "editor/task::run-pkg",
            2,
            ":resolve-entry",
            65,
            vec![(":entry", Term::Str(entry.clone()))],
        ),
        workflow_partial(
            "editor/task::run-pkg",
            3,
            ":launch-ready",
            100,
            vec![(":entry", Term::Str(entry))],
        ),
    ];
    TaskOutcome {
        partials,
        result: Term::Map(out),
    }
}

pub(super) fn task_debug_pkg(input: &Term) -> TaskOutcome {
    let analyzed = match analyze_package(input, false) {
        Ok(ok) => ok,
        Err(err) => return TaskOutcome::immediate(err),
    };
    let pkg_path = map_get_map_string(&analyzed, ":path").unwrap_or_else(|| "<pkg>".to_string());
    let entry = payload_map(input)
        .and_then(|m| map_get_string(m, ":entry"))
        .unwrap_or_else(|| "pkg/a::x".to_string());
    let breakpoints = payload_map(input)
        .and_then(|m| map_get_string_vec(m, ":breakpoints"))
        .unwrap_or_default();
    let mut out = analyzed;
    out.insert(
        TermOrdKey(Term::symbol(":task-kind")),
        Term::symbol("editor/task::debug-pkg"),
    );
    out.insert(
        TermOrdKey(Term::symbol(":debug/entry")),
        Term::Str(entry.clone()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":debug/session-id")),
        Term::Str(package_artifact_hash(
            &format!("{pkg_path}:{entry}"),
            breakpoints.len() as i64,
        )),
    );
    out.insert(
        TermOrdKey(Term::symbol(":debug/breakpoints")),
        Term::Vector(breakpoints.iter().map(|bp| Term::Str(bp.clone())).collect()),
    );
    let partials = vec![
        workflow_partial(
            "editor/task::debug-pkg",
            1,
            ":prepare-session",
            25,
            vec![(":path", Term::Str(pkg_path.clone()))],
        ),
        workflow_partial(
            "editor/task::debug-pkg",
            2,
            ":set-breakpoints",
            65,
            vec![(
                ":breakpoint-count",
                Term::Int((breakpoints.len() as i64).into()),
            )],
        ),
        workflow_partial(
            "editor/task::debug-pkg",
            3,
            ":ready",
            100,
            vec![(":entry", Term::Str(entry))],
        ),
    ];
    TaskOutcome {
        partials,
        result: Term::Map(out),
    }
}

pub(super) fn task_refactor_module(input: &Term) -> TaskOutcome {
    let Some(input_map) = payload_map(input) else {
        return TaskOutcome::immediate(task_diagnostic_error(
            "editor/task::refactor-module",
            "editor/task/invalid-input",
            "<module>",
            "expected map input".to_string(),
        ));
    };
    let Some(from) = map_get_string(input_map, ":from") else {
        return TaskOutcome::immediate(task_diagnostic_error(
            "editor/task::refactor-module",
            "editor/task/refactor-from-missing",
            "<module>",
            "missing :from".to_string(),
        ));
    };
    let Some(to) = map_get_string(input_map, ":to") else {
        return TaskOutcome::immediate(task_diagnostic_error(
            "editor/task::refactor-module",
            "editor/task/refactor-to-missing",
            "<module>",
            "missing :to".to_string(),
        ));
    };
    let (source, path) = match load_module_source(input, "editor/task::refactor-module") {
        Ok(ok) => ok,
        Err(err) => return TaskOutcome::immediate(err),
    };
    let forms = match parse_module(&source) {
        Ok(forms) => forms,
        Err(err) => {
            return TaskOutcome::immediate(task_diagnostic_error(
                "editor/task::refactor-module",
                "editor/task/parse-error",
                &path,
                err.to_string(),
            ));
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(canon) => canon,
        Err(err) => {
            return TaskOutcome::immediate(task_diagnostic_error(
                "editor/task::refactor-module",
                "editor/task/canonicalize-error",
                &path,
                err.to_string(),
            ));
        }
    };
    let module_h = hash_module_hex(hash_module(&canon));
    let rewritten_forms = canon
        .iter()
        .map(|form| rename_symbol_term(form, &from, &to))
        .collect::<Vec<_>>();
    let rewritten = match canonicalize_module(rewritten_forms) {
        Ok(rewritten) => rewritten,
        Err(err) => {
            return TaskOutcome::immediate(task_diagnostic_error(
                "editor/task::refactor-module",
                "editor/task/refactor-canonicalize-error",
                &path,
                err.to_string(),
            ));
        }
    };
    let updated_h = hash_module_hex(hash_module(&rewritten));
    let changed = module_h != updated_h;
    let partials = vec![
        workflow_partial(
            "editor/task::refactor-module",
            1,
            ":parse",
            30,
            vec![
                (":path", Term::Str(path.clone())),
                (":from", Term::Str(from.clone())),
                (":to", Term::Str(to.clone())),
            ],
        ),
        workflow_partial(
            "editor/task::refactor-module",
            2,
            ":rewrite",
            75,
            vec![(":changed", Term::Bool(changed))],
        ),
        workflow_partial(
            "editor/task::refactor-module",
            3,
            ":emit",
            100,
            vec![(":updated-h", Term::Str(updated_h.clone()))],
        ),
    ];
    TaskOutcome {
        partials,
        result: map_term(vec![
            (":ok", Term::Bool(true)),
            (":task-kind", Term::symbol("editor/task::refactor-module")),
            (":path", Term::Str(path)),
            (":from", Term::Str(from)),
            (":to", Term::Str(to)),
            (":changed", Term::Bool(changed)),
            (":module-h", Term::Str(module_h)),
            (":updated-h", Term::Str(updated_h)),
            (":updated", Term::Str(print_module(&rewritten))),
            (":diagnostics", Term::Vector(Vec::new())),
            (":error-count", Term::Int(0_i64.into())),
            (":warn-count", Term::Int(0_i64.into())),
        ]),
    }
}

pub(super) fn task_index_workspace(input: &Term) -> TaskOutcome {
    let Some(input_map) = payload_map(input) else {
        return TaskOutcome::immediate(task_diagnostic_error(
            "editor/task::index-workspace",
            "editor/task/invalid-input",
            "<workspace>",
            "expected map input".to_string(),
        ));
    };
    let Some(root) = map_get_string(input_map, ":root") else {
        return TaskOutcome::immediate(task_diagnostic_error(
            "editor/task::index-workspace",
            "editor/task/index-root-missing",
            "<workspace>",
            "missing :root".to_string(),
        ));
    };
    let max_packages = map_get_int(input_map, ":max-packages")
        .unwrap_or(64)
        .clamp(1, 4096) as usize;
    let max_partials = map_get_int(input_map, ":max-partials")
        .unwrap_or(16)
        .clamp(1, 1024) as usize;
    let root_path = PathBuf::from(root.clone());
    let package_paths = match collect_package_paths(&root_path, max_packages) {
        Ok(paths) => paths,
        Err(msg) => {
            return TaskOutcome::immediate(task_diagnostic_error(
                "editor/task::index-workspace",
                "editor/task/index-scan-error",
                &root,
                msg,
            ));
        }
    };

    let mut package_entries = Vec::new();
    let mut diagnostics = Vec::new();
    for path in &package_paths {
        match PackageManifest::load(path) {
            Ok((manifest, _dir)) => {
                package_entries.push(map_term(vec![
                    (":path", Term::Str(path_to_slash(path))),
                    (
                        ":module-count",
                        Term::Int((manifest.modules.len() as i64).into()),
                    ),
                    (
                        ":test-declaration-count",
                        Term::Int(
                            ((manifest.tests.len()
                                + manifest.property_tests.len()
                                + manifest.gfx.golden_tests.len()
                                + manifest.gfx.frame_budget_tests.len())
                                as i64)
                                .into(),
                        ),
                    ),
                ]));
            }
            Err(err) => {
                diagnostics.push(diag_term(
                    ":warn",
                    "editor/task/index-manifest-load-warning",
                    &path_to_slash(path),
                    err.to_string(),
                ));
            }
        }
    }

    let err_count = diagnostics
        .iter()
        .filter(|diag| diag_level_is(diag, ":error"))
        .count() as i64;
    let warn_count = diagnostics
        .iter()
        .filter(|diag| diag_level_is(diag, ":warn"))
        .count() as i64;

    let partials = package_entries
        .iter()
        .take(max_partials)
        .enumerate()
        .map(|(idx, pkg)| {
            workflow_partial(
                "editor/task::index-workspace",
                (idx + 1) as u64,
                ":index-package",
                (((idx + 1) as f64 / (package_entries.len().max(1) as f64)) * 100.0).round() as i64,
                vec![(":package", pkg.clone())],
            )
        })
        .collect::<Vec<_>>();

    TaskOutcome {
        partials,
        result: map_term(vec![
            (":ok", Term::Bool(err_count == 0)),
            (":task-kind", Term::symbol("editor/task::index-workspace")),
            (":workspace/root", Term::Str(path_to_slash(&root_path))),
            (
                ":package-count",
                Term::Int((package_entries.len() as i64).into()),
            ),
            (":packages", Term::Vector(package_entries)),
            (":diagnostics", Term::Vector(diagnostics)),
            (":error-count", Term::Int(err_count.into())),
            (":warn-count", Term::Int(warn_count.into())),
        ]),
    }
}

fn workflow_partial(
    kind: &str,
    seq: u64,
    phase: &str,
    progress_percent: i64,
    extras: Vec<(&str, Term)>,
) -> Term {
    let mut entries = vec![
        (":task-kind", Term::symbol(kind)),
        (":seq", Term::Int((seq as i64).into())),
        (":phase", Term::symbol(phase)),
        (
            ":progress-percent",
            Term::Int(progress_percent.clamp(0, 100).into()),
        ),
    ];
    entries.extend(extras);
    map_term(entries)
}

fn package_artifact_hash(seed: &str, module_count: i64) -> String {
    let payload = format!("{seed}:{module_count}");
    gc_vcs::bytes32_to_hex(blake3::hash(payload.as_bytes()).as_bytes())
}

fn rename_symbol_term(term: &Term, from: &str, to: &str) -> Term {
    match term {
        Term::Nil => Term::Nil,
        Term::Bool(v) => Term::Bool(*v),
        Term::Int(v) => Term::Int(v.clone()),
        Term::Str(v) => Term::Str(v.clone()),
        Term::Bytes(v) => Term::Bytes(v.clone()),
        Term::Symbol(v) if v == from => Term::Symbol(to.to_string()),
        Term::Symbol(v) => Term::Symbol(v.clone()),
        Term::Pair(car, cdr) => Term::Pair(
            Box::new(rename_symbol_term(car, from, to)),
            Box::new(rename_symbol_term(cdr, from, to)),
        ),
        Term::Vector(items) => Term::Vector(
            items
                .iter()
                .map(|item| rename_symbol_term(item, from, to))
                .collect(),
        ),
        Term::Map(items) => {
            let mut out = BTreeMap::new();
            for (key, value) in items {
                out.insert(
                    TermOrdKey(rename_symbol_term(&key.0, from, to)),
                    rename_symbol_term(value, from, to),
                );
            }
            Term::Map(out)
        }
    }
}

fn collect_package_paths(root: &Path, max_packages: usize) -> Result<Vec<PathBuf>, String> {
    let mut queue = VecDeque::new();
    queue.push_back(root.to_path_buf());
    let mut package_paths = Vec::new();
    while let Some(dir) = queue.pop_front() {
        let read_dir = std::fs::read_dir(&dir)
            .map_err(|err| format!("failed to read directory {}: {err}", path_to_slash(&dir)))?;
        let mut entries = read_dir
            .filter_map(|entry| entry.ok())
            .collect::<Vec<std::fs::DirEntry>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                queue.push_back(path);
                continue;
            }
            if file_type.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy() == "package.toml")
            {
                package_paths.push(path);
                if package_paths.len() >= max_packages {
                    package_paths.sort_by_key(|p| path_to_slash(p));
                    return Ok(package_paths);
                }
            }
        }
    }
    package_paths.sort_by_key(|p| path_to_slash(p));
    Ok(package_paths)
}

fn map_get_map_int(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<i64> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|value| match value {
            Term::Int(n) => n.to_string().parse::<i64>().ok(),
            _ => None,
        })
}

fn map_get_map_string(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|value| match value {
            Term::Str(v) => Some(v.clone()),
            Term::Symbol(v) => Some(v.clone()),
            _ => None,
        })
}

fn map_get_map_string_vec(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Vec<String> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|value| match value {
            Term::Vector(items) => Some(
                items
                    .iter()
                    .filter_map(|item| match item {
                        Term::Str(v) => Some(v.clone()),
                        Term::Symbol(v) => Some(v.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn map_get_int(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<i64> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|value| match value {
            Term::Int(n) => n.to_string().parse::<i64>().ok(),
            _ => None,
        })
}

fn map_get_payload_vec(payload: &Term, key: &str) -> Term {
    payload_map(payload)
        .and_then(|map| map.get(&TermOrdKey(Term::symbol(key))))
        .cloned()
        .unwrap_or_else(|| Term::Vector(Vec::new()))
}
