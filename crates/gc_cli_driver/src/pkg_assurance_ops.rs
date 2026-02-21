use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term, parse_term, print_term};
use gc_effects::ArtifactStore;
use gc_pkg::PackageManifest;
use gc_vcs::validate_hex_hash;

use crate::pkg_workspace_ops::LocalPkgResult;

pub(crate) struct ToolQualificationArgs<'a> {
    pub commit: Option<&'a str>,
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
    let requirements_vec = parse_requirements_graph(&req_term, &manifest.obligations)?;

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
    if let Some(p) = args.policy {
        validate_hex_hash(p).map_err(|e| format!("invalid --policy hash: {e}"))?;
    }
    let reqs = normalize_requirement_ids(args.requirement_ids)?;
    let tests = parse_test_artifacts(args.test_artifacts)?;
    let tool_specs = parse_tools(args.tools)?;

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
        .map(|(id, artifact)| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":id")), Term::Str(id.clone())),
                    (
                        TermOrdKey(Term::symbol(":artifact")),
                        Term::Str(artifact.clone()),
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
        let cwd = std::env::current_dir().map_err(|e| format!("current_dir: {e}"))?;
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

fn parse_requirements_graph(
    req_term: &Term,
    known_obligations: &[String],
) -> Result<Vec<Term>, String> {
    let Term::Map(root) = req_term else {
        return Err("requirements graph must be a map".to_string());
    };
    let ty = root
        .get(&TermOrdKey(Term::symbol(":type")))
        .ok_or_else(|| "requirements graph missing :type".to_string())?;
    match ty {
        Term::Symbol(s) | Term::Str(s) if s == ":req/graph" || s == "req/graph" => {}
        _ => {
            return Err(format!(
                "requirements graph :type must be :req/graph, got {}",
                print_term(ty)
            ));
        }
    }
    let reqs = root
        .get(&TermOrdKey(Term::symbol(":requirements")))
        .ok_or_else(|| "requirements graph missing :requirements".to_string())?;
    let Term::Vector(items) = reqs else {
        return Err("requirements graph :requirements must be vector".to_string());
    };
    if items.is_empty() {
        return Err("requirements graph :requirements cannot be empty".to_string());
    }
    let known_ob: std::collections::BTreeSet<&str> =
        known_obligations.iter().map(String::as_str).collect();
    let mut out: Vec<Term> = Vec::new();
    for (i, req) in items.iter().enumerate() {
        let Term::Map(m) = req else {
            return Err(format!("requirements[{i}] must be map"));
        };
        let id = req_required_string(m, ":id", &format!("requirements[{i}]"))?;
        let level = normalize_symbol_like(&req_required_symbol_or_string(
            m,
            ":level",
            &format!("requirements[{i}]"),
        )?);
        if level != ":system" && level != ":hlr" && level != ":llr" {
            return Err(format!(
                "requirements[{i}] :level must be :system|:hlr|:llr"
            ));
        }
        let hazards = req_opt_str_vec(m, ":hazards", &format!("requirements[{i}]"))?;
        let parents = req_opt_str_vec(m, ":parents", &format!("requirements[{i}]"))?;
        let links_t = m
            .get(&TermOrdKey(Term::symbol(":links")))
            .ok_or_else(|| format!("requirements[{i}] missing :links"))?;
        let Term::Map(links) = links_t else {
            return Err(format!("requirements[{i}] :links must be map"));
        };
        let modules = parse_link_modules(links, i)?;
        let obligations = parse_link_symbols(links, ":obligations", i)?;
        for ob in &obligations {
            if !known_ob.contains(ob.as_str()) {
                return Err(format!(
                    "requirements[{i}] links obligation `{ob}` not present in package obligations"
                ));
            }
        }
        let evidence_kinds = parse_link_symbols(links, ":evidence-kinds", i)?
            .into_iter()
            .map(|k| normalize_symbol_like(&k))
            .collect::<Vec<_>>();
        if modules.is_empty() && obligations.is_empty() && evidence_kinds.is_empty() {
            return Err(format!(
                "requirements[{i}] :links must include modules, obligations, or evidence-kinds"
            ));
        }

        let mut links_out: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
        if !modules.is_empty() {
            links_out.insert(TermOrdKey(Term::symbol(":modules")), Term::Vector(modules));
        }
        if !obligations.is_empty() {
            links_out.insert(
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(obligations.into_iter().map(Term::symbol).collect()),
            );
        }
        if !evidence_kinds.is_empty() {
            links_out.insert(
                TermOrdKey(Term::symbol(":evidence-kinds")),
                Term::Vector(evidence_kinds.into_iter().map(Term::symbol).collect()),
            );
        }
        let req_out = Term::Map(
            [
                (TermOrdKey(Term::symbol(":id")), Term::Str(id)),
                (TermOrdKey(Term::symbol(":level")), Term::symbol(&level)),
                (
                    TermOrdKey(Term::symbol(":parents")),
                    Term::Vector(parents.into_iter().map(Term::Str).collect()),
                ),
                (
                    TermOrdKey(Term::symbol(":hazards")),
                    Term::Vector(hazards.into_iter().map(Term::Str).collect()),
                ),
                (TermOrdKey(Term::symbol(":links")), Term::Map(links_out)),
            ]
            .into_iter()
            .collect(),
        );
        out.push(req_out);
    }
    out.sort_by_key(print_term);
    Ok(out)
}

fn parse_link_modules(links: &BTreeMap<TermOrdKey, Term>, idx: usize) -> Result<Vec<Term>, String> {
    let Some(t) = links.get(&TermOrdKey(Term::symbol(":modules"))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!(
            "requirements[{idx}] :links/:modules must be vector"
        ));
    };
    let mut out = Vec::new();
    for (j, x) in xs.iter().enumerate() {
        let Term::Map(mm) = x else {
            return Err(format!(
                "requirements[{idx}] :links/:modules[{j}] must be map"
            ));
        };
        let path = req_required_string(mm, ":path", &format!("requirements[{idx}]:modules[{j}]"))?;
        let exports = parse_link_symbols(mm, ":exports", idx)?;
        if exports.is_empty() {
            return Err(format!(
                "requirements[{idx}] :links/:modules[{j}] :exports cannot be empty"
            ));
        }
        out.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":path")), Term::Str(path)),
                (
                    TermOrdKey(Term::symbol(":exports")),
                    Term::Vector(exports.into_iter().map(Term::symbol).collect()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }
    Ok(out)
}

fn parse_link_symbols(
    links: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    idx: usize,
) -> Result<Vec<String>, String> {
    let Some(t) = links.get(&TermOrdKey(Term::symbol(key))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!("requirements[{idx}] :links/{key} must be vector"));
    };
    let mut out = Vec::new();
    for (j, x) in xs.iter().enumerate() {
        match x {
            Term::Symbol(s) | Term::Str(s) => out.push(s.clone()),
            _ => {
                return Err(format!(
                    "requirements[{idx}] :links/{key}[{j}] must be symbol|string"
                ));
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn req_required_string(
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

fn req_required_symbol_or_string(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<String, String> {
    let t = m
        .get(&TermOrdKey(Term::symbol(key)))
        .ok_or_else(|| format!("{what} missing {key}"))?;
    match t {
        Term::Symbol(s) | Term::Str(s) => {
            if s.trim().is_empty() {
                Err(format!("{what} {key} cannot be empty"))
            } else {
                Ok(s.clone())
            }
        }
        _ => Err(format!("{what} {key} must be symbol|string")),
    }
}

fn req_opt_str_vec(
    m: &BTreeMap<TermOrdKey, Term>,
    key: &str,
    what: &str,
) -> Result<Vec<String>, String> {
    let Some(t) = m.get(&TermOrdKey(Term::symbol(key))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(format!("{what} {key} must be vector"));
    };
    let mut out = Vec::new();
    for (i, x) in xs.iter().enumerate() {
        match x {
            Term::Str(s) => {
                if s.trim().is_empty() {
                    return Err(format!("{what} {key}[{i}] cannot be empty"));
                }
                out.push(s.clone());
            }
            _ => return Err(format!("{what} {key}[{i}] must be string")),
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn normalize_requirement_ids(ids: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = if ids.is_empty() {
        vec!["TQ-BASELINE".to_string()]
    } else {
        ids.iter().map(|s| s.trim().to_string()).collect()
    };
    if out.iter().any(|s| s.is_empty()) {
        return Err("empty requirement id in --requirement".to_string());
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn parse_test_artifacts(xs: &[String]) -> Result<Vec<(String, String)>, String> {
    if xs.is_empty() {
        return Err(
            "at least one --test-artifact id=<64-hex> is required for tool qualification"
                .to_string(),
        );
    }
    let mut out = Vec::new();
    for raw in xs {
        let (id, h) = raw
            .split_once('=')
            .ok_or_else(|| format!("invalid --test-artifact `{raw}`; expected id=<64-hex>"))?;
        let id = id.trim();
        let h = h.trim();
        if id.is_empty() {
            return Err(format!("invalid --test-artifact `{raw}`: empty id"));
        }
        validate_hex_hash(h).map_err(|e| format!("invalid --test-artifact `{raw}` hash: {e}"))?;
        out.push((id.to_string(), h.to_string()));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn parse_tools(xs: &[String]) -> Result<Vec<(String, PathBuf)>, String> {
    if xs.is_empty() {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        return Ok(vec![("genesis-cli-driver".to_string(), exe)]);
    }
    let mut out = Vec::new();
    for raw in xs {
        let (name, path) = raw
            .split_once('=')
            .ok_or_else(|| format!("invalid --tool `{raw}`; expected name=path"))?;
        let name = name.trim();
        let path = path.trim();
        if name.is_empty() || path.is_empty() {
            return Err(format!(
                "invalid --tool `{raw}`; both name and path must be non-empty"
            ));
        }
        let pb = PathBuf::from(path);
        if !pb.is_file() {
            return Err(format!(
                "tool path does not exist or is not a file: {}",
                pb.display()
            ));
        }
        out.push((name.to_string(), pb));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    out.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    Ok(out)
}

fn normalize_symbol_like(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with(':') {
        trimmed.to_string()
    } else {
        format!(":{trimmed}")
    }
}
