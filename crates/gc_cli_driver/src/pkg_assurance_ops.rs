use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term, parse_term, print_term};
use gc_effects::ArtifactStore;
use gc_pkg::PackageManifest;
use gc_vcs::validate_hex_hash;

use crate::pkg_workspace_ops::LocalPkgResult;
#[path = "pkg_assurance_ops_qualification.rs"]
mod pkg_assurance_ops_qualification;
#[path = "pkg_assurance_ops_requirements.rs"]
mod pkg_assurance_ops_requirements;

pub(crate) struct ToolQualificationArgs<'a> {
    pub commit: Option<&'a str>,
    pub snapshot: &'a str,
    pub policy: Option<&'a str>,
    pub profile: &'a str,
    pub requirement_ids: &'a [String],
    pub test_artifacts: &'a [String],
    pub tools: &'a [String],
    pub out: &'a Path,
    pub no_store: bool,
}

pub(crate) fn handle_trace(
    pkg: &Path,
    requirements: &Path,
    commit: Option<&str>,
    snapshot: Option<&str>,
    policy: Option<&str>,
    out: &Path,
    no_store: bool,
) -> Result<LocalPkgResult, String> {
    if let Some(c) = commit {
        validate_hex_hash(c).map_err(|e| format!("invalid --commit hash: {e}"))?;
    }
    let snapshot = snapshot.ok_or_else(|| {
        "missing --snapshot hash (required when --commit is omitted to avoid hash-cycle release bindings)"
            .to_string()
    })?;
    validate_hex_hash(snapshot).map_err(|e| format!("invalid --snapshot hash: {e}"))?;
    if let Some(p) = policy {
        validate_hex_hash(p).map_err(|e| format!("invalid --policy hash: {e}"))?;
    }

    let (manifest, pkg_dir) = PackageManifest::load(pkg).map_err(|e| e.to_string())?;
    let req_path = if requirements.is_absolute() {
        requirements.to_path_buf()
    } else {
        pkg_dir.join(requirements)
    };
    let req_src = std::fs::read_to_string(&req_path)
        .map_err(|e| format!("read {}: {e}", req_path.display()))?;
    let req_term = parse_term(&req_src)
        .map_err(|e| format!("parse requirements graph {}: {e}", req_path.display()))?;
    let requirements_vec =
        pkg_assurance_ops_requirements::parse_requirements_graph(&req_term, &manifest.obligations)?;

    let graph_h = blake3::hash(print_term(&req_term).as_bytes())
        .to_hex()
        .to_string();
    let module_index: Vec<Term> = manifest
        .modules
        .iter()
        .map(|m| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":path")), Term::Str(m.path.clone())),
                    (
                        TermOrdKey(Term::symbol(":hash")),
                        m.hash.clone().map(Term::Str).unwrap_or(Term::Nil),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    let requirements_count = requirements_vec.len();
    let evidence = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/evidence"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":requirements-trace"),
            ),
            (
                TermOrdKey(Term::symbol(":status")),
                Term::symbol(":verified"),
            ),
            (
                TermOrdKey(Term::symbol(":graph-h")),
                Term::Str(graph_h.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(manifest.name.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":version")),
                            Term::Str(manifest.version.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":manifest-path")),
                            Term::Str(pkg.display().to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":release")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":commit")),
                            commit
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":snapshot")),
                            Term::Str(snapshot.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            policy
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":requirements")),
                Term::Vector(requirements_vec),
            ),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    manifest
                        .obligations
                        .iter()
                        .cloned()
                        .map(Term::symbol)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":module-index")),
                Term::Vector(module_index),
            ),
            (
                TermOrdKey(Term::symbol(":summary")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":requirements")),
                            Term::Int(i64::try_from(requirements_count).unwrap_or(0).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":links-verified")),
                            Term::Bool(true),
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
    let evidence_src = print_term(&evidence) + "\n";
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(out, evidence_src.as_bytes())
        .map_err(|e| format!("write {}: {e}", out.display()))?;
    let evidence_h = blake3::hash(evidence_src.as_bytes()).to_hex().to_string();
    let graph_store_h = if no_store {
        None
    } else {
        let store = ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
            .map_err(|e| format!("open store: {e}"))?;
        let gh = store
            .put_bytes((print_term(&req_term) + "\n").as_bytes())
            .map_err(|e| format!("store requirements graph: {e}"))?;
        let _ = store
            .put_bytes(evidence_src.as_bytes())
            .map_err(|e| format!("store requirements trace: {e}"))?;
        Some(gh)
    };

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":requirements-trace"),
            ),
            (TermOrdKey(Term::symbol(":artifact")), Term::Str(evidence_h)),
            (
                TermOrdKey(Term::symbol(":graph-h")),
                Term::Str(graph_store_h.unwrap_or(graph_h)),
            ),
            (
                TermOrdKey(Term::symbol(":out")),
                Term::Str(out.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":requirements-path")),
                Term::Str(req_path.display().to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(LocalPkgResult {
        kind: "genesis/pkg-requirements-trace-v0.1",
        log_op: "pkg-requirements-trace",
        program_hash: hash_term(&value),
        value,
    })
}

pub(crate) fn handle_tool_qualification(
    args: ToolQualificationArgs<'_>,
) -> Result<LocalPkgResult, String> {
    if let Some(c) = args.commit {
        validate_hex_hash(c).map_err(|e| format!("invalid --commit hash: {e}"))?;
    }
    validate_hex_hash(args.snapshot).map_err(|e| format!("invalid --snapshot hash: {e}"))?;
    if let Some(p) = args.policy {
        validate_hex_hash(p).map_err(|e| format!("invalid --policy hash: {e}"))?;
    }
    let reqs = pkg_assurance_ops_requirements::normalize_requirement_ids(args.requirement_ids)?;
    let cwd = std::env::current_dir().map_err(|e| format!("current_dir: {e}"))?;
    let tests = pkg_assurance_ops_qualification::resolve_qualification_tests(
        args.test_artifacts,
        pkg_assurance_ops_qualification::QualificationLineageContext {
            commit: args.commit,
            snapshot: args.snapshot,
            policy: args.policy,
            profile: args.profile,
            store_dir: &cwd.join(".genesis").join("store"),
        },
    )?;
    let tool_specs = pkg_assurance_ops_requirements::parse_tools(args.tools)?;

    let mut tools_term: Vec<Term> = Vec::new();
    for (name, path) in &tool_specs {
        let bytes =
            std::fs::read(path).map_err(|e| format!("read tool {}: {e}", path.display()))?;
        let bh = blake3::hash(&bytes).to_hex().to_string();
        let size = i64::try_from(bytes.len()).map_err(|_| "tool too large".to_string())?;
        tools_term.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(path.display().to_string()),
                ),
                (TermOrdKey(Term::symbol(":blake3")), Term::Str(bh)),
                (
                    TermOrdKey(Term::symbol(":size-bytes")),
                    Term::Int(size.into()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }
    tools_term.sort_by_key(print_term);

    let tests_term: Vec<Term> = tests
        .iter()
        .map(|test| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":id")), Term::Str(test.id.clone())),
                    (
                        TermOrdKey(Term::symbol(":artifact")),
                        Term::Str(test.artifact.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":manifest")),
                        Term::Str(test.manifest_hash.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":run-id")),
                        Term::Str(test.run_id.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":runner")),
                        Term::Str(test.runner.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":profile")),
                        Term::Str(args.profile.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(test.snapshot.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":policy")),
                        test.policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (TermOrdKey(Term::symbol(":result")), Term::symbol(":pass")),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();

    let evidence = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/evidence"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":tool-qualification"),
            ),
            (
                TermOrdKey(Term::symbol(":status")),
                Term::symbol(":qualified"),
            ),
            (
                TermOrdKey(Term::symbol(":toolchain")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":profile")),
                            Term::Str(args.profile.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":driver-version")),
                            Term::Str(env!("CARGO_PKG_VERSION").to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
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
                TermOrdKey(Term::symbol(":requirements")),
                Term::Vector(reqs.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":tools")),
                Term::Vector(tools_term.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":qualification-tests")),
                Term::Vector(tests_term.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let evidence_src = print_term(&evidence) + "\n";
    if let Some(parent) = args.out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(args.out, evidence_src.as_bytes())
        .map_err(|e| format!("write {}: {e}", args.out.display()))?;
    let evidence_h = blake3::hash(evidence_src.as_bytes()).to_hex().to_string();
    if !args.no_store {
        let store = ArtifactStore::open(&cwd.join(".genesis").join("store"))
            .map_err(|e| format!("open store: {e}"))?;
        let _ = store
            .put_bytes(evidence_src.as_bytes())
            .map_err(|e| format!("store tool qualification: {e}"))?;
    }

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":tool-qualification"),
            ),
            (TermOrdKey(Term::symbol(":artifact")), Term::Str(evidence_h)),
            (
                TermOrdKey(Term::symbol(":out")),
                Term::Str(args.out.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":profile")),
                Term::Str(args.profile.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":tools")),
                Term::Int(i64::try_from(tools_term.len()).unwrap_or(0).into()),
            ),
            (
                TermOrdKey(Term::symbol(":qualification-tests")),
                Term::Int(i64::try_from(tests_term.len()).unwrap_or(0).into()),
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-tool-qualification-v0.1",
        log_op: "pkg-tool-qualification",
        program_hash: hash_term(&value),
        value,
    })
}
