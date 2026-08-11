use std::collections::BTreeSet;

use super::*;
use crate::patch_protocol::extract_protocol_error;
use crate::patch_selfhost_toolchain::SelfhostPatchToolchain;
use crate::patch_semantic::term_tag;

const PATCH_AUTHORITY_PROFILE: &str = "genesis/patch-authority-v0.1";
const NODE_INDEX_REQUEST_KIND: &str = "genesis/patch-node-index-request-v0.1";
const NODE_INDEX_REPORT_KIND: &str = "genesis/patch-node-index-v0.1";
const PATCH_NORMALIZE_REQUEST_KIND: &str = "genesis/patch-normalize-request-v0.1";
const PATCH_NORMALIZE_REPORT_KIND: &str = "genesis/patch-normalize-v0.1";

fn authority_error(message: impl Into<String>) -> PatchError {
    PatchError::Validate(format!("patch-authority: {}", message.into()))
}

fn request_term(module_path: &str, forms: &[Term]) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":forms")),
                Term::Vector(forms.to_vec()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(NODE_INDEX_REQUEST_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(module_path.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PATCH_AUTHORITY_PROFILE.to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn normalize_request_term(patch: &Term) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(PATCH_NORMALIZE_REQUEST_KIND.to_string()),
            ),
            (TermOrdKey(Term::symbol(":patch")), patch.clone()),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PATCH_AUTHORITY_PROFILE.to_string()),
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
        return Err(authority_error(format!(
            "{context} must be a map, got {}",
            print_term(term)
        )));
    };
    if map.len() != fields.len()
        || fields
            .iter()
            .any(|field| !map.contains_key(&TermOrdKey(Term::symbol(*field))))
    {
        return Err(authority_error(format!(
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
        value => Err(authority_error(format!(
            "{context} {name} must be a string, got {}",
            print_term(value)
        ))),
    }
}

fn is_lower_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn exact_normalized_op_fields(op: &Term, ordinal: usize) -> Result<(), PatchError> {
    let Term::Map(map) = op else {
        return Err(authority_error(format!(
            "normalized patch :ops[{ordinal}] must be a map"
        )));
    };
    let op_symbol = match map.get(&TermOrdKey(Term::symbol(":op"))) {
        Some(Term::Symbol(value)) => value.as_str(),
        _ => {
            return Err(authority_error(format!(
                "normalized patch :ops[{ordinal}] :op must be a symbol"
            )));
        }
    };
    let fields: &[&str] = match op_symbol {
        ":replace-node" => &[":module-path", ":new", ":op", ":path"],
        ":replace-node-id" => &[":module-path", ":new", ":node-id", ":op"],
        ":add-module" => &[":content", ":module-path", ":op"],
        ":remove-module" => &[":module-path", ":op"],
        ":update-manifest" => &[
            ":caps-policy",
            ":obligations-add",
            ":obligations-remove",
            ":op",
            ":set",
            ":tests-add",
            ":tests-remove",
        ],
        ":rename-symbol" => &[":from", ":module-path", ":op", ":to"],
        ":move-module" => &[":from-module-path", ":op", ":to-module-path"],
        ":split-module" => &[":from-module-path", ":op", ":symbols", ":to-module-path"],
        ":rewrite-imports" | ":rewrite-exports" => {
            &[":add", ":module-path", ":op", ":remove", ":replace"]
        }
        ":migrate-contract-signature" => &[
            ":contract-symbol",
            ":from-param",
            ":module-path",
            ":op",
            ":to-param",
        ],
        other => {
            return Err(authority_error(format!(
                "normalized patch :ops[{ordinal}] has unknown op {other}"
            )));
        }
    };
    closed_map(op, &format!("normalized patch :ops[{ordinal}]"), fields)?;
    Ok(())
}

fn decode_normalize_report(report: Term, source_patch: &Term) -> Result<Patch, PatchError> {
    let map = closed_map(
        &report,
        "patch-normalize report",
        &[
            ":kind",
            ":normalized-patch",
            ":ok",
            ":op-identities",
            ":patch-h",
            ":profile",
            ":source-patch-h",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "patch-normalize report")? != PATCH_NORMALIZE_REPORT_KIND {
        return Err(authority_error("patch-normalize report :kind mismatch"));
    }
    if string_field(map, ":profile", "patch-normalize report")? != PATCH_AUTHORITY_PROFILE {
        return Err(authority_error("patch-normalize report :profile mismatch"));
    }
    if field(map, ":ok") != &Term::Bool(true) || field(map, ":v") != &Term::Int(1.into()) {
        return Err(authority_error("patch-normalize report :ok/:v mismatch"));
    }
    let source_hash = string_field(map, ":source-patch-h", "patch-normalize report")?;
    if source_hash != hash32_hex(hash_term(source_patch)) {
        return Err(authority_error(
            "patch-normalize report :source-patch-h mismatch",
        ));
    }
    let normalized = field(map, ":normalized-patch").clone();
    let normalized_map = closed_map(
        &normalized,
        "normalized patch",
        &[":intent", ":ops", ":provenance", ":version"],
    )?;
    if field(normalized_map, ":version") != &Term::Int(1.into()) {
        return Err(authority_error("normalized patch :version must equal 1"));
    }
    if !matches!(field(normalized_map, ":intent"), Term::Str(_))
        || !matches!(field(normalized_map, ":provenance"), Term::Map(_))
    {
        return Err(authority_error(
            "normalized patch intent/provenance types mismatch",
        ));
    }
    let Term::Vector(ops) = field(normalized_map, ":ops") else {
        return Err(authority_error("normalized patch :ops must be a vector"));
    };
    for (ordinal, op) in ops.iter().enumerate() {
        exact_normalized_op_fields(op, ordinal)?;
    }
    let patch_hash = string_field(map, ":patch-h", "patch-normalize report")?;
    if !is_lower_hex64(&patch_hash) || patch_hash != hash32_hex(hash_term(&normalized)) {
        return Err(authority_error("patch-normalize report :patch-h mismatch"));
    }
    let Term::Vector(identity_terms) = field(map, ":op-identities") else {
        return Err(authority_error(
            "patch-normalize report :op-identities must be a vector",
        ));
    };
    if identity_terms.len() != ops.len() {
        return Err(authority_error(
            "patch-normalize report operation identity count mismatch",
        ));
    }
    let mut op_hashes = Vec::with_capacity(ops.len());
    for (ordinal, (identity, op)) in identity_terms.iter().zip(ops).enumerate() {
        let context = format!("patch-normalize report :op-identities[{ordinal}]");
        let identity_map = closed_map(identity, &context, &[":op-h", ":ordinal"])?;
        let expected_ordinal =
            i64::try_from(ordinal).map_err(|_| authority_error("operation ordinal exceeds i64"))?;
        if field(identity_map, ":ordinal") != &Term::Int(expected_ordinal.into()) {
            return Err(authority_error(format!("{context} :ordinal mismatch")));
        }
        let op_hash = string_field(identity_map, ":op-h", &context)?;
        if !is_lower_hex64(&op_hash) || op_hash != hash32_hex(hash_term(op)) {
            return Err(authority_error(format!("{context} :op-h mismatch")));
        }
        op_hashes.push(op_hash);
    }
    let mut patch = Patch::from_term(&normalized)?;
    patch.normalized_term = normalized;
    patch.semantic_hash = patch_hash;
    patch.source_hash = source_hash;
    patch.op_hashes = op_hashes;
    Ok(patch)
}

fn collect_expected_nodes(
    path: &mut Vec<PathStep>,
    term: &Term,
    out: &mut Vec<(Term, String, String, String)>,
) -> Result<(), PatchError> {
    let path_term = path_steps_to_term(path)?;
    out.push((
        path_term.clone(),
        print_term(&path_term),
        term_tag(term).to_string(),
        hash32_hex(hash_term(term)),
    ));
    match term {
        Term::Pair(car, cdr) => {
            path.push(PathStep::PairCar);
            collect_expected_nodes(path, car, out)?;
            path.pop();
            path.push(PathStep::PairCdr);
            collect_expected_nodes(path, cdr, out)?;
            path.pop();
        }
        Term::Vector(values) => {
            for (index, child) in values.iter().enumerate() {
                path.push(PathStep::Vec(index));
                collect_expected_nodes(path, child, out)?;
                path.pop();
            }
        }
        Term::Map(entries) => {
            for (key, child) in entries {
                path.push(PathStep::Map(key.0.clone()));
                collect_expected_nodes(path, child, out)?;
                path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

fn expected_nodes(forms: &[Term]) -> Result<Vec<(Term, String, String, String)>, PatchError> {
    let mut out = Vec::new();
    for (index, form) in forms.iter().enumerate() {
        collect_expected_nodes(&mut vec![PathStep::Form(index)], form, &mut out)?;
    }
    Ok(out)
}

fn decode_report(
    report: Term,
    requested_module_path: &str,
    requested_forms: &[Term],
) -> Result<Vec<SemanticNodeRecord>, PatchError> {
    let map = closed_map(
        &report,
        "node-index report",
        &[
            ":kind",
            ":module-h",
            ":module-path",
            ":nodes",
            ":ok",
            ":profile",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "node-index report")? != NODE_INDEX_REPORT_KIND {
        return Err(authority_error("node-index report :kind mismatch"));
    }
    if string_field(map, ":profile", "node-index report")? != PATCH_AUTHORITY_PROFILE {
        return Err(authority_error("node-index report :profile mismatch"));
    }
    if field(map, ":v") != &Term::Int(1.into()) {
        return Err(authority_error("node-index report :v must equal 1"));
    }
    if field(map, ":ok") != &Term::Bool(true) {
        return Err(authority_error("node-index report :ok must be true"));
    }
    if string_field(map, ":module-path", "node-index report")? != requested_module_path {
        return Err(authority_error("node-index report :module-path mismatch"));
    }
    if string_field(map, ":module-h", "node-index report")?
        != hash32_hex(hash_module(requested_forms))
    {
        return Err(authority_error("node-index report :module-h mismatch"));
    }
    let Term::Vector(node_terms) = field(map, ":nodes") else {
        return Err(authority_error("node-index report :nodes must be a vector"));
    };
    let expected = expected_nodes(requested_forms)?;
    if node_terms.len() != expected.len() {
        return Err(authority_error(format!(
            "node-index report has {} nodes, expected {}",
            node_terms.len(),
            expected.len()
        )));
    }
    let mut node_ids = BTreeSet::new();
    let mut records = Vec::with_capacity(node_terms.len());
    for (ordinal, (node, expected)) in node_terms.iter().zip(expected.iter()).enumerate() {
        let context = format!("node-index report :nodes[{ordinal}]");
        let node_map = closed_map(
            node,
            &context,
            &[
                ":module-path",
                ":node-id",
                ":path",
                ":path-repr",
                ":term-h",
                ":term-tag",
            ],
        )?;
        let module_path = string_field(node_map, ":module-path", &context)?;
        let node_id = string_field(node_map, ":node-id", &context)?;
        let path = field(node_map, ":path").clone();
        let path_repr = string_field(node_map, ":path-repr", &context)?;
        let term_hash = string_field(node_map, ":term-h", &context)?;
        let term_tag = string_field(node_map, ":term-tag", &context)?;
        if module_path != requested_module_path {
            return Err(authority_error(format!("{context} :module-path mismatch")));
        }
        if node_id.len() != 64
            || !node_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !node_ids.insert(node_id.clone())
        {
            return Err(authority_error(format!(
                "{context} :node-id must be unique lowercase hex64"
            )));
        }
        if path != expected.0
            || path_repr != expected.1
            || term_tag != expected.2
            || term_hash != expected.3
        {
            return Err(authority_error(format!(
                "{context} disagrees with requested canonical term inventory"
            )));
        }
        records.push(SemanticNodeRecord {
            module_path,
            node_id,
            path,
            path_repr,
            term_tag,
            term_hash,
        });
    }
    Ok(records)
}

pub(super) fn selfhost_semantic_node_index(
    module_path: &str,
    forms: &[Term],
    config: &gc_obligations::SelfhostFrontendConfig,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<Vec<SemanticNodeRecord>, PatchError> {
    let mut ctx = EvalCtx::with_step_limit(None);
    ctx.set_mem_limits(mem_limits);
    let prelude = build_prelude(&mut ctx);
    let error_token = prelude.protocol.error;
    let mut env = prelude.env;
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut ctx,
        &mut env,
        config.bootstrap_mode,
        config.artifact.as_deref(),
    )
    .map_err(|error| authority_error(format!("selfhost/init: {error}")))?;
    ctx.steps = 0;
    ctx.step_limit = step_limit.resolve();
    let index = env
        .get("core/cli::patch-semantic-node-index")
        .ok_or_else(|| authority_error("missing binding core/cli::patch-semantic-node-index"))?;
    let value = index
        .apply(&mut ctx, Value::data(request_term(module_path, forms)))
        .map_err(|error| authority_error(format!("node-index apply: {error}")))?;
    if let Some(error) = extract_protocol_error(&value, error_token) {
        return Err(authority_error(format!("node-index failed: {error}")));
    }
    let report = value
        .as_data()
        .cloned()
        .unwrap_or_else(|| value.to_term_for_log(ctx.protocol.map(|protocol| protocol.error)));
    decode_report(report, module_path, forms)
}

pub(super) fn selfhost_normalize_patch(
    patch: &Term,
    toolchain: &mut SelfhostPatchToolchain,
    step_limit: StepLimit,
) -> Result<Patch, PatchError> {
    let report =
        toolchain.normalize_patch_report_term(normalize_request_term(patch), step_limit)?;
    decode_normalize_report(report, patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_rejects_unbound_or_incomplete_reports() {
        let forms = vec![Term::Vector(vec![Term::Int(1.into())])];
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str(NODE_INDEX_REPORT_KIND.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":module-h")),
                    Term::Str(hash32_hex(hash_module(&forms))),
                ),
                (
                    TermOrdKey(Term::symbol(":module-path")),
                    Term::Str("mod.gc".to_string()),
                ),
                (TermOrdKey(Term::symbol(":nodes")), Term::Vector(Vec::new())),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":profile")),
                    Term::Str(PATCH_AUTHORITY_PROFILE.to_string()),
                ),
                (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            ]
            .into_iter()
            .collect(),
        );
        let error = decode_report(report, "mod.gc", &forms).unwrap_err();
        assert!(error.to_string().contains("nodes, expected"));
    }
}
