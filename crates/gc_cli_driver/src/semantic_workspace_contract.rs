use super::semantic_workspace_types::WorkspaceAnalysis;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn term_map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, key: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn term_vec_strings(t: &Term, field: &str) -> Result<Vec<String>, CliError> {
    let Term::Vector(xs) = t else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/workspace-graph",
            format!("{field} must be vector"),
        ));
    };
    let mut out = Vec::with_capacity(xs.len());
    for x in xs {
        let Term::Str(s) = x else {
            return Err(cli_err(
                EX_INTERNAL,
                "semantic-edit/workspace-graph",
                format!("{field} items must be strings"),
            ));
        };
        out.push(s.clone());
    }
    Ok(out)
}

fn looks_like_scoped_symbol(symbol: &str) -> bool {
    symbol.contains("::") || symbol.contains('/')
}

fn semantic_workspace_graph_contract_payload(analysis: &WorkspaceAnalysis) -> Term {
    let owners = analysis
        .owners
        .iter()
        .map(|(symbol, module_paths)| {
            let mut module_paths_sorted = module_paths.clone();
            module_paths_sorted.sort();
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":symbol")),
                        Term::Str(symbol.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":module-paths")),
                        Term::Vector(module_paths_sorted.into_iter().map(Term::Str).collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect::<Vec<_>>();

    let mut occurrences = Vec::new();
    for module in &analysis.modules {
        for occ in &module.occurrences {
            if !looks_like_scoped_symbol(&occ.symbol) {
                continue;
            }
            occurrences.push(Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":module-path")),
                        Term::Str(module.module_path.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":symbol")),
                        Term::Str(occ.symbol.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
        }
    }

    Term::Map(
        [
            (TermOrdKey(Term::symbol(":owners")), Term::Vector(owners)),
            (
                TermOrdKey(Term::symbol(":occurrences")),
                Term::Vector(occurrences),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn semantic_workspace_graph_model_from_contract(
    cli: &Cli,
    analysis: &WorkspaceAnalysis,
) -> Result<
    (
        Vec<serde_json::Value>,
        BTreeMap<(String, String, String), u64>,
        BTreeSet<String>,
    ),
    CliError,
> {
    let mut ctx = EvalCtx::with_step_limit(resolved_step_limit(cli).resolve());
    ctx.set_mem_limits(resolved_mem_limits(cli));
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let payload = semantic_workspace_graph_contract_payload(analysis);

    let contract = env
        .get("core/cli::semantic-workspace-graph-analyze")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "semantic-edit/workspace-graph",
                "missing binding core/cli::semantic-workspace-graph-analyze".to_string(),
            )
        })?;
    let out = contract.apply(&mut ctx, Value::Data(payload)).map_err(|e| {
        cli_err(
            EX_EVAL,
            "semantic-edit/workspace-graph",
            format!("core/cli::semantic-workspace-graph-analyze failed: {e}"),
        )
    })?;
    let out_term = out.to_term_for_log(ctx.protocol.map(|p| p.error));
    let Term::Map(out_map) = out_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/workspace-graph",
            "workspace graph contract returned non-map".to_string(),
        ));
    };

    let duplicates_term = term_map_get(&out_map, ":duplicate-symbol-owners").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "semantic-edit/workspace-graph",
            "workspace graph contract missing :duplicate-symbol-owners".to_string(),
        )
    })?;
    let Term::Vector(duplicates_vec) = duplicates_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/workspace-graph",
            ":duplicate-symbol-owners must be vector".to_string(),
        ));
    };

    let mut duplicate_symbol_owners = Vec::with_capacity(duplicates_vec.len());
    for entry in duplicates_vec {
        let Term::Map(m) = entry else {
            return Err(cli_err(
                EX_INTERNAL,
                "semantic-edit/workspace-graph",
                "duplicate owner entry must be map".to_string(),
            ));
        };
        let symbol = match term_map_get(m, ":symbol") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/workspace-graph",
                    "duplicate owner entry missing :symbol string".to_string(),
                ));
            }
        };
        let module_paths = match term_map_get(m, ":module-paths") {
            Some(t) => term_vec_strings(t, ":module-paths")?,
            None => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/workspace-graph",
                    "duplicate owner entry missing :module-paths".to_string(),
                ));
            }
        };
        duplicate_symbol_owners.push(serde_json::json!({
            "symbol": symbol,
            "module_paths": module_paths,
        }));
    }

    let edge_events_term = term_map_get(&out_map, ":edge-events").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "semantic-edit/workspace-graph",
            "workspace graph contract missing :edge-events".to_string(),
        )
    })?;
    let Term::Vector(edge_events) = edge_events_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/workspace-graph",
            ":edge-events must be vector".to_string(),
        ));
    };

    let mut edge_counts: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    for edge in edge_events {
        let Term::Map(m) = edge else {
            return Err(cli_err(
                EX_INTERNAL,
                "semantic-edit/workspace-graph",
                "edge event must be map".to_string(),
            ));
        };
        let from_module = match term_map_get(m, ":from-module") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/workspace-graph",
                    "edge event missing :from-module string".to_string(),
                ));
            }
        };
        let to_module = match term_map_get(m, ":to-module") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/workspace-graph",
                    "edge event missing :to-module string".to_string(),
                ));
            }
        };
        let symbol = match term_map_get(m, ":symbol") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/workspace-graph",
                    "edge event missing :symbol string".to_string(),
                ));
            }
        };
        *edge_counts.entry((from_module, to_module, symbol)).or_insert(0) += 1;
    }

    let unresolved_term = term_map_get(&out_map, ":unresolved-symbols").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "semantic-edit/workspace-graph",
            "workspace graph contract missing :unresolved-symbols".to_string(),
        )
    })?;
    let unresolved_vec = term_vec_strings(unresolved_term, ":unresolved-symbols")?;
    let unresolved_symbols = unresolved_vec.into_iter().collect::<BTreeSet<_>>();

    Ok((duplicate_symbol_owners, edge_counts, unresolved_symbols))
}
