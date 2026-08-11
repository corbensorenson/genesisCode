use super::*;

#[path = "semantic_workspace_analysis.rs"]
mod semantic_workspace_analysis;
#[path = "semantic_workspace_contract.rs"]
mod semantic_workspace_contract;
#[path = "semantic_workspace_misc.rs"]
mod semantic_workspace_misc;
#[path = "semantic_workspace_plan.rs"]
mod semantic_workspace_plan;
#[path = "semantic_workspace_types.rs"]
mod semantic_workspace_types;
use semantic_workspace_analysis::analyze_workspace;
use semantic_workspace_contract::semantic_workspace_graph_model_from_contract;
use semantic_workspace_misc::refactor_kind_token;
use semantic_workspace_plan::map_patch_error;

pub(super) fn cmd_semantic_edit_workspace_graph(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);
    let analysis = analyze_workspace(cli, pkg, &frontend)?;
    let (duplicate_symbol_owners, edge_counts, unresolved_symbols) =
        semantic_workspace_graph_model_from_contract(cli, &analysis)?;

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
    let authority_artifact = resolved_selfhost_artifact_for_frontend(cli).ok_or_else(|| {
        cli_err(
            EX_VERIFY,
            "selfhost/artifact-required",
            "semantic refactor planning requires an artifact-loaded GenesisCode toolchain"
                .to_string(),
        )
    })?;
    let authority_frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(authority_artifact.clone()),
        });
    let modules = analysis
        .modules
        .iter()
        .map(|module| gc_patches::SemanticRefactorModule {
            module_path: module.module_path.clone(),
            forms: module.forms.clone(),
        })
        .collect::<Vec<_>>();
    let plan = gc_patches::plan_semantic_refactor_with_frontend(
        refactor_kind_token(kind),
        from_symbol,
        to_symbol,
        target_module_path.unwrap_or_default(),
        &modules,
        &authority_frontend,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
    )
    .map_err(map_patch_error)?;
    let conflicts = plan.conflicts;
    let patch_coreform = plan.patch.as_ref().map(print_term).unwrap_or_default();
    let patch_hash = plan.patch_hash;
    let ops_json = plan
        .patch
        .as_ref()
        .and_then(|patch| match patch {
            Term::Map(map) => map.get(&TermOrdKey(Term::symbol(":ops"))),
            _ => None,
        })
        .and_then(|ops| match ops {
            Term::Vector(ops) => Some(ops),
            _ => None,
        })
        .map(|ops| {
            ops.iter()
                .map(|op| {
                    let (op_name, module_path) = match op {
                        Term::Map(map) => {
                            let op_name = map
                                .get(&TermOrdKey(Term::symbol(":op")))
                                .and_then(|value| match value {
                                    Term::Symbol(value) => Some(value.as_str()),
                                    _ => None,
                                })
                                .unwrap_or(":unknown");
                            let module_path = map
                                .get(&TermOrdKey(Term::symbol(":module-path")))
                                .or_else(|| map.get(&TermOrdKey(Term::symbol(":to-module-path"))))
                                .and_then(|value| match value {
                                    Term::Str(value) => Some(value.as_str()),
                                    _ => None,
                                });
                            (op_name, module_path)
                        }
                        _ => (":unknown", None),
                    };
                    serde_json::json!({
                        "op": op_name,
                        "module_path": module_path,
                        "op_hash": hex32(gc_coreform::hash_term(op)),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut stdout = String::new();
    if !cli.json {
        if plan.safe_to_apply {
            stdout.push_str(&format!("{patch_coreform}\n"));
        } else {
            for conflict in &conflicts {
                stdout.push_str(&format!("{}: {}\n", conflict.code, conflict.message));
            }
        }
    }
    let conflict_json = conflicts
        .iter()
        .map(|conflict| {
            serde_json::json!({
                "code": conflict.code,
                "message": conflict.message,
                "module_path": conflict.module_path,
                "path_repr": conflict.path_repr,
            })
        })
        .collect::<Vec<_>>();
    let safe_to_apply = plan.safe_to_apply;
    let env = JsonEnvelope {
        ok: safe_to_apply,
        kind: "genesis/semantic-edit-refactor-plan-v0.1",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "pkg_dir": analysis.pkg_dir.display().to_string(),
            "coreform_frontend": frontend_info,
            "kind": refactor_kind_token(kind),
            "from_symbol": from_symbol,
            "to_symbol": to_symbol,
            "target_module_path": target_module_path,
            "module_count": plan.module_count,
            "replacement_count": plan.replacement_count,
            "op_count": plan.op_count,
            "safe_to_apply": safe_to_apply,
            "refactor_authority": {
                "name": "selfhost",
                "bootstrap_mode": "artifact-only",
                "artifact": authority_artifact.display().to_string(),
            },
            "conflicts": conflict_json,
            "patch_hash": patch_hash,
            "patch_coreform": patch_coreform,
            "ops": ops_json,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: if safe_to_apply { EX_OK } else { EX_VERIFY },
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
        crate::platform_process_id(),
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
