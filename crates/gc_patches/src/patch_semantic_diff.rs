use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(super) const REQUEST_KIND: &str = "genesis/patch-diff-request-v0.1";
const REPORT_KIND: &str = "genesis/patch-diff-v0.1";
pub(super) const PROFILE: &str = "genesis/patch-authority-v0.1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticWorkspaceModule {
    pub module_path: String,
    pub forms: Vec<Term>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticWorkspaceDiff {
    pub additions: usize,
    pub op_count: usize,
    pub patch: Term,
    pub patch_hash: String,
    pub removals: usize,
    pub replacements: usize,
}

fn diff_error(message: impl Into<String>) -> PatchError {
    PatchError::Validate(format!("patch-diff: {}", message.into()))
}

fn valid_module_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.starts_with('/')
        && !path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        && !path
            .split('/')
            .next()
            .is_some_and(|first| first.ends_with(':'))
        && matches!(
            gc_kernel::text_profile::normalize_nfc(path),
            Ok(normalized) if normalized == path
        )
}

pub(super) fn workspace_map(
    modules: &[SemanticWorkspaceModule],
    context: &str,
) -> Result<BTreeMap<String, Vec<Term>>, PatchError> {
    let mut out = BTreeMap::new();
    for module in modules {
        if !valid_module_path(&module.module_path) {
            return Err(diff_error(format!(
                "{context} module path must be portable and package-relative"
            )));
        }
        let canonical = canonicalize_module(module.forms.clone())
            .map_err(|error| diff_error(format!("{context} module is invalid: {error}")))?;
        if canonical != module.forms {
            return Err(diff_error(format!(
                "{context} module {} is not canonical",
                module.module_path
            )));
        }
        if out.insert(module.module_path.clone(), canonical).is_some() {
            return Err(diff_error(format!(
                "{context} contains duplicate module path {}",
                module.module_path
            )));
        }
    }
    Ok(out)
}

pub(super) fn workspace_term(modules: &BTreeMap<String, Vec<Term>>) -> Term {
    Term::Vector(
        modules
            .iter()
            .map(|(module_path, forms)| {
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":forms")),
                            Term::Vector(forms.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":module-path")),
                            Term::Str(module_path.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}

pub(super) fn request_term(
    intent: &str,
    provenance: &Term,
    base: &BTreeMap<String, Vec<Term>>,
    target: &BTreeMap<String, Vec<Term>>,
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
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PROFILE.to_string()),
            ),
            (TermOrdKey(Term::symbol(":provenance")), provenance.clone()),
            (TermOrdKey(Term::symbol(":target")), workspace_term(target)),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn closed_map<'a>(
    term: &'a Term,
    context: &str,
    fields: &[&str],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, PatchError> {
    let Term::Map(map) = term else {
        return Err(diff_error(format!("{context} must be a map")));
    };
    if map.len() != fields.len()
        || fields
            .iter()
            .any(|field| !map.contains_key(&TermOrdKey(Term::symbol(*field))))
    {
        return Err(diff_error(format!(
            "{context} must contain exactly fields [{}]",
            fields.join(", ")
        )));
    }
    Ok(map)
}

pub(super) fn field<'a>(map: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> &'a Term {
    &map[&TermOrdKey(Term::symbol(name))]
}

pub(super) fn string_field(
    map: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<String, PatchError> {
    match field(map, name) {
        Term::Str(value) => Ok(value.clone()),
        _ => Err(diff_error(format!("{context} {name} must be a string"))),
    }
}

pub(super) fn usize_field(
    map: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<usize, PatchError> {
    match field(map, name) {
        Term::Int(value) => value
            .to_usize()
            .ok_or_else(|| diff_error(format!("{context} {name} is out of range"))),
        _ => Err(diff_error(format!("{context} {name} must be an int"))),
    }
}

pub(super) fn lower_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn verify_op_identities(term: &Term, ops: &[Term]) -> Result<(), PatchError> {
    let Term::Vector(entries) = term else {
        return Err(diff_error("report :op-identities must be a vector"));
    };
    if entries.len() != ops.len() {
        return Err(diff_error("report operation identity count mismatch"));
    }
    for (ordinal, (entry, op)) in entries.iter().zip(ops).enumerate() {
        let context = format!("report :op-identities[{ordinal}]");
        let map = closed_map(entry, &context, &[":op-h", ":ordinal"])?;
        if usize_field(map, ":ordinal", &context)? != ordinal {
            return Err(diff_error(format!("{context} ordinal mismatch")));
        }
        let op_hash = string_field(map, ":op-h", &context)?;
        if !lower_hex64(&op_hash) || op_hash != hash32_hex(hash_term(op)) {
            return Err(diff_error(format!("{context} hash mismatch")));
        }
    }
    Ok(())
}

fn expected_replace(module_path: &str, index: usize, new_form: &Term) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(module_path.to_string()),
            ),
            (TermOrdKey(Term::symbol(":new")), new_form.clone()),
            (
                TermOrdKey(Term::symbol(":op")),
                Term::symbol(":replace-node"),
            ),
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Vector(vec![Term::Vector(vec![
                    Term::symbol(":form"),
                    Term::Int((index as i64).into()),
                ])]),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn expected_remove(module_path: &str) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(module_path.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":op")),
                Term::symbol(":remove-module"),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn expected_add(module_path: &str, forms: &[Term]) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":content")),
                Term::Vector(forms.to_vec()),
            ),
            (
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(module_path.to_string()),
            ),
            (TermOrdKey(Term::symbol(":op")), Term::symbol(":add-module")),
        ]
        .into_iter()
        .collect(),
    )
}

fn verify_minimal_top_form_diff(
    ops: &[Term],
    base: &BTreeMap<String, Vec<Term>>,
    target: &BTreeMap<String, Vec<Term>>,
) -> Result<(usize, usize, usize), PatchError> {
    let paths = base
        .keys()
        .chain(target.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut ordinal = 0usize;
    let mut replacements = 0usize;
    let mut additions = 0usize;
    let mut removals = 0usize;
    let mut expect = |term: Term, category: &mut usize| -> Result<(), PatchError> {
        if ops.get(ordinal) != Some(&term) {
            return Err(diff_error(format!(
                "report patch operation {ordinal} violates canonical minimal diff topology"
            )));
        }
        ordinal += 1;
        *category += 1;
        Ok(())
    };

    for path in paths {
        match (base.get(&path), target.get(&path)) {
            (Some(before), Some(after)) if before == after => {}
            (Some(before), Some(after)) if before.len() == after.len() => {
                for (index, (old_form, new_form)) in before.iter().zip(after).enumerate() {
                    if old_form != new_form {
                        expect(expected_replace(&path, index, new_form), &mut replacements)?;
                    }
                }
            }
            (Some(_), Some(after)) => {
                expect(expected_remove(&path), &mut removals)?;
                expect(expected_add(&path, after), &mut additions)?;
            }
            (Some(_), None) => expect(expected_remove(&path), &mut removals)?,
            (None, Some(after)) => expect(expected_add(&path, after), &mut additions)?,
            (None, None) => {}
        }
    }
    if ordinal != ops.len() {
        return Err(diff_error("report patch contains trailing operations"));
    }
    Ok((replacements, additions, removals))
}

pub(super) fn decode_report(
    report: Term,
    request: &Term,
    intent: &str,
    provenance: &Term,
    base: &BTreeMap<String, Vec<Term>>,
    target: &BTreeMap<String, Vec<Term>>,
) -> Result<SemanticWorkspaceDiff, PatchError> {
    let map = closed_map(
        &report,
        "report",
        &[
            ":base-h",
            ":kind",
            ":ok",
            ":op-count",
            ":op-identities",
            ":patch",
            ":patch-h",
            ":profile",
            ":request-h",
            ":stats",
            ":target-h",
            ":v",
        ],
    )?;
    let base_hash = hash32_hex(hash_term(&workspace_term(base)));
    let target_hash = hash32_hex(hash_term(&workspace_term(target)));
    if string_field(map, ":kind", "report")? != REPORT_KIND
        || string_field(map, ":profile", "report")? != PROFILE
        || field(map, ":v") != &Term::Int(1.into())
        || field(map, ":ok") != &Term::Bool(true)
        || string_field(map, ":request-h", "report")? != hash32_hex(hash_term(request))
        || string_field(map, ":base-h", "report")? != base_hash
        || string_field(map, ":target-h", "report")? != target_hash
    {
        return Err(diff_error("report authority identity mismatch"));
    }

    let patch_term = field(map, ":patch").clone();
    let patch = Patch::from_term(&patch_term)?;
    if patch.intent != intent || &patch.provenance != provenance {
        return Err(diff_error("report patch intent/provenance mismatch"));
    }
    let Term::Map(patch_map) = &patch_term else {
        return Err(diff_error("report patch must be a map"));
    };
    let Some(Term::Vector(ops)) = patch_map.get(&TermOrdKey(Term::symbol(":ops"))) else {
        return Err(diff_error("report patch :ops must be a vector"));
    };
    let patch_hash = string_field(map, ":patch-h", "report")?;
    if !lower_hex64(&patch_hash) || patch_hash != hash32_hex(hash_term(&patch_term)) {
        return Err(diff_error("report patch hash mismatch"));
    }
    if usize_field(map, ":op-count", "report")? != ops.len() || patch.ops.len() != ops.len() {
        return Err(diff_error("report operation count mismatch"));
    }
    verify_op_identities(field(map, ":op-identities"), ops)?;
    let (replacements, additions, removals) = verify_minimal_top_form_diff(ops, base, target)?;

    let stats = closed_map(
        field(map, ":stats"),
        "report :stats",
        &[":additions", ":removals", ":replacements"],
    )?;
    if usize_field(stats, ":replacements", "report :stats")? != replacements
        || usize_field(stats, ":additions", "report :stats")? != additions
        || usize_field(stats, ":removals", "report :stats")? != removals
    {
        return Err(diff_error("report statistics mismatch"));
    }

    Ok(SemanticWorkspaceDiff {
        additions,
        op_count: ops.len(),
        patch: patch_term,
        patch_hash,
        removals,
        replacements,
    })
}

pub fn diff_semantic_workspaces_with_frontend(
    intent: &str,
    provenance: &Term,
    base_modules: &[SemanticWorkspaceModule],
    target_modules: &[SemanticWorkspaceModule],
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<SemanticWorkspaceDiff, PatchError> {
    let CoreformFrontend::Selfhost(config) = frontend else {
        return Err(diff_error(
            "GenesisCode diff authority requires an artifact-loaded selfhost frontend",
        ));
    };
    if config.bootstrap_mode != gc_prelude::SelfhostBootstrapMode::ArtifactOnly
        || config.artifact.is_none()
    {
        return Err(diff_error(
            "GenesisCode diff authority requires artifact-only bootstrap",
        ));
    }
    if !matches!(provenance, Term::Map(_)) {
        return Err(diff_error("provenance must be a map"));
    }
    let base = workspace_map(base_modules, "base workspace")?;
    let target = workspace_map(target_modules, "target workspace")?;
    let request = request_term(intent, provenance, &base, &target);
    let mut toolchain = SelfhostPatchToolchain::init(config, mem_limits)?;
    let report = toolchain.patch_diff_report_term(request.clone(), step_limit)?;
    decode_report(report, &request, intent, provenance, &base, &target)
}

#[cfg(test)]
#[path = "patch_semantic_diff_tests.rs"]
mod tests;
