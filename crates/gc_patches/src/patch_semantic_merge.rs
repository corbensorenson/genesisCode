use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::patch_semantic_diff::{
    PROFILE, SemanticWorkspaceDiff, SemanticWorkspaceModule, closed_map,
    decode_report as decode_diff_report, field, lower_hex64, request_term as diff_request_term,
    string_field, usize_field, workspace_map, workspace_term,
};

const REQUEST_KIND: &str = "genesis/patch-merge-request-v0.1";
const REPORT_KIND: &str = "genesis/patch-merge-v0.1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticWorkspaceConflict {
    pub base_hash: String,
    pub code: String,
    pub conflict_hash: String,
    pub explanation: String,
    pub form_index: Option<usize>,
    pub left_hash: String,
    pub module_path: String,
    pub right_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticWorkspaceMerge {
    pub conflicts: Vec<SemanticWorkspaceConflict>,
    pub diff: Option<SemanticWorkspaceDiff>,
    pub merged_hash: Option<String>,
    pub merged_modules: Option<Vec<SemanticWorkspaceModule>>,
}

#[derive(Clone, Debug)]
struct ExpectedMerge {
    conflicts: Vec<SemanticWorkspaceConflict>,
    conflict_terms: Vec<Term>,
    merged: Option<BTreeMap<String, Vec<Term>>>,
}

fn merge_error(message: impl Into<String>) -> PatchError {
    PatchError::Validate(format!("patch-merge: {}", message.into()))
}

fn conflict(
    code: &str,
    explanation: &str,
    module_path: &str,
    form_index: Option<usize>,
    base: Term,
    left: Term,
    right: Term,
) -> (SemanticWorkspaceConflict, Term) {
    let base_hash = hash32_hex(hash_term(&base));
    let left_hash = hash32_hex(hash_term(&left));
    let right_hash = hash32_hex(hash_term(&right));
    let mut conflict_map = [
        (
            TermOrdKey(Term::symbol(":base-h")),
            Term::Str(base_hash.clone()),
        ),
        (
            TermOrdKey(Term::symbol(":code")),
            Term::Str(code.to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":explanation")),
            Term::Str(explanation.to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":form-index")),
            form_index
                .map(|index| Term::Int((index as i64).into()))
                .unwrap_or(Term::Nil),
        ),
        (
            TermOrdKey(Term::symbol(":left-h")),
            Term::Str(left_hash.clone()),
        ),
        (
            TermOrdKey(Term::symbol(":module-path")),
            Term::Str(module_path.to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":right-h")),
            Term::Str(right_hash.clone()),
        ),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    let core = Term::Map(conflict_map.clone());
    let conflict_hash = hash32_hex(hash_term(&core));
    conflict_map.insert(
        TermOrdKey(Term::symbol(":conflict-h")),
        Term::Str(conflict_hash.clone()),
    );
    (
        SemanticWorkspaceConflict {
            base_hash,
            code: code.to_string(),
            conflict_hash,
            explanation: explanation.to_string(),
            form_index,
            left_hash,
            module_path: module_path.to_string(),
            right_hash,
        },
        Term::Map(conflict_map),
    )
}

fn push_conflict(
    conflicts: &mut Vec<SemanticWorkspaceConflict>,
    terms: &mut Vec<Term>,
    value: (SemanticWorkspaceConflict, Term),
) {
    conflicts.push(value.0);
    terms.push(value.1);
}

fn expected_merge(
    base: &BTreeMap<String, Vec<Term>>,
    left: &BTreeMap<String, Vec<Term>>,
    right: &BTreeMap<String, Vec<Term>>,
) -> ExpectedMerge {
    let paths = base
        .keys()
        .chain(left.keys())
        .chain(right.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut merged = BTreeMap::new();
    let mut conflicts = Vec::new();
    let mut conflict_terms = Vec::new();

    for path in paths {
        let base_forms = base.get(&path);
        let left_forms = left.get(&path);
        let right_forms = right.get(&path);
        let selected = if left_forms == right_forms {
            left_forms.cloned()
        } else if left_forms == base_forms {
            right_forms.cloned()
        } else if right_forms == base_forms {
            left_forms.cloned()
        } else {
            match (base_forms, left_forms, right_forms) {
                (None, Some(left_forms), Some(right_forms)) => {
                    push_conflict(
                        &mut conflicts,
                        &mut conflict_terms,
                        conflict(
                            "module/divergent-add",
                            "both branches added the same module path differently",
                            &path,
                            None,
                            Term::Nil,
                            Term::Vector(left_forms.clone()),
                            Term::Vector(right_forms.clone()),
                        ),
                    );
                    None
                }
                (Some(base_forms), None, Some(right_forms)) => {
                    push_conflict(
                        &mut conflicts,
                        &mut conflict_terms,
                        conflict(
                            "module/delete-modify",
                            "one branch deleted a module changed by the other branch",
                            &path,
                            None,
                            Term::Vector(base_forms.clone()),
                            Term::Nil,
                            Term::Vector(right_forms.clone()),
                        ),
                    );
                    None
                }
                (Some(base_forms), Some(left_forms), None) => {
                    push_conflict(
                        &mut conflicts,
                        &mut conflict_terms,
                        conflict(
                            "module/delete-modify",
                            "one branch deleted a module changed by the other branch",
                            &path,
                            None,
                            Term::Vector(base_forms.clone()),
                            Term::Vector(left_forms.clone()),
                            Term::Nil,
                        ),
                    );
                    None
                }
                (Some(base_forms), Some(left_forms), Some(right_forms))
                    if base_forms.len() == left_forms.len()
                        && base_forms.len() == right_forms.len() =>
                {
                    let mut forms = Vec::with_capacity(base_forms.len());
                    for (index, ((base_form, left_form), right_form)) in base_forms
                        .iter()
                        .zip(left_forms)
                        .zip(right_forms)
                        .enumerate()
                    {
                        if left_form == right_form {
                            forms.push(left_form.clone());
                        } else if left_form == base_form {
                            forms.push(right_form.clone());
                        } else if right_form == base_form {
                            forms.push(left_form.clone());
                        } else {
                            push_conflict(
                                &mut conflicts,
                                &mut conflict_terms,
                                conflict(
                                    "form/divergent-edit",
                                    "both branches changed the same top-level form differently",
                                    &path,
                                    Some(index),
                                    base_form.clone(),
                                    left_form.clone(),
                                    right_form.clone(),
                                ),
                            );
                            forms.push(base_form.clone());
                        }
                    }
                    Some(forms)
                }
                (Some(base_forms), Some(left_forms), Some(right_forms)) => {
                    push_conflict(
                        &mut conflicts,
                        &mut conflict_terms,
                        conflict(
                            "module/structural-divergence",
                            "both branches changed module structure incompatibly",
                            &path,
                            None,
                            Term::Vector(base_forms.clone()),
                            Term::Vector(left_forms.clone()),
                            Term::Vector(right_forms.clone()),
                        ),
                    );
                    None
                }
                _ => None,
            }
        };
        if let Some(forms) = selected {
            merged.insert(path, forms);
        }
    }

    ExpectedMerge {
        merged: conflicts.is_empty().then_some(merged),
        conflicts,
        conflict_terms,
    }
}

fn request_term(
    intent: &str,
    provenance: &Term,
    base: &BTreeMap<String, Vec<Term>>,
    left: &BTreeMap<String, Vec<Term>>,
    right: &BTreeMap<String, Vec<Term>>,
) -> Term {
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":base")), workspace_term(base)),
            (
                TermOrdKey(Term::symbol(":intent")),
                Term::Str(intent.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (TermOrdKey(Term::symbol(":left")), workspace_term(left)),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PROFILE.to_string()),
            ),
            (TermOrdKey(Term::symbol(":provenance")), provenance.clone()),
            (TermOrdKey(Term::symbol(":right")), workspace_term(right)),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn canonical_modules(modules: &BTreeMap<String, Vec<Term>>) -> Vec<SemanticWorkspaceModule> {
    modules
        .iter()
        .map(|(module_path, forms)| SemanticWorkspaceModule {
            module_path: module_path.clone(),
            forms: forms.clone(),
        })
        .collect()
}

fn decode_report(
    report: Term,
    request: &Term,
    intent: &str,
    provenance: &Term,
    base: &BTreeMap<String, Vec<Term>>,
    left: &BTreeMap<String, Vec<Term>>,
    right: &BTreeMap<String, Vec<Term>>,
) -> Result<SemanticWorkspaceMerge, PatchError> {
    let map = closed_map(
        &report,
        "merge report",
        &[
            ":base-h",
            ":conflict-count",
            ":conflicts",
            ":diff",
            ":kind",
            ":left-h",
            ":merged",
            ":merged-h",
            ":ok",
            ":profile",
            ":request-h",
            ":right-h",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "merge report")? != REPORT_KIND
        || string_field(map, ":profile", "merge report")? != PROFILE
        || field(map, ":v") != &Term::Int(1.into())
        || string_field(map, ":request-h", "merge report")? != hash32_hex(hash_term(request))
        || string_field(map, ":base-h", "merge report")?
            != hash32_hex(hash_term(&workspace_term(base)))
        || string_field(map, ":left-h", "merge report")?
            != hash32_hex(hash_term(&workspace_term(left)))
        || string_field(map, ":right-h", "merge report")?
            != hash32_hex(hash_term(&workspace_term(right)))
    {
        return Err(merge_error("report authority identity mismatch"));
    }

    let expected = expected_merge(base, left, right);
    if usize_field(map, ":conflict-count", "merge report")? != expected.conflicts.len()
        || field(map, ":conflicts") != &Term::Vector(expected.conflict_terms.clone())
    {
        return Err(merge_error(format!(
            "report conflicts do not match canonical merge: expected {}, got {}",
            print_term(&Term::Vector(expected.conflict_terms)),
            print_term(field(map, ":conflicts"))
        )));
    }

    let Some(merged) = expected.merged else {
        if field(map, ":ok") != &Term::Bool(false)
            || field(map, ":merged") != &Term::Nil
            || field(map, ":merged-h") != &Term::Nil
            || field(map, ":diff") != &Term::Nil
        {
            return Err(merge_error(
                "conflicted report must not contain a merge or diff",
            ));
        }
        return Ok(SemanticWorkspaceMerge {
            conflicts: expected.conflicts,
            diff: None,
            merged_hash: None,
            merged_modules: None,
        });
    };

    let merged_term = workspace_term(&merged);
    let merged_hash = string_field(map, ":merged-h", "merge report")?;
    if field(map, ":ok") != &Term::Bool(true)
        || field(map, ":merged") != &merged_term
        || !lower_hex64(&merged_hash)
        || merged_hash != hash32_hex(hash_term(&merged_term))
    {
        return Err(merge_error("successful report merged workspace mismatch"));
    }
    let diff_request = diff_request_term(intent, provenance, base, &merged);
    let diff = decode_diff_report(
        field(map, ":diff").clone(),
        &diff_request,
        intent,
        provenance,
        base,
        &merged,
    )?;
    Ok(SemanticWorkspaceMerge {
        conflicts: Vec::new(),
        diff: Some(diff),
        merged_hash: Some(merged_hash),
        merged_modules: Some(canonical_modules(&merged)),
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "semantic merge keeps intent, provenance, three workspaces, frontend, and resource bounds explicit"
)]
pub fn merge_semantic_workspaces_with_frontend(
    intent: &str,
    provenance: &Term,
    base_modules: &[SemanticWorkspaceModule],
    left_modules: &[SemanticWorkspaceModule],
    right_modules: &[SemanticWorkspaceModule],
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<SemanticWorkspaceMerge, PatchError> {
    let CoreformFrontend::Selfhost(config) = frontend else {
        return Err(merge_error(
            "GenesisCode merge authority requires an artifact-loaded selfhost frontend",
        ));
    };
    if config.bootstrap_mode != gc_prelude::SelfhostBootstrapMode::ArtifactOnly
        || config.artifact.is_none()
    {
        return Err(merge_error(
            "GenesisCode merge authority requires artifact-only bootstrap",
        ));
    }
    if !matches!(provenance, Term::Map(_)) {
        return Err(merge_error("provenance must be a map"));
    }
    let base = workspace_map(base_modules, "base workspace")?;
    let left = workspace_map(left_modules, "left workspace")?;
    let right = workspace_map(right_modules, "right workspace")?;
    let request = request_term(intent, provenance, &base, &left, &right);
    let mut toolchain = SelfhostPatchToolchain::init(config, mem_limits)?;
    let report = toolchain.patch_merge_report_term(request.clone(), step_limit)?;
    decode_report(report, &request, intent, provenance, &base, &left, &right)
}

#[cfg(test)]
#[path = "patch_semantic_merge_tests.rs"]
mod tests;
