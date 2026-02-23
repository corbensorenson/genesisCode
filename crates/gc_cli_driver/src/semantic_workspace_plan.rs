use super::*;
use super::semantic_workspace_types::{
    DefinitionSite, PathStep, PlannedOp, RefactorConflict, SymbolOccurrence, WorkspaceAnalysis,
};
pub(super) fn find_definition_sites(
    analysis: &WorkspaceAnalysis,
    symbol: &str,
) -> Vec<DefinitionSite> {
    let mut out = Vec::new();
    for module in &analysis.modules {
        if let Some(def) = module.defs.get(symbol) {
            out.push(def.clone());
        }
    }
    out
}

pub(super) fn collect_symbol_replacements(
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

pub(super) fn dedupe_replace_targets(
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

pub(super) fn patch_term_from_plan(
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

pub(super) fn replace_symbol_in_term(term: &Term, from_symbol: &str, to_symbol: &str) -> Term {
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

pub(super) fn make_def_form(symbol: &str, expr: Term) -> Term {
    proper_list(vec![Term::symbol("def"), Term::symbol(symbol), expr])
}

fn proper_list(items: Vec<Term>) -> Term {
    items.into_iter().rev().fold(Term::Nil, |tail, item| {
        Term::Pair(Box::new(item), Box::new(tail))
    })
}

pub(super) fn validate_refactor_symbols(
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

pub(super) fn validate_relative_module_path(path: &str) -> Result<(), String> {
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

pub(super) fn map_patch_error(err: gc_patches::PatchError) -> CliError {
    match err {
        gc_patches::PatchError::Parse(_) | gc_patches::PatchError::Validate(_) => {
            cli_err(EX_PARSE, "semantic-edit/invalid", format!("{err}"))
        }
        gc_patches::PatchError::Io(_) => cli_err(EX_IO, "io/error", format!("{err}")),
        gc_patches::PatchError::Obligations(inner) => obligation_err(inner),
    }
}

pub(super) fn path_to_term(path: &[PathStep]) -> Result<Term, CliError> {
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
