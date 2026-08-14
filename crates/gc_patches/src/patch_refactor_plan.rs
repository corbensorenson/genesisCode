use std::collections::{BTreeMap, BTreeSet};

use super::*;

const REQUEST_KIND: &str = "genesis/refactor-plan-request-v0.1";
const REPORT_KIND: &str = "genesis/refactor-plan-v0.1";
const PROFILE: &str = "genesis/patch-authority-v0.1";

#[derive(Clone, Debug)]
pub struct SemanticRefactorModule {
    pub module_path: String,
    pub forms: Vec<Term>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticRefactorConflict {
    pub code: String,
    pub message: String,
    pub module_path: Option<String>,
    pub path_repr: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SemanticRefactorPlan {
    pub conflicts: Vec<SemanticRefactorConflict>,
    pub module_count: usize,
    pub op_count: usize,
    pub patch: Option<Term>,
    pub patch_hash: String,
    pub replacement_count: usize,
    pub safe_to_apply: bool,
}

fn plan_error(message: impl Into<String>) -> PatchError {
    PatchError::Validate(format!("refactor-plan: {}", message.into()))
}

fn request_term(
    kind: &str,
    from_symbol: &str,
    to_symbol: &str,
    target_module_path: &str,
    modules: &[SemanticRefactorModule],
) -> Term {
    let module_terms = modules
        .iter()
        .map(|module| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":forms")),
                        Term::Vector(module.forms.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":module-path")),
                        Term::Str(module.module_path.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":from-symbol")),
                Term::Str(from_symbol.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_terms),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PROFILE.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":refactor-kind")),
                Term::Str(kind.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":target-module-path")),
                Term::Str(target_module_path.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":to-symbol")),
                Term::Str(to_symbol.to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn closed_map<'a>(
    term: &'a Term,
    context: &str,
    fields: &[&str],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, PatchError> {
    let Term::Map(map) = term else {
        return Err(plan_error(format!("{context} must be a map")));
    };
    if map.len() != fields.len()
        || fields
            .iter()
            .any(|field| !map.contains_key(&TermOrdKey(Term::symbol(*field))))
    {
        return Err(plan_error(format!(
            "{context} must contain exactly fields [{}]",
            fields.join(", ")
        )));
    }
    Ok(map)
}

fn field<'a>(map: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> &'a Term {
    &map[&TermOrdKey(Term::symbol(name))]
}

fn string_field(
    map: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<String, PatchError> {
    match field(map, name) {
        Term::Str(value) => Ok(value.clone()),
        _ => Err(plan_error(format!("{context} {name} must be a string"))),
    }
}

fn usize_field(
    map: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<usize, PatchError> {
    match field(map, name) {
        Term::Int(value) => value
            .to_usize()
            .ok_or_else(|| plan_error(format!("{context} {name} is out of range"))),
        _ => Err(plan_error(format!("{context} {name} must be an int"))),
    }
}

fn bool_field(
    map: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<bool, PatchError> {
    match field(map, name) {
        Term::Bool(value) => Ok(*value),
        _ => Err(plan_error(format!("{context} {name} must be a bool"))),
    }
}

fn lower_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn decode_conflicts(term: &Term) -> Result<Vec<SemanticRefactorConflict>, PatchError> {
    let Term::Vector(entries) = term else {
        return Err(plan_error("report :conflicts must be a vector"));
    };
    let allowed = [
        "refactor/kind-invalid",
        "refactor/source-symbol-invalid",
        "refactor/destination-symbol-invalid",
        "refactor/no-op",
        "refactor/source-symbol-missing",
        "refactor/source-symbol-ambiguous",
        "refactor/destination-symbol-exists",
        "refactor/target-module-required",
        "refactor/target-module-invalid",
        "refactor/target-module-exists",
        "refactor/target-order-dependency",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let context = format!("report :conflicts[{index}]");
        let map = closed_map(
            entry,
            &context,
            &[":code", ":message", ":module-path", ":path-repr"],
        )?;
        let code = string_field(map, ":code", &context)?;
        let message = string_field(map, ":message", &context)?;
        let module_path = string_field(map, ":module-path", &context)?;
        let path_repr = string_field(map, ":path-repr", &context)?;
        if !allowed.contains(code.as_str()) || message.is_empty() {
            return Err(plan_error(format!("{context} code/message is invalid")));
        }
        if !seen.insert((code.clone(), module_path.clone(), path_repr.clone())) {
            return Err(plan_error(format!("{context} duplicates a conflict")));
        }
        out.push(SemanticRefactorConflict {
            code,
            message,
            module_path: (!module_path.is_empty()).then_some(module_path),
            path_repr: (!path_repr.is_empty()).then_some(path_repr),
        });
    }
    Ok(out)
}

fn verify_op_identities(term: &Term, ops: &[Term]) -> Result<(), PatchError> {
    let Term::Vector(entries) = term else {
        return Err(plan_error("report :op-identities must be a vector"));
    };
    if entries.len() != ops.len() {
        return Err(plan_error("report operation identity count mismatch"));
    }
    for (ordinal, (entry, op)) in entries.iter().zip(ops).enumerate() {
        let context = format!("report :op-identities[{ordinal}]");
        let map = closed_map(entry, &context, &[":op-h", ":ordinal"])?;
        if usize_field(map, ":ordinal", &context)? != ordinal {
            return Err(plan_error(format!("{context} ordinal mismatch")));
        }
        let op_hash = string_field(map, ":op-h", &context)?;
        if !lower_hex64(&op_hash) || op_hash != hash32_hex(hash_term(op)) {
            return Err(plan_error(format!("{context} hash mismatch")));
        }
    }
    Ok(())
}

fn symbol_occurrence_count(term: &Term, symbol: &str) -> Result<usize, PatchError> {
    let mut pending = vec![term];
    let mut count = 0usize;
    while let Some(next) = pending.pop() {
        match next {
            Term::Symbol(value) if value == symbol => {
                count = count
                    .checked_add(1)
                    .ok_or_else(|| plan_error("symbol occurrence count overflow"))?;
            }
            Term::Pair(car, cdr) => {
                pending.push(cdr);
                pending.push(car);
            }
            Term::Vector(values) => pending.extend(values.iter().rev()),
            Term::Map(values) => {
                for (key, value) in values.iter().rev() {
                    pending.push(value);
                    pending.push(&key.0);
                }
            }
            _ => {}
        }
    }
    Ok(count)
}

fn expected_affected_modules(
    modules: &[SemanticRefactorModule],
    from_symbol: &str,
) -> Result<(BTreeSet<String>, usize), PatchError> {
    let mut affected = BTreeSet::new();
    let mut replacement_count = 0usize;
    for module in modules {
        let mut module_count = 0usize;
        for form in &module.forms {
            module_count = module_count
                .checked_add(symbol_occurrence_count(form, from_symbol)?)
                .ok_or_else(|| plan_error("module occurrence count overflow"))?;
        }
        if module_count != 0 {
            affected.insert(module.module_path.clone());
            replacement_count = replacement_count
                .checked_add(module_count)
                .ok_or_else(|| plan_error("workspace occurrence count overflow"))?;
        }
    }
    Ok((affected, replacement_count))
}

fn top_level_definition_name(form: &Term) -> Option<&str> {
    let items = form.as_proper_list()?;
    match items.as_slice() {
        [Term::Symbol(head), Term::Symbol(name), _] if head == "def" => Some(name),
        _ => None,
    }
}

fn definition_modules<'a>(modules: &'a [SemanticRefactorModule], symbol: &str) -> Vec<&'a str> {
    modules
        .iter()
        .flat_map(|module| {
            module.forms.iter().filter_map(move |form| {
                (top_level_definition_name(form) == Some(symbol))
                    .then_some(module.module_path.as_str())
            })
        })
        .collect()
}

fn manifest_module_term(path: &str) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::Str("hash".to_string())),
                Term::Str(String::new()),
            ),
            (
                TermOrdKey(Term::Str("path".to_string())),
                Term::Str(path.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn expected_manifest_set(
    modules: &[SemanticRefactorModule],
    source_module_path: &str,
    target_module_path: &str,
) -> Term {
    let mut ordered = Vec::with_capacity(modules.len() + 1);
    for module in modules {
        if module.module_path == source_module_path {
            ordered.push(manifest_module_term(target_module_path));
        }
        ordered.push(manifest_module_term(&module.module_path));
    }
    Term::Map(
        [(TermOrdKey(Term::symbol(":modules")), Term::Vector(ordered))]
            .into_iter()
            .collect(),
    )
}

fn verify_operation_topology(
    patch: &Patch,
    kind: &str,
    from_symbol: &str,
    to_symbol: &str,
    modules: &[SemanticRefactorModule],
    target_module_path: &str,
    replacement_count: usize,
) -> Result<(), PatchError> {
    if !matches!(kind, "rename" | "move" | "extract")
        || from_symbol.is_empty()
        || to_symbol.is_empty()
        || from_symbol == to_symbol
    {
        return Err(plan_error("safe report violates refactor preconditions"));
    }
    let source_definitions = definition_modules(modules, from_symbol);
    let destination_definitions = definition_modules(modules, to_symbol);
    if source_definitions.len() != 1 || !destination_definitions.is_empty() {
        return Err(plan_error("safe report definition facts mismatch"));
    }
    let expected_source_module = source_definitions[0];
    let module_ordinals = modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.module_path.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let (expected_edits, expected_replacements) = expected_affected_modules(modules, from_symbol)?;
    if expected_edits.is_empty() || replacement_count != expected_replacements {
        return Err(plan_error("report replacement facts mismatch"));
    }
    let move_kind = matches!(kind, "move" | "extract");
    let mut edits = BTreeSet::new();
    let mut last_edit_ordinal = None;
    let mut split_count = 0;
    let mut manifest_count = 0;
    let mut source_module_path = None;
    for (ordinal, op) in patch.ops.iter().enumerate() {
        match op {
            PatchOp::RenameSymbol {
                module_path,
                from,
                to,
            } => {
                if split_count != 0 || manifest_count != 0 {
                    return Err(plan_error("report rename operations are out of order"));
                }
                let Some(module_ordinal) = module_ordinals.get(module_path.as_str()).copied()
                else {
                    return Err(plan_error("report contains an unknown module edit"));
                };
                if from != from_symbol
                    || to != to_symbol
                    || !edits.insert((module_path.clone(), ":rename-symbol"))
                    || last_edit_ordinal.is_some_and(|last| module_ordinal <= last)
                {
                    return Err(plan_error(
                        "report contains an invalid, duplicate, or unordered rename",
                    ));
                }
                last_edit_ordinal = Some(module_ordinal);
            }
            PatchOp::SplitModule {
                from_module_path,
                to_module_path,
                symbols,
            } => {
                split_count += 1;
                if !move_kind
                    || split_count != 1
                    || manifest_count != 0
                    || from_module_path != expected_source_module
                    || to_module_path != target_module_path
                    || module_ordinals.contains_key(to_module_path.as_str())
                    || symbols != &[to_symbol.to_string()]
                {
                    return Err(plan_error("report split-module topology mismatch"));
                }
                source_module_path = Some(from_module_path.as_str());
            }
            PatchOp::UpdateManifest {
                set,
                obligations_add,
                obligations_remove,
                tests_add,
                tests_remove,
                caps_policy,
            } => {
                manifest_count += 1;
                let expected_set = source_module_path
                    .map(|source| expected_manifest_set(modules, source, target_module_path));
                if !move_kind
                    || split_count != 1
                    || manifest_count != 1
                    || ordinal + 1 != patch.ops.len()
                    || set.as_ref() != expected_set.as_ref()
                    || !obligations_add.is_empty()
                    || !obligations_remove.is_empty()
                    || !tests_add.is_empty()
                    || !tests_remove.is_empty()
                    || caps_policy.is_some()
                {
                    return Err(plan_error("report manifest operation topology mismatch"));
                }
            }
            _ => {
                return Err(plan_error(
                    "report contains an unsupported refactor operation",
                ));
            }
        }
    }
    let actual_edits = edits
        .into_iter()
        .map(|(module_path, _)| module_path)
        .collect::<BTreeSet<_>>();
    if actual_edits != expected_edits {
        return Err(plan_error("report affected-module set mismatch"));
    }
    if !move_kind {
        if split_count != 0 || manifest_count != 0 {
            return Err(plan_error("rename report contains move operations"));
        }
    } else if split_count != 1 || manifest_count != 1 {
        return Err(plan_error(
            "move report is missing split/manifest operations",
        ));
    }
    Ok(())
}

fn decode_report(
    report: Term,
    request: &Term,
    kind: &str,
    from_symbol: &str,
    to_symbol: &str,
    modules: &[SemanticRefactorModule],
    target_module_path: &str,
) -> Result<SemanticRefactorPlan, PatchError> {
    let map = closed_map(
        &report,
        "report",
        &[
            ":conflicts",
            ":kind",
            ":module-count",
            ":ok",
            ":op-count",
            ":op-identities",
            ":patch",
            ":patch-h",
            ":profile",
            ":replacement-count",
            ":request-h",
            ":safe-to-apply",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "report")? != REPORT_KIND
        || string_field(map, ":profile", "report")? != PROFILE
        || field(map, ":v") != &Term::Int(1.into())
        || string_field(map, ":request-h", "report")? != hash32_hex(hash_term(request))
    {
        return Err(plan_error("report authority identity mismatch"));
    }
    let conflicts = decode_conflicts(field(map, ":conflicts"))?;
    let ok = bool_field(map, ":ok", "report")?;
    let safe = bool_field(map, ":safe-to-apply", "report")?;
    let module_count = usize_field(map, ":module-count", "report")?;
    let op_count = usize_field(map, ":op-count", "report")?;
    let replacement_count = usize_field(map, ":replacement-count", "report")?;
    if module_count != modules.len() || ok != safe || safe != conflicts.is_empty() {
        return Err(plan_error("report status/count facts mismatch"));
    }
    let patch_hash = string_field(map, ":patch-h", "report")?;
    if !safe {
        if field(map, ":patch") != &Term::Nil
            || !patch_hash.is_empty()
            || op_count != 0
            || replacement_count != 0
            || field(map, ":op-identities") != &Term::Vector(Vec::new())
        {
            return Err(plan_error("conflicted report carries patch authority"));
        }
        return Ok(SemanticRefactorPlan {
            conflicts,
            module_count,
            op_count,
            patch: None,
            patch_hash,
            replacement_count,
            safe_to_apply: false,
        });
    }

    let patch_term = field(map, ":patch").clone();
    let patch = Patch::from_term(&patch_term)?;
    let Term::Map(patch_map) = &patch_term else {
        return Err(plan_error("report patch must be a map"));
    };
    let Some(Term::Vector(op_terms)) = patch_map.get(&TermOrdKey(Term::symbol(":ops"))) else {
        return Err(plan_error("report patch :ops must be a vector"));
    };
    if !lower_hex64(&patch_hash) || patch_hash != hash32_hex(hash_term(&patch_term)) {
        return Err(plan_error("report patch hash mismatch"));
    }
    if op_count != patch.ops.len() || replacement_count == 0 {
        return Err(plan_error("report operation/replacement count mismatch"));
    }
    verify_op_identities(field(map, ":op-identities"), op_terms)?;
    verify_operation_topology(
        &patch,
        kind,
        from_symbol,
        to_symbol,
        modules,
        target_module_path,
        replacement_count,
    )?;
    Ok(SemanticRefactorPlan {
        conflicts,
        module_count,
        op_count,
        patch: Some(patch_term),
        patch_hash,
        replacement_count,
        safe_to_apply: true,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "semantic refactor planning keeps intent, symbol, workspace, frontend, and resource bounds explicit"
)]
pub fn plan_semantic_refactor_with_frontend(
    kind: &str,
    from_symbol: &str,
    to_symbol: &str,
    target_module_path: &str,
    modules: &[SemanticRefactorModule],
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<SemanticRefactorPlan, PatchError> {
    let CoreformFrontend::Selfhost(config) = frontend else {
        return Err(plan_error(
            "GenesisCode refactor authority requires an artifact-loaded selfhost frontend",
        ));
    };
    if config.bootstrap_mode != gc_prelude::SelfhostBootstrapMode::ArtifactOnly
        || config.artifact.is_none()
    {
        return Err(plan_error(
            "GenesisCode refactor authority requires artifact-only bootstrap",
        ));
    }
    let request = request_term(kind, from_symbol, to_symbol, target_module_path, modules);
    let mut toolchain = SelfhostPatchToolchain::init(config, mem_limits)?;
    let report = toolchain.refactor_plan_report_term(request.clone(), step_limit)?;
    decode_report(
        report,
        &request,
        kind,
        from_symbol,
        to_symbol,
        modules,
        target_module_path,
    )
}

#[cfg(test)]
#[path = "patch_refactor_plan_tests.rs"]
mod tests;
