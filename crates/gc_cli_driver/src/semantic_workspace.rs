use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[path = "semantic_workspace_analysis.rs"]
mod semantic_workspace_analysis;
#[path = "semantic_workspace_misc.rs"]
mod semantic_workspace_misc;
#[path = "semantic_workspace_plan.rs"]
mod semantic_workspace_plan;
#[path = "semantic_workspace_types.rs"]
mod semantic_workspace_types;
use semantic_workspace_analysis::analyze_workspace;
use semantic_workspace_misc::{
    looks_like_scoped_symbol, refactor_kind_symbol, refactor_kind_token, term_tag,
};
use semantic_workspace_plan::{
    collect_symbol_replacements, dedupe_replace_targets, find_definition_sites, make_def_form,
    map_patch_error, patch_term_from_plan, replace_symbol_in_term, validate_refactor_symbols,
    validate_relative_module_path,
};
use semantic_workspace_types::{PlannedOp, RefactorConflict};

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
