use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term, parse_term, print_term};
use gc_effects::ArtifactStore;
use gc_pkg::PackageManifest;
use gc_vcs::{
    RequirementsTraceGateContext, ToolQualificationGateContext, validate_hex_hash,
    validate_requirements_trace_evidence, validate_tool_qualification_evidence,
};

use crate::pkg_workspace_ops::LocalPkgResult;
#[path = "pkg_assurance_pack_bundle.rs"]
mod pkg_assurance_pack_bundle;
use pkg_assurance_pack_bundle::write_bundle_dir;

pub(crate) struct AssurancePackArgs<'a> {
    pub pkg: &'a Path,
    pub assurance_profile: &'a str,
    pub commit: Option<&'a str>,
    pub snapshot: &'a str,
    pub policy: Option<&'a str>,
    pub trace_spec: &'a str,
    pub qualification_spec: &'a str,
    pub coverage_specs: &'a [String],
    pub object_equivalence_spec: Option<&'a str>,
    pub independence_attestations: &'a [String],
    pub independent_verifier_run_specs: &'a [String],
    pub out: &'a Path,
    pub bundle_dir: Option<&'a Path>,
    pub no_store: bool,
}

#[derive(Debug, Clone)]
struct LoadedTerm {
    term: Term,
    hash: String,
    canonical_src: String,
    source: String,
}

#[derive(Debug, Clone)]
struct CoverageExport {
    loaded: LoadedTerm,
    profile: String,
    ok: bool,
}

#[derive(Debug, Clone)]
struct ObjectEquivalenceEvidence {
    loaded: LoadedTerm,
    source_artifact: String,
    object_artifact: String,
    method: String,
}

#[derive(Debug, Clone)]
struct IndependentVerifierRun {
    loaded: LoadedTerm,
    run_id: String,
    runner: String,
    roles: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssuranceProfile {
    Custom,
    Do178cDalA,
    Do178cDalB,
    NasaClassA,
    NasaClassB,
    Iec62304ClassC,
}

#[derive(Debug, Clone, Copy)]
struct AssuranceRequirements {
    minimum_coverage_rank: u8,
    require_independence_attestations: bool,
    require_object_equivalence: bool,
    require_independent_verifier_runs: bool,
}

impl AssuranceProfile {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "custom" => Ok(Self::Custom),
            "do178c-dal-a" => Ok(Self::Do178cDalA),
            "do178c-dal-b" => Ok(Self::Do178cDalB),
            "nasa-class-a" => Ok(Self::NasaClassA),
            "nasa-class-b" => Ok(Self::NasaClassB),
            "iec62304-class-c" => Ok(Self::Iec62304ClassC),
            other => Err(format!(
                "unsupported --assurance-profile `{other}`; expected one of custom|do178c-dal-a|do178c-dal-b|nasa-class-a|nasa-class-b|iec62304-class-c"
            )),
        }
    }

    fn as_symbol(self) -> &'static str {
        match self {
            Self::Custom => ":custom",
            Self::Do178cDalA => ":do178c-dal-a",
            Self::Do178cDalB => ":do178c-dal-b",
            Self::NasaClassA => ":nasa-class-a",
            Self::NasaClassB => ":nasa-class-b",
            Self::Iec62304ClassC => ":iec62304-class-c",
        }
    }

    fn requirements(self) -> AssuranceRequirements {
        match self {
            Self::Custom => AssuranceRequirements {
                minimum_coverage_rank: 0,
                require_independence_attestations: false,
                require_object_equivalence: false,
                require_independent_verifier_runs: false,
            },
            Self::Do178cDalA => AssuranceRequirements {
                minimum_coverage_rank: 3,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::Do178cDalB => AssuranceRequirements {
                minimum_coverage_rank: 2,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::NasaClassA => AssuranceRequirements {
                minimum_coverage_rank: 3,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::NasaClassB => AssuranceRequirements {
                minimum_coverage_rank: 2,
                require_independence_attestations: true,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
            Self::Iec62304ClassC => AssuranceRequirements {
                minimum_coverage_rank: 1,
                require_independence_attestations: false,
                require_object_equivalence: true,
                require_independent_verifier_runs: true,
            },
        }
    }
}

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

fn load_term_from_spec(
    spec: &str,
    base_dir: &Path,
    store_dir: &Path,
    label: &str,
) -> Result<LoadedTerm, String> {
    let candidate = PathBuf::from(spec);
    let path = if candidate.is_file() || candidate.is_absolute() {
        candidate
    } else {
        let from_base = base_dir.join(spec);
        if from_base.is_file() {
            from_base
        } else if is_hex64(spec) {
            store_dir.join(spec)
        } else {
            from_base
        }
    };
    if !path.is_file() {
        return Err(format!(
            "{label} artifact spec `{spec}` did not resolve to a readable file (tried {})",
            path.display()
        ));
    }
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let term = parse_term(&src).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let canonical_src = print_term(&term) + "\n";
    let hash = blake3::hash(canonical_src.as_bytes()).to_hex().to_string();
    if is_hex64(spec) && spec != hash {
        return Err(format!(
            "{label} artifact hash mismatch for `{spec}`: canonical hash is {hash}"
        ));
    }
    Ok(LoadedTerm {
        term,
        hash,
        canonical_src,
        source: path.display().to_string(),
    })
}

fn parse_coverage_export(loaded: LoadedTerm) -> Result<CoverageExport, String> {
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

fn parse_object_equivalence_evidence(
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

fn parse_independent_verifier_run(
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

fn parse_independence_attestation(raw: &str) -> Result<Term, String> {
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

fn extract_release_binding(term: &Term, key: &str) -> Result<Option<String>, String> {
    let root = as_map(term, "artifact")?;
    let release_term = root
        .get(&TermOrdKey(Term::symbol(":release")))
        .ok_or_else(|| "artifact missing :release".to_string())?;
    let release = as_map(release_term, "artifact/:release")?;
    match release.get(&TermOrdKey(Term::symbol(key))) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(s.clone()))
            }
        }
        Some(other) => Err(format!(
            "artifact/:release {key} must be string|nil, got {}",
            print_term(other)
        )),
    }
}

fn extract_vector_field<'a>(term: &'a Term, key: &str, what: &str) -> Result<&'a [Term], String> {
    let root = as_map(term, what)?;
    match root.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Vector(v)) => Ok(v.as_slice()),
        Some(other) => Err(format!(
            "{what} {key} must be vector, got {}",
            print_term(other)
        )),
        None => Err(format!("{what} missing {key}")),
    }
}

fn as_map<'a>(term: &'a Term, what: &str) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    match term {
        Term::Map(m) => Ok(m),
        _ => Err(format!("{what} must be map, got {}", print_term(term))),
    }
}

fn required_symbol_or_string(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Symbol(s) | Term::Str(s) => Ok(s.clone()),
        _ => Err(format!("{what} {key} must be symbol|string")),
    }
}

fn required_string(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Str(s) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(s.clone())
            }
        }
        _ => Err(format!("{what} {key} must be string")),
    }
}

fn required_hex64(m: &BTreeMap<TermOrdKey, Term>, key: &str, what: &str) -> Result<String, String> {
    let value = required_string(m, key, what)?;
    validate_hex_hash(&value).map_err(|e| format!("{what} {key} must be hex64: {e}"))?;
    Ok(value)
}

fn required_symbol_vector(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<Vec<String>, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    let Term::Vector(values) = t else {
        return Err(format!("{what} {key} must be vector"));
    };
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        match value {
            Term::Symbol(s) | Term::Str(s) => out.push(normalize_symbol_like(s)),
            _ => {
                return Err(format!(
                    "{what} {key} entries must be symbols|strings, got {}",
                    print_term(value)
                ));
            }
        }
    }
    Ok(out)
}

fn required_bool(m: &BTreeMap<TermOrdKey, Term>, key: &str, what: &str) -> Result<bool, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Bool(v) => Ok(*v),
        _ => Err(format!("{what} {key} must be bool")),
    }
}

fn normalize_symbol_like(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with(':') {
        trimmed.to_string()
    } else {
        format!(":{trimmed}")
    }
}

fn is_hex64(s: &str) -> bool {
    if s.len() != 64 {
        return false;
    }
    validate_hex_hash(s).is_ok()
}

fn coverage_rank(profile: &str) -> u8 {
    match normalize_symbol_like(profile).as_str() {
        ":symbol" => 1,
        ":decision" => 2,
        ":mcdc" => 3,
        _ => 0,
    }
}
