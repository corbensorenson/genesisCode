use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::PathBuf;

use super::*;
use crate::patch_selfhost_toolchain::SelfhostPatchToolchain;

const REQUEST_KIND: &str = "genesis/patch-preflight-request-v0.1";
const REPORT_KIND: &str = "genesis/patch-preflight-v0.1";
const PROFILE: &str = "genesis/patch-authority-v0.1";

fn preflight_error(message: impl Into<String>) -> PatchError {
    PatchError::Validate(format!("patch-preflight: {}", message.into()))
}

fn closed_map<'a>(
    term: &'a Term,
    context: &str,
    fields: &[&str],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, PatchError> {
    let Term::Map(map) = term else {
        return Err(preflight_error(format!("{context} must be a map")));
    };
    if map.len() != fields.len()
        || fields
            .iter()
            .any(|field| !map.contains_key(&TermOrdKey(Term::symbol(*field))))
    {
        return Err(preflight_error(format!(
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
        _ => Err(preflight_error(format!(
            "{context} {name} must be a string"
        ))),
    }
}

fn op_name(op: &PatchOp) -> &'static str {
    match op {
        PatchOp::ReplaceNode { .. } => ":replace-node",
        PatchOp::ReplaceNodeId { .. } => ":replace-node-id",
        PatchOp::AddModule { .. } => ":add-module",
        PatchOp::RemoveModule { .. } => ":remove-module",
        PatchOp::UpdateManifest { .. } => ":update-manifest",
        PatchOp::RenameSymbol { .. } => ":rename-symbol",
        PatchOp::MoveModule { .. } => ":move-module",
        PatchOp::SplitModule { .. } => ":split-module",
        PatchOp::RewriteMetaList { field, .. } => field.op_symbol(),
        PatchOp::MigrateContractSignature { .. } => ":migrate-contract-signature",
    }
}

fn op_paths(op: &PatchOp) -> Vec<&str> {
    match op {
        PatchOp::ReplaceNode { module_path, .. }
        | PatchOp::ReplaceNodeId { module_path, .. }
        | PatchOp::AddModule { module_path, .. }
        | PatchOp::RemoveModule { module_path }
        | PatchOp::RenameSymbol { module_path, .. }
        | PatchOp::RewriteMetaList { module_path, .. }
        | PatchOp::MigrateContractSignature { module_path, .. } => vec![module_path],
        PatchOp::MoveModule {
            from_module_path,
            to_module_path,
        }
        | PatchOp::SplitModule {
            from_module_path,
            to_module_path,
            ..
        } => vec![from_module_path, to_module_path],
        PatchOp::UpdateManifest { .. } => Vec::new(),
    }
}

fn classify_path(pkg_dir: &Path, relative: &str) -> Result<&'static str, PatchError> {
    let mut current = PathBuf::from(pkg_dir);
    let components = relative.split('/').collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok("absent"),
            Err(error) => return Err(PatchError::Io(error)),
        };
        if metadata.file_type().is_symlink() {
            return Err(preflight_error(format!(
                "symlink path component denied: {relative}"
            )));
        }
        if index + 1 < components.len() && !metadata.is_dir() {
            return Ok("other");
        }
        if index + 1 == components.len() {
            return Ok(if metadata.is_file() { "file" } else { "other" });
        }
    }
    Ok("other")
}

fn path_states(pkg_dir: &Path, patch: &Patch) -> Result<Term, PatchError> {
    let paths = patch
        .ops
        .iter()
        .flat_map(op_paths)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut records = Vec::with_capacity(paths.len());
    for path in paths {
        records.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                (
                    TermOrdKey(Term::symbol(":state")),
                    Term::Str(classify_path(pkg_dir, &path)?.to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }
    Ok(Term::Vector(records))
}

fn request_term(patch: &Patch, states: &Term) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":patch")),
                patch.normalized_term.clone(),
            ),
            (TermOrdKey(Term::symbol(":path-states")), states.clone()),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PROFILE.to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

#[derive(Clone, Copy)]
struct ExpectedCheck<'a> {
    ordinal: usize,
    op: &'static str,
    path: &'a str,
    expected: &'static str,
}

fn expected_checks(patch: &Patch) -> Vec<ExpectedCheck<'_>> {
    let mut checks = Vec::new();
    for (ordinal, op) in patch.ops.iter().enumerate() {
        let op_symbol = op_name(op);
        match op {
            PatchOp::AddModule { module_path, .. } => checks.push(ExpectedCheck {
                ordinal,
                op: op_symbol,
                path: module_path,
                expected: "absent",
            }),
            PatchOp::MoveModule {
                from_module_path,
                to_module_path,
            }
            | PatchOp::SplitModule {
                from_module_path,
                to_module_path,
                ..
            } => {
                checks.push(ExpectedCheck {
                    ordinal,
                    op: op_symbol,
                    path: from_module_path,
                    expected: "file",
                });
                checks.push(ExpectedCheck {
                    ordinal,
                    op: op_symbol,
                    path: to_module_path,
                    expected: "absent",
                });
            }
            PatchOp::UpdateManifest { .. } => {}
            _ => checks.push(ExpectedCheck {
                ordinal,
                op: op_symbol,
                path: op_paths(op)[0],
                expected: "file",
            }),
        }
    }
    checks
}

fn decode_check(
    term: &Term,
    context: &str,
    expected: ExpectedCheck<'_>,
    conflict: bool,
) -> Result<String, PatchError> {
    let fields: &[&str] = if conflict {
        &[":actual", ":code", ":expected", ":op", ":ordinal", ":path"]
    } else {
        &[":actual", ":expected", ":op", ":ordinal", ":path"]
    };
    let map = closed_map(term, context, fields)?;
    let ordinal = match field(map, ":ordinal") {
        Term::Int(value) => value
            .to_usize()
            .ok_or_else(|| preflight_error(format!("{context} :ordinal out of range")))?,
        _ => return Err(preflight_error(format!("{context} :ordinal must be int"))),
    };
    let op = match field(map, ":op") {
        Term::Symbol(value) => value.as_str(),
        _ => return Err(preflight_error(format!("{context} :op must be symbol"))),
    };
    if ordinal != expected.ordinal
        || op != expected.op
        || string_field(map, ":path", context)? != expected.path
        || string_field(map, ":expected", context)? != expected.expected
    {
        return Err(preflight_error(format!(
            "{context} does not match patch operation"
        )));
    }
    let actual = string_field(map, ":actual", context)?;
    if !matches!(actual.as_str(), "absent" | "file" | "other" | "unbound") {
        return Err(preflight_error(format!(
            "{context} :actual state is invalid"
        )));
    }
    if conflict {
        if string_field(map, ":code", context)? != "patch/path-state-conflict"
            || actual == expected.expected
        {
            return Err(preflight_error(format!(
                "{context} conflict facts mismatch"
            )));
        }
    } else if actual != expected.expected {
        return Err(preflight_error(format!(
            "{context} successful check facts mismatch"
        )));
    }
    Ok(actual)
}

fn expected_final_states(
    patch: &Patch,
    states: &Term,
    completed_checks: usize,
) -> Result<BTreeMap<String, String>, PatchError> {
    let Term::Vector(input) = states else {
        return Err(preflight_error("internal path states must be vector"));
    };
    let mut expected = BTreeMap::new();
    for (index, record) in input.iter().enumerate() {
        let context = format!("internal path states[{index}]");
        let map = closed_map(record, &context, &[":path", ":state"])?;
        expected.insert(
            string_field(map, ":path", &context)?,
            string_field(map, ":state", &context)?,
        );
    }

    let mut consumed_checks = 0;
    for op in &patch.ops {
        let op_check_count = match op {
            PatchOp::UpdateManifest { .. } => 0,
            PatchOp::MoveModule { .. } | PatchOp::SplitModule { .. } => 2,
            _ => 1,
        };
        if consumed_checks + op_check_count > completed_checks {
            break;
        }
        consumed_checks += op_check_count;
        match op {
            PatchOp::AddModule { module_path, .. } => {
                expected.insert(module_path.clone(), "file".to_string());
            }
            PatchOp::RemoveModule { module_path } => {
                expected.insert(module_path.clone(), "absent".to_string());
            }
            PatchOp::MoveModule {
                from_module_path,
                to_module_path,
            } => {
                expected.insert(from_module_path.clone(), "absent".to_string());
                expected.insert(to_module_path.clone(), "file".to_string());
            }
            PatchOp::SplitModule { to_module_path, .. } => {
                expected.insert(to_module_path.clone(), "file".to_string());
            }
            _ => {}
        }
    }
    Ok(expected)
}

fn decode_final_states(term: &Term, expected: &BTreeMap<String, String>) -> Result<(), PatchError> {
    let Term::Vector(final_states) = term else {
        return Err(preflight_error("report :final-path-states must be vector"));
    };
    if final_states.len() != expected.len() {
        return Err(preflight_error("report final path state count mismatch"));
    }
    for (index, ((expected_path, expected_state), record)) in
        expected.iter().zip(final_states).enumerate()
    {
        let context = format!("report :final-path-states[{index}]");
        let map = closed_map(record, &context, &[":path", ":state"])?;
        let path = string_field(map, ":path", &context)?;
        let state = string_field(map, ":state", &context)?;
        if path != *expected_path || state != *expected_state {
            return Err(preflight_error(format!("{context} facts mismatch")));
        }
    }
    Ok(())
}

fn decode_report(report: Term, patch: &Patch, states: &Term) -> Result<(), PatchError> {
    let map = closed_map(
        &report,
        "report",
        &[
            ":checks",
            ":conflict",
            ":final-path-states",
            ":kind",
            ":ok",
            ":patch-h",
            ":path-states-h",
            ":profile",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "report")? != REPORT_KIND
        || string_field(map, ":profile", "report")? != PROFILE
        || field(map, ":v") != &Term::Int(1.into())
        || string_field(map, ":patch-h", "report")? != patch.semantic_hash
        || string_field(map, ":path-states-h", "report")? != hash32_hex(hash_term(states))
    {
        return Err(preflight_error("report identity mismatch"));
    }
    let ok = match field(map, ":ok") {
        Term::Bool(value) => *value,
        _ => return Err(preflight_error("report :ok must be bool")),
    };
    let Term::Vector(checks) = field(map, ":checks") else {
        return Err(preflight_error("report :checks must be vector"));
    };
    let expected = expected_checks(patch);
    if checks.len() > expected.len() {
        return Err(preflight_error("report has excess checks"));
    }
    for (index, check) in checks.iter().enumerate() {
        decode_check(
            check,
            &format!("report :checks[{index}]"),
            expected[index],
            false,
        )?;
    }
    let expected_final = expected_final_states(patch, states, checks.len())?;
    decode_final_states(field(map, ":final-path-states"), &expected_final)?;
    if ok {
        if checks.len() != expected.len() || field(map, ":conflict") != &Term::Nil {
            return Err(preflight_error("successful report is incomplete"));
        }
        return Ok(());
    }
    if checks.len() >= expected.len() {
        return Err(preflight_error("failed report has no unresolved check"));
    }
    let conflict = field(map, ":conflict");
    let actual = decode_check(conflict, "report :conflict", expected[checks.len()], true)?;
    let conflict_map = closed_map(
        conflict,
        "report :conflict",
        &[":actual", ":code", ":expected", ":op", ":ordinal", ":path"],
    )?;
    Err(preflight_error(format!(
        "conflict code={} ordinal={} op={} path={} expected={} actual={actual}",
        string_field(conflict_map, ":code", "report :conflict")?,
        expected[checks.len()].ordinal,
        expected[checks.len()].op,
        expected[checks.len()].path,
        expected[checks.len()].expected,
    )))
}

pub(super) fn selfhost_preflight_patch(
    patch: &Patch,
    pkg_dir: &Path,
    toolchain: &mut SelfhostPatchToolchain,
    step_limit: StepLimit,
) -> Result<(), PatchError> {
    let states = path_states(pkg_dir, patch)?;
    let report = toolchain.preflight_report_term(request_term(patch, &states), step_limit)?;
    decode_report(report, patch, &states)
}
