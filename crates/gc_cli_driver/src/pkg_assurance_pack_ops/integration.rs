use std::collections::BTreeSet;
use std::path::PathBuf;

use gc_coreform::{Term, TermOrdKey};
use serde_json::Value as JsonValue;

use super::profile::AssuranceProfile;

const CROSSWALK_KIND: &str = "genesis/assurance-standards-crosswalk-v0.1";
const INTEGRATION_CONTRACT: &str = "genesis/assurance-external-control-bindings-v0.1";

fn map_term<const N: usize>(pairs: [(TermOrdKey, Term); N]) -> Term {
    Term::Map(pairs.into_iter().collect())
}

fn str_field<'a>(obj: &'a JsonValue, key: &str, ctx: &str) -> Result<&'a str, String> {
    obj.get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{ctx}: missing string field `{key}`"))
}

fn str_vec_field(obj: &JsonValue, key: &str, ctx: &str) -> Result<Vec<String>, String> {
    let values = obj
        .get(key)
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{ctx}: missing array field `{key}`"))?;
    values
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("{ctx}: `{key}` must contain only strings"))
        })
        .collect()
}

fn crosswalk_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json")
}

fn workflow_target_for_standard_family(standard_family: &str) -> &'static str {
    if standard_family.contains("IEC 62304") {
        "qms/device-regulator-workflow"
    } else if standard_family.contains("NASA") {
        "mission-assurance/governance-workflow"
    } else {
        "airworthiness/regulator-workflow"
    }
}

pub(super) fn build_external_control_bindings(
    assurance_profile: AssuranceProfile,
    trace_artifact: &str,
    qualification_artifact: &str,
    coverage_artifacts: &[String],
    object_equivalence_artifact: Option<&str>,
    independent_verifier_run_artifacts: &[String],
) -> Result<Term, String> {
    let crosswalk_path = crosswalk_path();
    let crosswalk_src = std::fs::read_to_string(&crosswalk_path)
        .map_err(|e| format!("read {}: {e}", crosswalk_path.display()))?;
    let crosswalk: JsonValue = serde_json::from_str(&crosswalk_src)
        .map_err(|e| format!("parse {}: {e}", crosswalk_path.display()))?;
    let kind = str_field(&crosswalk, "kind", "crosswalk root")?;
    if kind != CROSSWALK_KIND {
        return Err(format!(
            "crosswalk kind mismatch in {}: expected `{CROSSWALK_KIND}`, found `{kind}`",
            crosswalk_path.display()
        ));
    }
    let version = str_field(&crosswalk, "version", "crosswalk root")?;
    let profiles = crosswalk
        .get("profiles")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| "crosswalk root: missing `profiles` array".to_string())?;
    let profile_entry = profiles
        .iter()
        .find(|entry| {
            entry.get("assurance_profile").and_then(JsonValue::as_str)
                == Some(assurance_profile.as_name())
        })
        .ok_or_else(|| {
            format!(
                "crosswalk missing profile `{}` in {}",
                assurance_profile.as_name(),
                crosswalk_path.display()
            )
        })?;
    let profile_ctx = format!("crosswalk profile `{}`", assurance_profile.as_name());
    let standard_family = str_field(profile_entry, "standard_family", &profile_ctx)?;

    let mut artifact_hashes = BTreeSet::new();
    artifact_hashes.insert(trace_artifact.to_string());
    artifact_hashes.insert(qualification_artifact.to_string());
    artifact_hashes.extend(coverage_artifacts.iter().cloned());
    artifact_hashes.extend(independent_verifier_run_artifacts.iter().cloned());
    if let Some(object_eq) = object_equivalence_artifact {
        artifact_hashes.insert(object_eq.to_string());
    }
    let artifact_terms: Vec<Term> = artifact_hashes.into_iter().map(Term::Str).collect();

    let objectives = profile_entry
        .get("objectives")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{profile_ctx}: missing `objectives` array"))?;
    let objective_terms: Vec<Term> = objectives
        .iter()
        .map(|objective| {
            let objective_id = str_field(objective, "objective_id", &profile_ctx)?;
            let status = str_field(objective, "status", &profile_ctx)?;
            let summary = str_field(objective, "summary", &profile_ctx)?;
            let evidence_refs = str_vec_field(objective, "evidence_refs", &profile_ctx)?;
            Ok::<Term, String>(map_term([
                (
                    TermOrdKey(Term::symbol(":objective-id")),
                    Term::Str(objective_id.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":status")),
                    Term::Str(status.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":summary")),
                    Term::Str(summary.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":evidence-refs")),
                    Term::Vector(evidence_refs.into_iter().map(Term::Str).collect()),
                ),
                (
                    TermOrdKey(Term::symbol(":evidence-artifacts")),
                    Term::Vector(artifact_terms.clone()),
                ),
            ]))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let unresolved_controls = profile_entry
        .get("unresolved_controls")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{profile_ctx}: missing `unresolved_controls` array"))?;
    let mut unresolved_open_count = 0u64;
    let control_terms: Vec<Term> = unresolved_controls
        .iter()
        .map(|control| {
            let control_id = str_field(control, "control_id", &profile_ctx)?;
            let status = str_field(control, "status", &profile_ctx)?;
            let summary = str_field(control, "summary", &profile_ctx)?;
            let owner = str_field(control, "owner", &profile_ctx)?;
            let tracked_in = str_field(control, "tracked_in", &profile_ctx)?;
            if status != "closed" {
                unresolved_open_count = unresolved_open_count.saturating_add(1);
            }
            let immutable_refs = control
                .get("immutable_refs")
                .and_then(JsonValue::as_array)
                .map(|refs| {
                    refs.iter()
                        .filter_map(JsonValue::as_str)
                        .map(|s| Term::Str(s.to_string()))
                        .collect()
                })
                .unwrap_or_else(Vec::new);
            Ok::<Term, String>(map_term([
                (
                    TermOrdKey(Term::symbol(":control-id")),
                    Term::Str(control_id.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":status")),
                    Term::Str(status.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":summary")),
                    Term::Str(summary.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":owner")),
                    Term::Str(owner.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":tracked-in")),
                    Term::Str(tracked_in.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":closure-bundle")),
                    control
                        .get("closure_bundle")
                        .and_then(JsonValue::as_str)
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":immutable-refs")),
                    Term::Vector(immutable_refs),
                ),
            ]))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(map_term([
        (
            TermOrdKey(Term::symbol(":contract")),
            Term::Str(INTEGRATION_CONTRACT.to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":assurance-profile")),
            Term::symbol(assurance_profile.as_symbol()),
        ),
        (
            TermOrdKey(Term::symbol(":standard-family")),
            Term::Str(standard_family.to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":workflow-target")),
            Term::Str(workflow_target_for_standard_family(standard_family).to_string()),
        ),
        (
            TermOrdKey(Term::symbol(":crosswalk")),
            map_term([
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str(kind.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":version")),
                    Term::Str(version.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":source")),
                    Term::Str("docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json".to_string()),
                ),
            ]),
        ),
        (
            TermOrdKey(Term::symbol(":release-artifacts")),
            map_term([
                (
                    TermOrdKey(Term::symbol(":trace-artifact")),
                    Term::Str(trace_artifact.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":qualification-artifact")),
                    Term::Str(qualification_artifact.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":coverage-artifacts")),
                    Term::Vector(coverage_artifacts.iter().cloned().map(Term::Str).collect()),
                ),
                (
                    TermOrdKey(Term::symbol(":object-equivalence-artifact")),
                    object_equivalence_artifact
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":independent-verifier-run-artifacts")),
                    Term::Vector(
                        independent_verifier_run_artifacts
                            .iter()
                            .cloned()
                            .map(Term::Str)
                            .collect(),
                    ),
                ),
            ]),
        ),
        (
            TermOrdKey(Term::symbol(":objective-bindings")),
            Term::Vector(objective_terms),
        ),
        (
            TermOrdKey(Term::symbol(":external-controls")),
            Term::Vector(control_terms.clone()),
        ),
        (
            TermOrdKey(Term::symbol(":unresolved-control-count")),
            Term::Int(i64::try_from(control_terms.len()).unwrap_or(0).into()),
        ),
        (
            TermOrdKey(Term::symbol(":unresolved-open-count")),
            Term::Int(i64::try_from(unresolved_open_count).unwrap_or(0).into()),
        ),
    ]))
}
