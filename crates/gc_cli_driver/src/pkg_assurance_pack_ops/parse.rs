use gc_coreform::{Term, TermOrdKey};

use super::term_helpers::{
    as_map, normalize_symbol_like, required_bool, required_hex64, required_string,
    required_symbol_or_string, required_symbol_vector,
};
use super::types::{CoverageExport, IndependentVerifierRun, LoadedTerm, ObjectEquivalenceEvidence};

pub(super) fn parse_coverage_export(loaded: LoadedTerm) -> Result<CoverageExport, String> {
    let map = as_map(&loaded.term, "coverage export")?;
    let kind = required_symbol_or_string(map, ":kind", "coverage export")?;
    if kind != "genesis/coverage-v0.2" {
        return Err(format!(
            "coverage export kind must be genesis/coverage-v0.2, got {kind}"
        ));
    }
    let profile = normalize_symbol_like(&required_symbol_or_string(
        map,
        ":profile",
        "coverage export",
    )?);
    let ok = required_bool(map, ":ok", "coverage export")?;
    Ok(CoverageExport {
        loaded,
        profile,
        ok,
    })
}

pub(super) fn parse_object_equivalence_evidence(
    loaded: LoadedTerm,
    expected_trace_hash: &str,
    expected_qualification_hash: &str,
) -> Result<ObjectEquivalenceEvidence, String> {
    let map = as_map(&loaded.term, "object equivalence artifact")?;
    let kind = required_symbol_or_string(map, ":kind", "object equivalence artifact")?;
    if kind != "genesis/object-equivalence-v0.1" {
        return Err(format!(
            "object equivalence kind must be genesis/object-equivalence-v0.1, got {kind}"
        ));
    }
    let ok = required_bool(map, ":ok", "object equivalence artifact")?;
    if !ok {
        return Err("object equivalence artifact must declare :ok true".to_string());
    }
    let trace_artifact = required_hex64(map, ":trace-artifact", "object equivalence artifact")?;
    if trace_artifact != expected_trace_hash {
        return Err(format!(
            "object equivalence :trace-artifact {} does not match assurance trace artifact {}",
            trace_artifact, expected_trace_hash
        ));
    }
    let qualification_artifact = required_hex64(
        map,
        ":qualification-artifact",
        "object equivalence artifact",
    )?;
    if qualification_artifact != expected_qualification_hash {
        return Err(format!(
            "object equivalence :qualification-artifact {} does not match assurance qualification artifact {}",
            qualification_artifact, expected_qualification_hash
        ));
    }
    let source_artifact = required_hex64(map, ":source-artifact", "object equivalence artifact")?;
    let object_artifact = required_hex64(map, ":object-artifact", "object equivalence artifact")?;
    let method = normalize_symbol_like(&required_symbol_or_string(
        map,
        ":method",
        "object equivalence artifact",
    )?);
    Ok(ObjectEquivalenceEvidence {
        loaded,
        source_artifact,
        object_artifact,
        method,
    })
}

pub(super) fn parse_independent_verifier_run(
    loaded: LoadedTerm,
    expected_profile: &str,
    expected_trace_hash: &str,
    expected_qualification_hash: &str,
    expected_object_equivalence_hash: Option<&str>,
) -> Result<IndependentVerifierRun, String> {
    let map = as_map(&loaded.term, "independent verifier run artifact")?;
    let kind = required_symbol_or_string(map, ":kind", "independent verifier run artifact")?;
    if kind != "genesis/independent-verifier-run-v0.1" {
        return Err(format!(
            "independent verifier run kind must be genesis/independent-verifier-run-v0.1, got {kind}"
        ));
    }
    let ok = required_bool(map, ":ok", "independent verifier run artifact")?;
    if !ok {
        return Err("independent verifier run artifact must declare :ok true".to_string());
    }
    let profile = normalize_symbol_like(&required_symbol_or_string(
        map,
        ":assurance-profile",
        "independent verifier run artifact",
    )?);
    if profile != expected_profile {
        return Err(format!(
            "independent verifier run :assurance-profile {} does not match target profile {}",
            profile, expected_profile
        ));
    }
    let result = normalize_symbol_like(&required_symbol_or_string(
        map,
        ":result",
        "independent verifier run artifact",
    )?);
    if result != ":pass" {
        return Err(format!(
            "independent verifier run :result must be :pass, got {result}"
        ));
    }
    let trace_artifact =
        required_hex64(map, ":trace-artifact", "independent verifier run artifact")?;
    if trace_artifact != expected_trace_hash {
        return Err(format!(
            "independent verifier run :trace-artifact {} does not match assurance trace artifact {}",
            trace_artifact, expected_trace_hash
        ));
    }
    let qualification_artifact = required_hex64(
        map,
        ":qualification-artifact",
        "independent verifier run artifact",
    )?;
    if qualification_artifact != expected_qualification_hash {
        return Err(format!(
            "independent verifier run :qualification-artifact {} does not match assurance qualification artifact {}",
            qualification_artifact, expected_qualification_hash
        ));
    }
    let object_equivalence_artifact = required_hex64(
        map,
        ":object-equivalence-artifact",
        "independent verifier run artifact",
    )?;
    if let Some(expected) = expected_object_equivalence_hash {
        if object_equivalence_artifact != expected {
            return Err(format!(
                "independent verifier run :object-equivalence-artifact {} does not match assurance object equivalence artifact {}",
                object_equivalence_artifact, expected
            ));
        }
    } else {
        return Err(
            "independent verifier run was provided but no --object-equivalence artifact is loaded"
                .to_string(),
        );
    }
    let run_id = required_string(map, ":run-id", "independent verifier run artifact")?;
    let runner = required_string(map, ":runner", "independent verifier run artifact")?;
    let roles = required_symbol_vector(map, ":roles", "independent verifier run artifact")?;
    if roles.len() < 2 {
        return Err(
            "independent verifier run artifact :roles must include at least two role symbols"
                .to_string(),
        );
    }
    Ok(IndependentVerifierRun {
        loaded,
        run_id,
        runner,
        roles,
    })
}

pub(super) fn parse_independence_attestation(raw: &str) -> Result<Term, String> {
    let trimmed = raw.trim();
    let (pair, attestor) = trimmed.split_once('@').ok_or_else(|| {
        format!(
            "invalid --independence-attestation `{trimmed}`; expected <left-role>:<right-role>@<attestor>"
        )
    })?;
    let (left, right) = pair.split_once(':').ok_or_else(|| {
        format!(
            "invalid --independence-attestation `{trimmed}`; expected <left-role>:<right-role>@<attestor>"
        )
    })?;
    let left = normalize_symbol_like(left);
    let right = normalize_symbol_like(right);
    let attestor = attestor.trim();
    if left == right {
        return Err(format!(
            "invalid --independence-attestation `{trimmed}`: role pair must use distinct roles"
        ));
    }
    if attestor.is_empty() {
        return Err(format!(
            "invalid --independence-attestation `{trimmed}`: attestor cannot be empty"
        ));
    }
    Ok(Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":independence-attestation"),
            ),
            (
                TermOrdKey(Term::symbol(":roles")),
                Term::Vector(vec![Term::symbol(&left), Term::symbol(&right)]),
            ),
            (
                TermOrdKey(Term::symbol(":attestor")),
                Term::Str(attestor.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    ))
}
