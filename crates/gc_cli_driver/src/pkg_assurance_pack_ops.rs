use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_effects::ArtifactStore;
use gc_pkg::PackageManifest;
use gc_vcs::{
    RequirementsTraceGateContext, ToolQualificationGateContext, validate_hex_hash,
    validate_requirements_trace_evidence, validate_tool_qualification_evidence,
};
use std::collections::BTreeSet;

use crate::pkg_workspace_ops::LocalPkgResult;

mod bundle;
mod integration;
mod parse;
mod profile;
mod resolve;
mod term_helpers;
mod types;

use bundle::write_bundle_dir;
use integration::build_external_control_bindings;
use parse::{
    parse_coverage_export, parse_independence_attestation, parse_independent_verifier_run,
    parse_object_equivalence_evidence,
};
use profile::AssuranceProfile;
use resolve::load_term_from_spec;
use term_helpers::{coverage_rank, extract_release_binding, extract_vector_field};
pub(crate) use types::AssurancePackArgs;

pub(crate) fn handle_assurance_pack(args: AssurancePackArgs<'_>) -> Result<LocalPkgResult, String> {
    let assurance_profile = AssuranceProfile::parse(args.assurance_profile)?;
    let requirements = assurance_profile.requirements();

    validate_hex_hash(args.snapshot).map_err(|e| format!("invalid --snapshot hash: {e}"))?;
    if let Some(commit_h) = args.commit {
        validate_hex_hash(commit_h).map_err(|e| format!("invalid --commit hash: {e}"))?;
    }
    if let Some(policy_h) = args.policy {
        validate_hex_hash(policy_h).map_err(|e| format!("invalid --policy hash: {e}"))?;
    }

    let (manifest, pkg_dir) = PackageManifest::load(args.pkg).map_err(|e| e.to_string())?;
    let store_dir = pkg_dir.join(".genesis").join("store");

    let trace = load_term_from_spec(args.trace_spec, &pkg_dir, &store_dir, "trace")?;
    let qualification = load_term_from_spec(
        args.qualification_spec,
        &pkg_dir,
        &store_dir,
        "qualification",
    )?;

    if args.commit.is_none() && extract_release_binding(&trace.term, ":commit")?.is_some() {
        return Err(
            "trace artifact binds :release/:commit but --commit was not provided".to_string(),
        );
    }
    if args.commit.is_none() && extract_release_binding(&qualification.term, ":commit")?.is_some() {
        return Err(
            "qualification artifact binds :release/:commit but --commit was not provided"
                .to_string(),
        );
    }

    let mut observed_kinds = BTreeSet::new();
    observed_kinds.insert(":requirements-trace".to_string());
    observed_kinds.insert(":tool-qualification".to_string());

    let mut coverage_exports = Vec::new();
    for spec in args.coverage_specs {
        let loaded = load_term_from_spec(spec, &pkg_dir, &store_dir, "coverage export")?;
        let export = parse_coverage_export(loaded)?;
        observed_kinds.insert(":coverage".to_string());
        coverage_exports.push(export);
    }
    coverage_exports.sort_by_key(|c| c.loaded.hash.clone());

    let object_equivalence = match args.object_equivalence_spec {
        Some(spec) => {
            let loaded = load_term_from_spec(spec, &pkg_dir, &store_dir, "object equivalence")?;
            let parsed =
                parse_object_equivalence_evidence(loaded, &trace.hash, &qualification.hash)?;
            observed_kinds.insert(":object-equivalence".to_string());
            Some(parsed)
        }
        None => None,
    };

    let mut independence_terms = Vec::new();
    for raw in args.independence_attestations {
        independence_terms.push(parse_independence_attestation(raw)?);
    }
    independence_terms.sort_by_key(print_term);
    independence_terms.dedup_by(|a, b| print_term(a) == print_term(b));

    if requirements.require_independence_attestations && independence_terms.is_empty() {
        return Err(format!(
            "assurance profile {} requires at least one --independence-attestation",
            assurance_profile.as_symbol()
        ));
    }

    if requirements.require_object_equivalence && object_equivalence.is_none() {
        return Err(format!(
            "assurance profile {} requires --object-equivalence evidence",
            assurance_profile.as_symbol()
        ));
    }

    let mut independent_verifier_runs = Vec::new();
    for spec in args.independent_verifier_run_specs {
        let loaded = load_term_from_spec(spec, &pkg_dir, &store_dir, "independent verifier run")?;
        let run = parse_independent_verifier_run(
            loaded,
            assurance_profile.as_symbol(),
            &trace.hash,
            &qualification.hash,
            object_equivalence.as_ref().map(|e| e.loaded.hash.as_str()),
        )?;
        observed_kinds.insert(":independent-verifier-run".to_string());
        independent_verifier_runs.push(run);
    }
    independent_verifier_runs.sort_by_key(|run| run.loaded.hash.clone());
    independent_verifier_runs.dedup_by(|a, b| a.loaded.hash == b.loaded.hash);

    if requirements.require_independent_verifier_runs && independent_verifier_runs.is_empty() {
        return Err(format!(
            "assurance profile {} requires at least one --independent-verifier-run artifact",
            assurance_profile.as_symbol()
        ));
    }

    if requirements.minimum_coverage_rank > 0 {
        if coverage_exports.is_empty() {
            return Err(format!(
                "assurance profile {} requires at least one --coverage artifact",
                assurance_profile.as_symbol()
            ));
        }
        let best_rank = coverage_exports
            .iter()
            .filter(|c| c.ok)
            .map(|c| coverage_rank(&c.profile))
            .max()
            .unwrap_or(0);
        if best_rank < requirements.minimum_coverage_rank {
            return Err(format!(
                "assurance profile {} requires minimum coverage rank {} but observed best rank {} (profiles: {})",
                assurance_profile.as_symbol(),
                requirements.minimum_coverage_rank,
                best_rank,
                coverage_exports
                    .iter()
                    .map(|c| c.profile.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    let commit_ctx = args
        .commit
        .unwrap_or("0000000000000000000000000000000000000000000000000000000000000000");

    let trace_ctx = RequirementsTraceGateContext {
        commit_hash: commit_ctx,
        snapshot_hash: args.snapshot,
        policy_hash: args.policy,
        commit_obligations: &manifest.obligations,
        observed_evidence_kinds: &observed_kinds,
    };
    validate_requirements_trace_evidence(&trace.term, &trace_ctx)
        .map_err(|e| format!("trace validation failed: {e}"))?;

    let qualification_ctx = ToolQualificationGateContext {
        commit_hash: commit_ctx,
        snapshot_hash: args.snapshot,
        policy_hash: args.policy,
    };
    validate_tool_qualification_evidence(&qualification.term, &qualification_ctx)
        .map_err(|e| format!("qualification validation failed: {e}"))?;

    let trace_requirements =
        extract_vector_field(&trace.term, ":requirements", "trace artifact")?.to_vec();
    let qualification_tools =
        extract_vector_field(&qualification.term, ":tools", "qualification artifact")?.to_vec();
    let qualification_tests = extract_vector_field(
        &qualification.term,
        ":qualification-tests",
        "qualification artifact",
    )?
    .to_vec();

    let coverage_terms: Vec<Term> = coverage_exports
        .iter()
        .map(|c| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":artifact")),
                        Term::Str(c.loaded.hash.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":profile")),
                        Term::symbol(&c.profile),
                    ),
                    (TermOrdKey(Term::symbol(":ok")), Term::Bool(c.ok)),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str(c.loaded.source.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();

    let independent_verifier_run_terms: Vec<Term> = independent_verifier_runs
        .iter()
        .map(|run| {
            Term::Map(
                [
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
                    (
                        TermOrdKey(Term::symbol(":roles")),
                        Term::Vector(run.roles.iter().map(Term::symbol).collect()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str(run.loaded.source.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();

    let object_equivalence_term = object_equivalence
        .as_ref()
        .map(|e| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":artifact")),
                        Term::Str(e.loaded.hash.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str(e.loaded.source.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source-artifact")),
                        Term::Str(e.source_artifact.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":object-artifact")),
                        Term::Str(e.object_artifact.clone()),
                    ),
                    (TermOrdKey(Term::symbol(":method")), Term::symbol(&e.method)),
                ]
                .into_iter()
                .collect(),
            )
        })
        .unwrap_or(Term::Nil);
    let coverage_artifact_hashes: Vec<String> = coverage_exports
        .iter()
        .map(|c| c.loaded.hash.clone())
        .collect();
    let independent_run_artifact_hashes: Vec<String> = independent_verifier_runs
        .iter()
        .map(|run| run.loaded.hash.clone())
        .collect();
    let external_control_bindings = build_external_control_bindings(
        assurance_profile,
        &trace.hash,
        &qualification.hash,
        &coverage_artifact_hashes,
        object_equivalence.as_ref().map(|e| e.loaded.hash.as_str()),
        &independent_run_artifact_hashes,
    )?;

    let pack = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/evidence"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":assurance-pack"),
            ),
            (TermOrdKey(Term::symbol(":status")), Term::symbol(":ready")),
            (
                TermOrdKey(Term::symbol(":target-profile")),
                Term::symbol(assurance_profile.as_symbol()),
            ),
            (
                TermOrdKey(Term::symbol(":release")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":commit")),
                            args.commit
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":snapshot")),
                            Term::Str(args.snapshot.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            args.policy
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":trace-matrix")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":artifact")),
                            Term::Str(trace.hash.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":source")),
                            Term::Str(trace.source.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":requirements")),
                            Term::Vector(trace_requirements),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":qualified-tool-manifest")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":artifact")),
                            Term::Str(qualification.hash.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":source")),
                            Term::Str(qualification.source.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":tools")),
                            Term::Vector(qualification_tools),
                        ),
                        (
                            TermOrdKey(Term::symbol(":qualification-tests")),
                            Term::Vector(qualification_tests),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":coverage-exports")),
                Term::Vector(coverage_terms),
            ),
            (
                TermOrdKey(Term::symbol(":object-equivalence")),
                object_equivalence_term,
            ),
            (
                TermOrdKey(Term::symbol(":independence-attestations")),
                Term::Vector(independence_terms.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":independent-verifier-runs")),
                Term::Vector(independent_verifier_run_terms),
            ),
            (
                TermOrdKey(Term::symbol(":external-control-bindings")),
                external_control_bindings.clone(),
            ),
            (
                TermOrdKey(Term::symbol(":summary")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":coverage-count")),
                            Term::Int(i64::try_from(coverage_exports.len()).unwrap_or(0).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":independence-attestation-count")),
                            Term::Int(i64::try_from(independence_terms.len()).unwrap_or(0).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":independent-verifier-run-count")),
                            Term::Int(
                                i64::try_from(independent_verifier_runs.len())
                                    .unwrap_or(0)
                                    .into(),
                            ),
                        ),
                        (
                            TermOrdKey(Term::symbol(":object-equivalence-present")),
                            Term::Bool(object_equivalence.is_some()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    );

    let pack_src = print_term(&pack) + "\n";
    if let Some(parent) = args.out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(args.out, pack_src.as_bytes())
        .map_err(|e| format!("write {}: {e}", args.out.display()))?;

    let mut pack_h = blake3::hash(pack_src.as_bytes()).to_hex().to_string();
    if !args.no_store {
        let store = ArtifactStore::open(&store_dir)
            .map_err(|e| format!("open {}: {e}", store_dir.display()))?;
        let stored = store
            .put_bytes(pack_src.as_bytes())
            .map_err(|e| format!("store assurance pack: {e}"))?;
        pack_h = stored;
    }

    if let Some(bundle_dir) = args.bundle_dir {
        write_bundle_dir(
            bundle_dir,
            &pack_src,
            &trace,
            &qualification,
            &coverage_exports,
            object_equivalence.as_ref(),
            &independence_terms,
            &independent_verifier_runs,
        )?;
    }

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":assurance-pack"),
            ),
            (TermOrdKey(Term::symbol(":artifact")), Term::Str(pack_h)),
            (
                TermOrdKey(Term::symbol(":target-profile")),
                Term::symbol(assurance_profile.as_symbol()),
            ),
            (
                TermOrdKey(Term::symbol(":out")),
                Term::Str(args.out.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":trace-artifact")),
                Term::Str(trace.hash),
            ),
            (
                TermOrdKey(Term::symbol(":qualification-artifact")),
                Term::Str(qualification.hash),
            ),
            (
                TermOrdKey(Term::symbol(":coverage-count")),
                Term::Int(i64::try_from(coverage_exports.len()).unwrap_or(0).into()),
            ),
            (
                TermOrdKey(Term::symbol(":independence-attestation-count")),
                Term::Int(i64::try_from(independence_terms.len()).unwrap_or(0).into()),
            ),
            (
                TermOrdKey(Term::symbol(":independent-verifier-run-count")),
                Term::Int(
                    i64::try_from(independent_verifier_runs.len())
                        .unwrap_or(0)
                        .into(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":object-equivalence-artifact")),
                object_equivalence
                    .as_ref()
                    .map(|e| Term::Str(e.loaded.hash.clone()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":bundle-dir")),
                args.bundle_dir
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":external-control-bindings")),
                external_control_bindings,
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-assurance-pack-v0.1",
        log_op: "pkg-assurance-pack",
        program_hash: hash_term(&value),
        value,
    })
}
