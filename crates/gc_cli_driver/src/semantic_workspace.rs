use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[path = "semantic_workspace_misc.rs"]
mod semantic_workspace_misc;
#[path = "semantic_workspace_types.rs"]
mod semantic_workspace_types;
use semantic_workspace_misc::{
    looks_like_scoped_symbol, refactor_kind_symbol, refactor_kind_token, term_tag,
};
use semantic_workspace_types::{
    DefinitionSite, ModuleAnalysis, PathStep, PlannedOp, RefactorConflict, SymbolOccurrence,
    WorkspaceAnalysis,
};

pub(super) fn cmd_semantic_edit_workspace_graph(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);
    let analysis = analyze_workspace(cli, pkg, &frontend)?;

    let mut duplicate_symbol_owners = Vec::new();
    for (symbol, owners) in &analysis.owners {
        if owners.len() > 1 {
            duplicate_symbol_owners.push(serde_json::json!({
                "symbol": symbol,
                "module_paths": owners,
            }));
        }
    }

    let mut edge_counts: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    let mut unresolved_symbols: BTreeSet<String> = BTreeSet::new();
    for module in &analysis.modules {
        for occ in &module.occurrences {
            if !looks_like_scoped_symbol(&occ.symbol) {
                continue;
            }
            match analysis.owners.get(&occ.symbol) {
                Some(owners) if owners.len() == 1 => {
                    let owner = owners[0].clone();
                    if owner != module.module_path {
                        *edge_counts
                            .entry((module.module_path.clone(), owner, occ.symbol.clone()))
                            .or_insert(0) += 1;
                    }
                }
                Some(_owners) => {
                    unresolved_symbols.insert(occ.symbol.clone());
                }
                None => {
                    unresolved_symbols.insert(occ.symbol.clone());
                }
            }
        }
    }

    let total_nodes: u64 = analysis.modules.iter().map(|m| m.node_count as u64).sum();
    let symbol_count: u64 = analysis.modules.iter().map(|m| m.defs.len() as u64).sum();

    let modules_json: Vec<serde_json::Value> = analysis
        .modules
        .iter()
        .map(|module| {
            let symbols: Vec<serde_json::Value> = module
                .defs
                .values()
                .map(|def| {
                    serde_json::json!({
                        "symbol": def.symbol,
                        "node_id": def.node_id,
                        "path_repr": def.symbol_path_repr,
                        "term_hash": def.term_hash,
                    })
                })
                .collect();
            serde_json::json!({
                "module_path": module.module_path,
                "symbol_count": module.defs.len(),
                "node_count": module.node_count,
                "symbols": symbols,
            })
        })
        .collect();

    let edges_json: Vec<serde_json::Value> = edge_counts
        .into_iter()
        .map(|((from_module, to_module, symbol), use_count)| {
            serde_json::json!({
                "from_module": from_module,
                "to_module": to_module,
                "symbol": symbol,
                "use_count": use_count,
            })
        })
        .collect();

    let mut stdout = String::new();
    if !cli.json {
        stdout.push_str(&format!(
            "modules={} symbols={} nodes={} edges={}\n",
            analysis.modules.len(),
            symbol_count,
            total_nodes,
            edges_json.len()
        ));
        for edge in &edges_json {
            let from = edge
                .get("from_module")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let to = edge
                .get("to_module")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let symbol = edge
                .get("symbol")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let use_count = edge.get("use_count").and_then(|v| v.as_u64()).unwrap_or(0);
            stdout.push_str(&format!("{from} -> {to} [{symbol}] x{use_count}\n"));
        }
    }

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/semantic-edit-workspace-graph-v0.1",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "pkg_dir": analysis.pkg_dir.display().to_string(),
            "coreform_frontend": frontend_info,
            "module_count": analysis.modules.len(),
            "symbol_count": symbol_count,
            "node_count": total_nodes,
            "edge_count": edges_json.len(),
            "duplicate_symbol_owners": duplicate_symbol_owners,
            "unresolved_symbols": unresolved_symbols.into_iter().collect::<Vec<_>>(),
            "modules": modules_json,
            "edges": edges_json,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout,
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_semantic_edit_refactor_plan(
    cli: &Cli,
    pkg: &Path,
    kind: RefactorKind,
    from_symbol: &str,
    to_symbol: &str,
    target_module_path: Option<&str>,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);
    let analysis = analyze_workspace(cli, pkg, &frontend)?;

    let mut conflicts = Vec::new();
    validate_refactor_symbols(from_symbol, to_symbol, &mut conflicts);

    let from_defs = find_definition_sites(&analysis, from_symbol);
    if from_defs.is_empty() {
        conflicts.push(RefactorConflict {
            code: "refactor/source-symbol-missing",
            message: format!("source symbol `{from_symbol}` is not defined in this package"),
            module_path: None,
            path_repr: None,
        });
    } else if from_defs.len() > 1 {
        conflicts.push(RefactorConflict {
            code: "refactor/source-symbol-ambiguous",
            message: format!(
                "source symbol `{from_symbol}` is defined in multiple modules: {}",
                from_defs
                    .iter()
                    .map(|d| d.module_path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            module_path: None,
            path_repr: None,
        });
    }

    let to_defs = find_definition_sites(&analysis, to_symbol);
    if !to_defs.is_empty() && from_symbol != to_symbol {
        conflicts.push(RefactorConflict {
            code: "refactor/destination-symbol-exists",
            message: format!("destination symbol `{to_symbol}` already exists"),
            module_path: to_defs.first().map(|d| d.module_path.clone()),
            path_repr: to_defs.first().map(|d| d.symbol_path_repr.clone()),
        });
    }

    let validated_target_module_path = match kind {
        RefactorKind::Rename => None,
        RefactorKind::Move | RefactorKind::Extract => match target_module_path {
            Some(path) => {
                if !conflicts.is_empty() {
                    None
                } else {
                    match validate_relative_module_path(path) {
                        Ok(()) => {
                            if analysis.modules.iter().any(|m| m.module_path == path) {
                                conflicts.push(RefactorConflict {
                                        code: "refactor/target-module-exists",
                                        message: format!(
                                            "target module `{path}` already exists; add-module would conflict"
                                        ),
                                        module_path: Some(path.to_string()),
                                        path_repr: None,
                                    });
                                None
                            } else {
                                Some(path.to_string())
                            }
                        }
                        Err(msg) => {
                            conflicts.push(RefactorConflict {
                                code: "refactor/target-module-invalid",
                                message: msg,
                                module_path: Some(path.to_string()),
                                path_repr: None,
                            });
                            None
                        }
                    }
                }
            }
            None => {
                conflicts.push(RefactorConflict {
                    code: "refactor/target-module-required",
                    message: "move/extract requires --target-module-path".to_string(),
                    module_path: None,
                    path_repr: None,
                });
                None
            }
        },
    };

    let mut planned_ops = Vec::new();
    let mut replacement_count = 0_u64;
    if conflicts.is_empty() {
        let Some(source_def) = from_defs.first().cloned() else {
            return Err(cli_err(
                EX_INTERNAL,
                "semantic-edit/refactor",
                "refactor source symbol resolution invariant failed".to_string(),
            ));
        };
        let mut replacements = collect_symbol_replacements(&analysis, from_symbol);
        replacements.retain(|occ| {
            !(matches!(kind, RefactorKind::Move | RefactorKind::Extract)
                && occ.module_path == source_def.module_path
                && occ.path_repr == source_def.symbol_path_repr)
        });
        replacement_count = replacements.len() as u64;

        match kind {
            RefactorKind::Rename => {
                for occ in replacements {
                    planned_ops.push(PlannedOp::ReplaceNode {
                        module_path: occ.module_path,
                        path: occ.path,
                        path_repr: occ.path_repr,
                        new_term: Term::symbol(to_symbol),
                    });
                }
            }
            RefactorKind::Move | RefactorKind::Extract => {
                let target_module = validated_target_module_path.clone().ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "semantic-edit/refactor",
                        "target module path missing after validation".to_string(),
                    )
                })?;
                let lifted_expr = replace_symbol_in_term(&source_def.expr, from_symbol, to_symbol);
                let lifted_def = make_def_form(to_symbol, lifted_expr);
                planned_ops.push(PlannedOp::AddModule {
                    module_path: target_module,
                    forms: vec![lifted_def],
                });
                planned_ops.push(PlannedOp::ReplaceNode {
                    module_path: source_def.module_path.clone(),
                    path: source_def.form_path.clone(),
                    path_repr: source_def.form_path_repr.clone(),
                    new_term: make_def_form(from_symbol, Term::symbol(to_symbol)),
                });
                for occ in replacements {
                    planned_ops.push(PlannedOp::ReplaceNode {
                        module_path: occ.module_path,
                        path: occ.path,
                        path_repr: occ.path_repr,
                        new_term: Term::symbol(to_symbol),
                    });
                }
            }
        }
    }

    let planned_ops = dedupe_replace_targets(planned_ops, &mut conflicts);
    let patch_term = patch_term_from_plan(kind, from_symbol, to_symbol, &planned_ops)?;
    gc_patches::validate_patch_term(&patch_term).map_err(map_patch_error)?;
    let patch_coreform = print_term(&patch_term);
    let patch_hash = hex32(gc_coreform::hash_term(&patch_term));
    let ops_json = planned_ops
        .iter()
        .map(|op| match op {
            PlannedOp::AddModule { module_path, forms } => serde_json::json!({
                "op": ":add-module",
                "module_path": module_path,
                "form_count": forms.len(),
                "forms_hash": hex32(gc_coreform::hash_term(&Term::Vector(forms.clone()))),
            }),
            PlannedOp::ReplaceNode {
                module_path,
                path_repr,
                new_term,
                ..
            } => serde_json::json!({
                "op": ":replace-node",
                "module_path": module_path,
                "path_repr": path_repr,
                "new_term_hash": hex32(gc_coreform::hash_term(new_term)),
                "new_term_tag": term_tag(new_term),
            }),
        })
        .collect::<Vec<_>>();

    let mut stdout = String::new();
    if !cli.json {
        if conflicts.is_empty() {
            stdout.push_str(&format!("{patch_coreform}\n"));
        } else {
            for conflict in &conflicts {
                stdout.push_str(&format!("{}: {}\n", conflict.code, conflict.message));
            }
        }
    }

    let conflict_json = conflicts
        .iter()
        .map(|c| {
            serde_json::json!({
                "code": c.code,
                "message": c.message,
                "module_path": c.module_path,
                "path_repr": c.path_repr,
            })
        })
        .collect::<Vec<_>>();

    let env = JsonEnvelope {
        ok: conflicts.is_empty(),
        kind: "genesis/semantic-edit-refactor-plan-v0.1",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "pkg_dir": analysis.pkg_dir.display().to_string(),
            "coreform_frontend": frontend_info,
            "kind": refactor_kind_token(kind),
            "from_symbol": from_symbol,
            "to_symbol": to_symbol,
            "target_module_path": validated_target_module_path,
            "module_count": analysis.modules.len(),
            "replacement_count": replacement_count,
            "op_count": planned_ops.len(),
            "safe_to_apply": conflicts.is_empty(),
            "conflicts": conflict_json,
            "patch_hash": patch_hash,
            "patch_coreform": patch_coreform,
            "ops": ops_json,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: if conflicts.is_empty() {
            EX_OK
        } else {
            EX_VERIFY
        },
        stdout,
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_semantic_edit_apply_plan(
    cli: &Cli,
    pkg: &Path,
    kind: RefactorKind,
    from_symbol: &str,
    to_symbol: &str,
    target_module_path: Option<&str>,
    caps: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let plan_out = cmd_semantic_edit_refactor_plan(
        cli,
        pkg,
        kind,
        from_symbol,
        to_symbol,
        target_module_path,
    )?;
    let plan_json = plan_out.json.clone();
    let plan_data = plan_json
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let safe_to_apply = plan_data
        .get("safe_to_apply")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let conflicts = plan_data
        .get("conflicts")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(vec![]));
    let patch_coreform = plan_data
        .get("patch_coreform")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let patch_hash = plan_data
        .get("patch_hash")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    if !safe_to_apply || patch_coreform.trim().is_empty() {
        let env = JsonEnvelope {
            ok: false,
            kind: "genesis/semantic-edit-apply-plan-v0.1",
            data: Some(serde_json::json!({
                "pkg": pkg.display().to_string(),
                "coreform_frontend": frontend_info,
                "safe_to_apply": false,
                "apply_status": "plan-conflicts",
                "patch_hash": patch_hash,
                "patch_coreform": patch_coreform,
                "conflicts": conflicts,
                "plan": plan_data,
            })),
            error: None,
        };
        return Ok(CmdOut {
            exit_code: EX_VERIFY,
            stdout: if cli.json {
                String::new()
            } else {
                plan_out.stdout
            },
            json: json_envelope_value(env)?,
        });
    }

    let patch_path = std::env::temp_dir().join(format!(
        "genesis-semantic-edit-apply-plan-{}-{}.gcpatch",
        std::process::id(),
        patch_hash
    ));
    std::fs::write(&patch_path, format!("{patch_coreform}\n"))
        .with_context(|| format!("write {}", patch_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let apply_result = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch_path,
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(map_patch_error);
    let _ = std::fs::remove_file(&patch_path);
    let r = apply_result?;

    let exit_code = if r.ok { EX_OK } else { EX_OBLIGATIONS };
    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/semantic-edit-apply-plan-v0.1",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "coreform_frontend": frontend_info,
            "safe_to_apply": true,
            "apply_status": if r.ok { "applied" } else { "obligations-failed" },
            "caps": caps.map(|p| p.display().to_string()),
            "patch_hash": patch_hash,
            "patch_coreform": patch_coreform,
            "patch_artifact": r.patch_artifact,
            "report_artifact": r.report_artifact,
            "acceptance_artifact": r.acceptance_artifact,
            "package_artifact": r.package_artifact,
            "plan": plan_data,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", r.report_artifact)
        },
        json: json_envelope_value(env)?,
    })
}

fn analyze_workspace(
    cli: &Cli,
    pkg: &Path,
    frontend: &gc_obligations::CoreformFrontend,
) -> Result<WorkspaceAnalysis, CliError> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "package/invalid", format!("{e}")))?;
    let mut modules = Vec::new();
    let mut module_paths = manifest
        .modules
        .iter()
        .map(|m| m.path.clone())
        .collect::<Vec<_>>();
    module_paths.sort();

    for module_path in module_paths {
        let module_abs = pkg_dir.join(&module_path);
        let src = std::fs::read_to_string(&module_abs)
            .with_context(|| format!("read {}", module_abs.display()))
            .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;

        let forms = gc_obligations::parse_canonicalize_module_source_with_frontend(
            &src,
            frontend,
            resolved_step_limit(cli),
            resolved_mem_limits(cli),
        )
        .map_err(obligation_err)?;

        let semantic_nodes = gc_patches::semantic_node_index_for_module_with_frontend(
            &module_path,
            &src,
            frontend,
            resolved_step_limit(cli),
            resolved_mem_limits(cli),
        )
        .map_err(map_patch_error)?;

        let mut semantic_node_lookup: BTreeMap<String, (&str, &str)> = BTreeMap::new();
        for node in &semantic_nodes {
            semantic_node_lookup.insert(node.path_repr.clone(), (&node.node_id, &node.term_hash));
        }

        let mut defs = BTreeMap::new();
        for (i, form) in forms.iter().enumerate() {
            let Some((symbol, expr)) = parse_def(form) else {
                continue;
            };
            let form_path = vec![PathStep::Form(i)];
            let symbol_path = vec![PathStep::Form(i), PathStep::PairCdr, PathStep::PairCar];
            let form_path_repr = print_term(&path_to_term(&form_path)?);
            let symbol_path_repr = print_term(&path_to_term(&symbol_path)?);
            let (node_id, term_hash) = semantic_node_lookup
                .get(symbol_path_repr.as_str())
                .map(|(n, h)| (Some((*n).to_string()), Some((*h).to_string())))
                .unwrap_or((None, None));

            defs.insert(
                symbol.clone(),
                DefinitionSite {
                    module_path: module_path.clone(),
                    symbol,
                    expr,
                    form_path,
                    form_path_repr,
                    symbol_path_repr,
                    node_id,
                    term_hash,
                },
            );
        }

        let mut occurrences = Vec::new();
        for (i, form) in forms.iter().enumerate() {
            let mut path = vec![PathStep::Form(i)];
            collect_symbol_occurrences(&module_path, &mut path, form, &mut occurrences)?;
        }
        modules.push(ModuleAnalysis {
            module_path,
            defs,
            occurrences,
            node_count: semantic_nodes.len(),
        });
    }

    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for module in &modules {
        for symbol in module.defs.keys() {
            owners
                .entry(symbol.clone())
                .or_default()
                .push(module.module_path.clone());
        }
    }

    Ok(WorkspaceAnalysis {
        pkg_dir,
        modules,
        owners,
    })
}

fn parse_def(form: &Term) -> Option<(String, Term)> {
    let items = form.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

fn collect_symbol_occurrences(
    module_path: &str,
    path: &mut Vec<PathStep>,
    term: &Term,
    out: &mut Vec<SymbolOccurrence>,
) -> Result<(), CliError> {
    match term {
        Term::Symbol(sym) => {
            if !sym.starts_with(':') {
                let path_repr = print_term(&path_to_term(path)?);
                out.push(SymbolOccurrence {
                    module_path: module_path.to_string(),
                    symbol: sym.clone(),
                    path: path.clone(),
                    path_repr,
                });
            }
        }
        Term::Pair(a, d) => {
            path.push(PathStep::PairCar);
            collect_symbol_occurrences(module_path, path, a, out)?;
            path.pop();
            path.push(PathStep::PairCdr);
            collect_symbol_occurrences(module_path, path, d, out)?;
            path.pop();
        }
        Term::Vector(xs) => {
            for (idx, child) in xs.iter().enumerate() {
                path.push(PathStep::Vec(idx));
                collect_symbol_occurrences(module_path, path, child, out)?;
                path.pop();
            }
        }
        Term::Map(map) => {
            for (k, v) in map {
                path.push(PathStep::Map(k.0.clone()));
                collect_symbol_occurrences(module_path, path, v, out)?;
                path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

fn find_definition_sites(analysis: &WorkspaceAnalysis, symbol: &str) -> Vec<DefinitionSite> {
    let mut out = Vec::new();
    for module in &analysis.modules {
        if let Some(def) = module.defs.get(symbol) {
            out.push(def.clone());
        }
    }
    out
}

fn collect_symbol_replacements(
    analysis: &WorkspaceAnalysis,
    from_symbol: &str,
) -> Vec<SymbolOccurrence> {
    let mut matches = Vec::new();
    for module in &analysis.modules {
        for occ in &module.occurrences {
            if occ.symbol == from_symbol {
                matches.push(occ.clone());
            }
        }
    }
    matches.sort_by(|a, b| {
        a.module_path
            .cmp(&b.module_path)
            .then(a.path_repr.cmp(&b.path_repr))
    });
    matches
}

fn dedupe_replace_targets(
    ops: Vec<PlannedOp>,
    conflicts: &mut Vec<RefactorConflict>,
) -> Vec<PlannedOp> {
    let mut seen_paths: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out = Vec::new();
    for op in ops {
        match &op {
            PlannedOp::ReplaceNode {
                module_path,
                path_repr,
                ..
            } => {
                let key = (module_path.clone(), path_repr.clone());
                if !seen_paths.insert(key.clone()) {
                    conflicts.push(RefactorConflict {
                        code: "refactor/duplicate-edit-target",
                        message: "multiple edits target the same node path".to_string(),
                        module_path: Some(key.0),
                        path_repr: Some(key.1),
                    });
                    continue;
                }
            }
            PlannedOp::AddModule { .. } => {}
        }
        out.push(op);
    }
    out
}

fn patch_term_from_plan(
    kind: RefactorKind,
    from_symbol: &str,
    to_symbol: &str,
    ops: &[PlannedOp],
) -> Result<Term, CliError> {
    let mut op_terms = Vec::new();
    let mut sorted_ops = ops.to_vec();
    sorted_ops.sort_by_key(op_sort_key);
    for op in sorted_ops {
        match op {
            PlannedOp::AddModule { module_path, forms } => {
                op_terms.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":op")), Term::symbol(":add-module")),
                        (
                            TermOrdKey(Term::symbol(":module-path")),
                            Term::Str(module_path),
                        ),
                        (TermOrdKey(Term::symbol(":content")), Term::Vector(forms)),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }
            PlannedOp::ReplaceNode {
                module_path,
                path,
                new_term,
                ..
            } => {
                let path_term = path_to_term(&path)?;
                op_terms.push(Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":op")),
                            Term::symbol(":replace-node"),
                        ),
                        (
                            TermOrdKey(Term::symbol(":module-path")),
                            Term::Str(module_path),
                        ),
                        (TermOrdKey(Term::symbol(":path")), path_term),
                        (TermOrdKey(Term::symbol(":new")), new_term),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }
        }
    }

    Ok(Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":version")),
                Term::Int(1_i64.into()),
            ),
            (
                TermOrdKey(Term::symbol(":intent")),
                Term::Str(format!("semantic-refactor/{}", refactor_kind_token(kind))),
            ),
            (
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":tool")),
                            Term::Str("genesis semantic-edit refactor-plan".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":kind")),
                            Term::symbol(refactor_kind_symbol(kind)),
                        ),
                        (TermOrdKey(Term::symbol(":from")), Term::symbol(from_symbol)),
                        (TermOrdKey(Term::symbol(":to")), Term::symbol(to_symbol)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":ops")), Term::Vector(op_terms)),
        ]
        .into_iter()
        .collect(),
    ))
}

fn op_sort_key(op: &PlannedOp) -> (u8, String, String) {
    match op {
        PlannedOp::AddModule { module_path, .. } => (0, module_path.clone(), String::new()),
        PlannedOp::ReplaceNode {
            module_path,
            path_repr,
            ..
        } => (1, module_path.clone(), path_repr.clone()),
    }
}

fn replace_symbol_in_term(term: &Term, from_symbol: &str, to_symbol: &str) -> Term {
    match term {
        Term::Symbol(sym) if sym == from_symbol => Term::symbol(to_symbol),
        Term::Pair(a, d) => Term::Pair(
            Box::new(replace_symbol_in_term(a, from_symbol, to_symbol)),
            Box::new(replace_symbol_in_term(d, from_symbol, to_symbol)),
        ),
        Term::Vector(xs) => Term::Vector(
            xs.iter()
                .map(|x| replace_symbol_in_term(x, from_symbol, to_symbol))
                .collect(),
        ),
        Term::Map(map) => Term::Map(
            map.iter()
                .map(|(k, v)| {
                    (
                        TermOrdKey(k.0.clone()),
                        replace_symbol_in_term(v, from_symbol, to_symbol),
                    )
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn make_def_form(symbol: &str, expr: Term) -> Term {
    proper_list(vec![Term::symbol("def"), Term::symbol(symbol), expr])
}

fn proper_list(items: Vec<Term>) -> Term {
    items.into_iter().rev().fold(Term::Nil, |tail, item| {
        Term::Pair(Box::new(item), Box::new(tail))
    })
}

fn validate_refactor_symbols(
    from_symbol: &str,
    to_symbol: &str,
    conflicts: &mut Vec<RefactorConflict>,
) {
    if from_symbol.trim().is_empty() {
        conflicts.push(RefactorConflict {
            code: "refactor/from-symbol-empty",
            message: "source symbol must be non-empty".to_string(),
            module_path: None,
            path_repr: None,
        });
    }
    if to_symbol.trim().is_empty() {
        conflicts.push(RefactorConflict {
            code: "refactor/to-symbol-empty",
            message: "destination symbol must be non-empty".to_string(),
            module_path: None,
            path_repr: None,
        });
    }
    if from_symbol.starts_with(':') || to_symbol.starts_with(':') {
        conflicts.push(RefactorConflict {
            code: "refactor/symbol-keyword-forbidden",
            message: "keyword symbols are not valid refactor targets".to_string(),
            module_path: None,
            path_repr: None,
        });
    }
    if from_symbol == to_symbol {
        conflicts.push(RefactorConflict {
            code: "refactor/no-op",
            message: "source and destination symbols are identical".to_string(),
            module_path: None,
            path_repr: None,
        });
    }
}

fn validate_relative_module_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("target module path must be non-empty".to_string());
    }
    if path.contains('\\') {
        return Err("target module path must use '/' separators".to_string());
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err("target module path must be relative".to_string());
    }
    for component in candidate.components() {
        match component {
            std::path::Component::Normal(_) => {}
            _ => {
                return Err(
                    "target module path must not contain '.', '..', or absolute components"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

fn map_patch_error(err: gc_patches::PatchError) -> CliError {
    match err {
        gc_patches::PatchError::Parse(_) | gc_patches::PatchError::Validate(_) => {
            cli_err(EX_PARSE, "semantic-edit/invalid", format!("{err}"))
        }
        gc_patches::PatchError::Io(_) => cli_err(EX_IO, "io/error", format!("{err}")),
        gc_patches::PatchError::Obligations(inner) => obligation_err(inner),
    }
}

fn path_to_term(path: &[PathStep]) -> Result<Term, CliError> {
    let mut steps = Vec::with_capacity(path.len());
    for step in path {
        let term = match step {
            PathStep::Form(i) => Term::Vector(vec![
                Term::symbol(":form"),
                Term::Int(
                    i64::try_from(*i)
                        .map_err(|_| {
                            cli_err(
                                EX_PARSE,
                                "semantic-edit/path",
                                "path index out of range".to_string(),
                            )
                        })?
                        .into(),
                ),
            ]),
            PathStep::PairCar => Term::Vector(vec![Term::symbol(":pair-car")]),
            PathStep::PairCdr => Term::Vector(vec![Term::symbol(":pair-cdr")]),
            PathStep::Vec(i) => Term::Vector(vec![
                Term::symbol(":vec"),
                Term::Int(
                    i64::try_from(*i)
                        .map_err(|_| {
                            cli_err(
                                EX_PARSE,
                                "semantic-edit/path",
                                "path index out of range".to_string(),
                            )
                        })?
                        .into(),
                ),
            ]),
            PathStep::Map(k) => Term::Vector(vec![Term::symbol(":map"), k.clone()]),
        };
        steps.push(term);
    }
    Ok(Term::Vector(steps))
}
