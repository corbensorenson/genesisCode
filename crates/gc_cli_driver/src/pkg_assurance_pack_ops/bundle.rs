use std::path::Path;

use gc_coreform::{Term, TermOrdKey, print_term};

use super::types::{CoverageExport, IndependentVerifierRun, LoadedTerm, ObjectEquivalenceEvidence};

#[expect(
    clippy::too_many_arguments,
    reason = "bundle export writes deterministic artifacts from explicit provenance inputs"
)]
pub(super) fn write_bundle_dir(
    bundle_dir: &Path,
    pack_src: &str,
    trace: &LoadedTerm,
    qualification: &LoadedTerm,
    coverage_exports: &[CoverageExport],
    object_equivalence: Option<&ObjectEquivalenceEvidence>,
    independence_attestations: &[Term],
    independent_verifier_runs: &[IndependentVerifierRun],
) -> Result<(), String> {
    std::fs::create_dir_all(bundle_dir)
        .map_err(|e| format!("mkdir {}: {e}", bundle_dir.display()))?;
    let coverage_dir = bundle_dir.join("coverage");
    std::fs::create_dir_all(&coverage_dir)
        .map_err(|e| format!("mkdir {}: {e}", coverage_dir.display()))?;
    let verifier_dir = bundle_dir.join("independent_verifier");
    std::fs::create_dir_all(&verifier_dir)
        .map_err(|e| format!("mkdir {}: {e}", verifier_dir.display()))?;

    std::fs::write(bundle_dir.join("assurance_pack.gc"), pack_src.as_bytes())
        .map_err(|e| format!("write assurance_pack.gc: {e}"))?;
    std::fs::write(
        bundle_dir.join("requirements_trace.gc"),
        trace.canonical_src.as_bytes(),
    )
    .map_err(|e| format!("write requirements_trace.gc: {e}"))?;
    std::fs::write(
        bundle_dir.join("tool_qualification.gc"),
        qualification.canonical_src.as_bytes(),
    )
    .map_err(|e| format!("write tool_qualification.gc: {e}"))?;
    if let Some(object_eq) = object_equivalence {
        std::fs::write(
            bundle_dir.join("object_equivalence.gc"),
            object_eq.loaded.canonical_src.as_bytes(),
        )
        .map_err(|e| format!("write object_equivalence.gc: {e}"))?;
    }

    let mut coverage_entries = Vec::new();
    for (idx, coverage) in coverage_exports.iter().enumerate() {
        let name = format!("coverage_{:02}_{}.gc", idx + 1, &coverage.loaded.hash[..12]);
        std::fs::write(
            coverage_dir.join(&name),
            coverage.loaded.canonical_src.as_bytes(),
        )
        .map_err(|e| format!("write {}: {e}", name))?;
        coverage_entries.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":file")), Term::Str(name)),
                (
                    TermOrdKey(Term::symbol(":artifact")),
                    Term::Str(coverage.loaded.hash.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":profile")),
                    Term::symbol(&coverage.profile),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }
    let mut verifier_entries = Vec::new();
    for (idx, run) in independent_verifier_runs.iter().enumerate() {
        let name = format!("run_{:02}_{}.gc", idx + 1, &run.loaded.hash[..12]);
        std::fs::write(
            verifier_dir.join(&name),
            run.loaded.canonical_src.as_bytes(),
        )
        .map_err(|e| format!("write {}: {e}", name))?;
        verifier_entries.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":file")), Term::Str(name)),
                (
                    TermOrdKey(Term::symbol(":artifact")),
                    Term::Str(run.loaded.hash.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":run-id")),
                    Term::Str(run.run_id.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":runner")),
                    Term::Str(run.runner.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let manifest = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":assurance-bundle-manifest"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":files")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":assurance-pack")),
                            Term::Str("assurance_pack.gc".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":requirements-trace")),
                            Term::Str("requirements_trace.gc".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":tool-qualification")),
                            Term::Str("tool_qualification.gc".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":coverage")),
                            Term::Vector(coverage_entries),
                        ),
                        (
                            TermOrdKey(Term::symbol(":object-equivalence")),
                            object_equivalence
                                .map(|_| Term::Str("object_equivalence.gc".to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":independent-verifier-runs")),
                            Term::Vector(verifier_entries),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":independence-attestations")),
                Term::Vector(independence_attestations.to_vec()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    std::fs::write(
        bundle_dir.join("bundle_manifest.gc"),
        (print_term(&manifest) + "\n").as_bytes(),
    )
    .map_err(|e| format!("write bundle_manifest.gc: {e}"))?;
    Ok(())
}
