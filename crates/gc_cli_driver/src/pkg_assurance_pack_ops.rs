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

pub(crate) struct AssurancePackArgs<'a> {
    pub pkg: &'a Path,
    pub assurance_profile: &'a str,
    pub commit: Option<&'a str>,
    pub snapshot: &'a str,
    pub policy: Option<&'a str>,
    pub trace_spec: &'a str,
    pub qualification_spec: &'a str,
    pub coverage_specs: &'a [String],
    pub independence_attestations: &'a [String],
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
            },
            Self::Do178cDalA => AssuranceRequirements {
                minimum_coverage_rank: 3,
                require_independence_attestations: true,
            },
            Self::Do178cDalB => AssuranceRequirements {
                minimum_coverage_rank: 2,
                require_independence_attestations: true,
            },
            Self::NasaClassA => AssuranceRequirements {
                minimum_coverage_rank: 3,
                require_independence_attestations: true,
            },
            Self::NasaClassB => AssuranceRequirements {
                minimum_coverage_rank: 2,
                require_independence_attestations: true,
            },
            Self::Iec62304ClassC => AssuranceRequirements {
                minimum_coverage_rank: 1,
                require_independence_attestations: false,
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
                TermOrdKey(Term::symbol(":independence-attestations")),
                Term::Vector(independence_terms.clone()),
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
            &independence_terms,
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

fn write_bundle_dir(
    bundle_dir: &Path,
    pack_src: &str,
    trace: &LoadedTerm,
    qualification: &LoadedTerm,
    coverage_exports: &[CoverageExport],
    independence_attestations: &[Term],
) -> Result<(), String> {
    std::fs::create_dir_all(bundle_dir)
        .map_err(|e| format!("mkdir {}: {e}", bundle_dir.display()))?;
    let coverage_dir = bundle_dir.join("coverage");
    std::fs::create_dir_all(&coverage_dir)
        .map_err(|e| format!("mkdir {}: {e}", coverage_dir.display()))?;

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
