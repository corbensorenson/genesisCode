use super::semantic_workspace_types::{
    DefinitionSite, ModuleAnalysis, PathStep, SymbolOccurrence, WorkspaceAnalysis,
};
use super::*;
pub(super) fn analyze_workspace(
    cli: &Cli,
    pkg: &Path,
    frontend: &gc_obligations::CoreformFrontend,
) -> Result<WorkspaceAnalysis, CliError> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg)
        .map_err(|e| cli_err(EX_PARSE, "package/invalid", format!("{e}")))?;
    let mut modules = Vec::new();
    let module_paths = manifest
        .modules
        .iter()
        .map(|m| m.path.clone())
        .collect::<Vec<_>>();

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
            let Some(symbol) = parse_def(form) else {
                continue;
            };
            let symbol_path = vec![PathStep::Form(i), PathStep::PairCdr, PathStep::PairCar];
            let symbol_path_repr =
                print_term(&super::semantic_workspace_plan::path_to_term(&symbol_path)?);
            let (node_id, term_hash) = semantic_node_lookup
                .get(symbol_path_repr.as_str())
                .map(|(n, h)| (Some((*n).to_string()), Some((*h).to_string())))
                .unwrap_or((None, None));

            defs.insert(
                symbol.clone(),
                DefinitionSite {
                    symbol,
                    symbol_path_repr,
                    node_id,
                    term_hash,
                },
            );
        }

        let mut occurrences = Vec::new();
        for form in &forms {
            collect_symbol_occurrences(form, &mut occurrences);
        }
        modules.push(ModuleAnalysis {
            module_path,
            forms,
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

fn parse_def(form: &Term) -> Option<String> {
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
    Some(name.clone())
}

fn collect_symbol_occurrences(term: &Term, out: &mut Vec<SymbolOccurrence>) {
    match term {
        Term::Symbol(sym) => {
            if !sym.starts_with(':') {
                out.push(SymbolOccurrence {
                    symbol: sym.clone(),
                });
            }
        }
        Term::Pair(a, d) => {
            collect_symbol_occurrences(a, out);
            collect_symbol_occurrences(d, out);
        }
        Term::Vector(xs) => {
            for child in xs {
                collect_symbol_occurrences(child, out);
            }
        }
        Term::Map(map) => {
            for value in map.values() {
                collect_symbol_occurrences(value, out);
            }
        }
        _ => {}
    }
}
