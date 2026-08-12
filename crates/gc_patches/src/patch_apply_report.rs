use super::*;
use crate::patch_semantic_diff::{closed_map, field, lower_hex64, string_field, usize_field};

const REQUEST_KIND: &str = "genesis/patch-apply-report-request-v0.1";
const REPORT_KIND: &str = "genesis/patch-apply-v0.3";
const PROFILE: &str = "genesis/patch-authority-v0.1";

fn report_error(message: impl Into<String>) -> PatchError {
    PatchError::Validate(format!("patch-apply-report: {}", message.into()))
}

fn op_identities_term(patch: &Patch) -> Term {
    Term::Vector(
        patch
            .op_hashes
            .iter()
            .enumerate()
            .map(|(ordinal, op_hash)| {
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":op-h")),
                            Term::Str(op_hash.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":ordinal")),
                            Term::Int((ordinal as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    )
}

fn semantic_edit_term(edit: &AppliedSemanticEdit) -> Result<Term, PatchError> {
    let mut map = BTreeMap::new();
    map.insert(TermOrdKey(Term::symbol(":op")), Term::symbol(edit.op));
    map.insert(
        TermOrdKey(Term::symbol(":module-path")),
        Term::Str(edit.module_path.clone()),
    );
    if let Some(node_id) = &edit.node_id {
        map.insert(
            TermOrdKey(Term::symbol(":node-id")),
            Term::Str(node_id.clone()),
        );
    }
    if let Some(path) = &edit.path {
        map.insert(TermOrdKey(Term::symbol(":path")), path_steps_to_term(path)?);
    }
    if let Some(new_term_hash) = &edit.new_term_hash {
        map.insert(
            TermOrdKey(Term::symbol(":new-term-h")),
            Term::Str(new_term_hash.clone()),
        );
    }
    if let Some(before_hash) = &edit.before_module_hash {
        map.insert(
            TermOrdKey(Term::symbol(":before-module-h")),
            Term::Str(before_hash.clone()),
        );
    }
    if let Some(after_hash) = &edit.after_module_hash {
        map.insert(
            TermOrdKey(Term::symbol(":after-module-h")),
            Term::Str(after_hash.clone()),
        );
    }
    if let Some(detail) = &edit.detail {
        map.insert(TermOrdKey(Term::symbol(":detail")), detail.clone());
    }
    Ok(Term::Map(map))
}

fn semantic_edits_term(edits: &[AppliedSemanticEdit]) -> Result<Term, PatchError> {
    edits
        .iter()
        .map(semantic_edit_term)
        .collect::<Result<Vec<_>, _>>()
        .map(Term::Vector)
}

fn acceptance_ok(term: &Term) -> Result<bool, PatchError> {
    let Term::Map(map) = term else {
        return Err(report_error("acceptance artifact must be a map"));
    };
    if map.get(&TermOrdKey(Term::symbol(":kind")))
        != Some(&Term::Str("genesis/acceptance-v0.2".to_string()))
    {
        return Err(report_error("acceptance artifact kind mismatch"));
    }
    match map.get(&TermOrdKey(Term::symbol(":ok"))) {
        Some(Term::Bool(ok)) => Ok(*ok),
        _ => Err(report_error("acceptance artifact :ok must be bool")),
    }
}

fn request_term(
    patch: &Patch,
    package_artifact: &str,
    acceptance_artifact: &str,
    acceptance: &Term,
    semantic_edits: Term,
) -> Term {
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":acceptance")), acceptance.clone()),
            (
                TermOrdKey(Term::symbol(":acceptance-artifact")),
                Term::Str(acceptance_artifact.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(package_artifact.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":patch")),
                patch.normalized_term.clone(),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(PROFILE.to_string()),
            ),
            (TermOrdKey(Term::symbol(":semantic-edits")), semantic_edits),
            (
                TermOrdKey(Term::symbol(":source-patch-h")),
                Term::Str(patch.source_hash.clone()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn decode_report(
    report: Term,
    request: &Term,
    patch: &Patch,
    package_artifact: &str,
    acceptance_artifact: &str,
    acceptance: &Term,
    semantic_edits: &Term,
) -> Result<bool, PatchError> {
    let map = closed_map(
        &report,
        "patch apply report",
        &[
            ":acceptance-artifact",
            ":intent",
            ":kind",
            ":ok",
            ":op-identities",
            ":ops-count",
            ":package-artifact",
            ":patch-h",
            ":profile",
            ":provenance",
            ":request-h",
            ":semantic-edits",
            ":source-patch-h",
            ":v",
        ],
    )?;
    let ok = acceptance_ok(acceptance)?;
    let request_hash = hash32_hex(hash_term(request));
    let report_request_hash = string_field(map, ":request-h", "patch apply report")?;
    if string_field(map, ":kind", "patch apply report")? != REPORT_KIND
        || string_field(map, ":profile", "patch apply report")? != PROFILE
        || field(map, ":v") != &Term::Int(1.into())
        || !lower_hex64(&report_request_hash)
        || report_request_hash != request_hash
        || field(map, ":ok") != &Term::Bool(ok)
        || field(map, ":intent") != &Term::Str(patch.intent.clone())
        || field(map, ":provenance") != &patch.provenance
        || usize_field(map, ":ops-count", "patch apply report")? != patch.ops.len()
        || field(map, ":op-identities") != &op_identities_term(patch)
        || string_field(map, ":patch-h", "patch apply report")? != patch.semantic_hash
        || string_field(map, ":source-patch-h", "patch apply report")? != patch.source_hash
        || string_field(map, ":package-artifact", "patch apply report")? != package_artifact
        || string_field(map, ":acceptance-artifact", "patch apply report")? != acceptance_artifact
        || field(map, ":semantic-edits") != semantic_edits
    {
        return Err(report_error("authority report facts mismatch"));
    }
    Ok(ok)
}

pub(super) fn selfhost_report_term(
    patch: &Patch,
    package_artifact: &str,
    acceptance: &PackageTestResult,
    acceptance_term: &Term,
    semantic_edits: &[AppliedSemanticEdit],
    toolchain: &mut SelfhostPatchToolchain,
    step_limit: StepLimit,
) -> Result<(bool, Term), PatchError> {
    if !lower_hex64(package_artifact)
        || !lower_hex64(&acceptance.acceptance_artifact)
        || !lower_hex64(&patch.source_hash)
    {
        return Err(report_error("artifact identity must be lowercase hex64"));
    }
    if acceptance_ok(acceptance_term)? != acceptance.ok {
        return Err(report_error(
            "acceptance result disagrees with its stored artifact",
        ));
    }
    let edits = semantic_edits_term(semantic_edits)?;
    let request = request_term(
        patch,
        package_artifact,
        &acceptance.acceptance_artifact,
        acceptance_term,
        edits.clone(),
    );
    let report = toolchain.patch_apply_report_term(request.clone(), step_limit)?;
    let ok = decode_report(
        report.clone(),
        &request,
        patch,
        package_artifact,
        &acceptance.acceptance_artifact,
        acceptance_term,
        &edits,
    )?;
    Ok((ok, report))
}

pub(super) fn stored_selfhost_report_term(
    patch: &Patch,
    package_artifact: &str,
    acceptance: &PackageTestResult,
    semantic_edits: &[AppliedSemanticEdit],
    store: &EvidenceStore,
    selfhost: Option<&mut SelfhostPatchToolchain>,
    step_limit: StepLimit,
    _mem_limits: MemLimits,
) -> Result<(bool, Term), PatchError> {
    store.verify_hex(&acceptance.acceptance_artifact)?;
    let acceptance_src = std::fs::read_to_string(store.path_for(&acceptance.acceptance_artifact))?;
    let acceptance_term =
        parse_term(&acceptance_src).map_err(|error| PatchError::Parse(error.to_string()))?;

    #[cfg(feature = "parity-oracle")]
    let mut parity_report_toolchain = if selfhost.is_none() {
        let config = gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc"),
            ),
        };
        Some(SelfhostPatchToolchain::init(&config, _mem_limits)?)
    } else {
        None
    };
    let report_toolchain = match selfhost {
        Some(toolchain) => toolchain,
        None => {
            #[cfg(feature = "parity-oracle")]
            {
                parity_report_toolchain
                    .as_mut()
                    .ok_or_else(|| report_error("parity report authority failed to initialize"))?
            }
            #[cfg(not(feature = "parity-oracle"))]
            return Err(report_error(
                "report authority requires the artifact-only selfhost toolchain",
            ));
        }
    };
    selfhost_report_term(
        patch,
        package_artifact,
        acceptance,
        &acceptance_term,
        semantic_edits,
        report_toolchain,
        step_limit,
    )
}
