use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::Context;

use super::*;

#[derive(Debug, Clone)]
pub(super) struct ToolchainManifest {
    pub(super) module_paths: Vec<String>,
    pub(super) required_symbols: Vec<String>,
}

static TOOLCHAIN_MANIFEST: OnceLock<Result<ToolchainManifest, String>> = OnceLock::new();
static EMBEDDED_MODULE_SOURCES: OnceLock<Result<Vec<(String, String)>, String>> = OnceLock::new();

fn parse_module_paths_vec(t: &Term, field: &str) -> anyhow::Result<Vec<String>> {
    let Term::Vector(v) = t else {
        return Err(anyhow::anyhow!("{field} must be a vector"));
    };
    let mut out = Vec::with_capacity(v.len());
    let mut seen = BTreeSet::new();
    for item in v {
        let Term::Str(s) = item else {
            return Err(anyhow::anyhow!("{field} entries must be strings"));
        };
        if s.trim().is_empty() {
            return Err(anyhow::anyhow!("{field} cannot contain empty paths"));
        }
        if !seen.insert(s.clone()) {
            return Err(anyhow::anyhow!("{field} contains duplicate path: {s}"));
        }
        out.push(s.clone());
    }
    if out.is_empty() {
        return Err(anyhow::anyhow!("{field} must not be empty"));
    }
    Ok(out)
}

fn parse_required_symbols_vec(t: &Term, field: &str) -> anyhow::Result<Vec<String>> {
    let Term::Vector(v) = t else {
        return Err(anyhow::anyhow!("{field} must be a vector"));
    };
    let mut out = Vec::with_capacity(v.len());
    let mut seen = BTreeSet::new();
    for item in v {
        let sym = match item {
            Term::Symbol(s) => s.clone(),
            Term::Str(s) => s.clone(),
            _ => {
                return Err(anyhow::anyhow!(
                    "{field} entries must be symbols or strings"
                ));
            }
        };
        if sym.trim().is_empty() {
            return Err(anyhow::anyhow!("{field} cannot contain empty symbol names"));
        }
        if !seen.insert(sym.clone()) {
            return Err(anyhow::anyhow!("{field} contains duplicate symbol: {sym}"));
        }
        out.push(sym);
    }
    Ok(out)
}

fn parse_toolchain_manifest_src(src: &str) -> anyhow::Result<ToolchainManifest> {
    let term = parse_term(src).map_err(|e| anyhow::anyhow!("manifest parse: {e}"))?;
    let root = match term {
        Term::Map(m) => m,
        _ => return Err(anyhow::anyhow!("manifest root must be a map")),
    };
    let kind = match super::map_get(&root, ":kind") {
        Some(Term::Str(s)) => s.as_str(),
        _ => return Err(anyhow::anyhow!("manifest missing :kind string")),
    };
    if kind != "genesis/selfhost-toolchain-manifest-v0.2" {
        return Err(anyhow::anyhow!(
            "manifest :kind mismatch: expected genesis/selfhost-toolchain-manifest-v0.2, got {kind}"
        ));
    }
    let v = match super::map_get(&root, ":v") {
        Some(Term::Int(i)) => i,
        _ => return Err(anyhow::anyhow!("manifest missing :v int")),
    };
    if v != &1.into() {
        return Err(anyhow::anyhow!("manifest :v must be 1, got {v}"));
    }
    let module_paths = parse_module_paths_vec(
        super::map_get(&root, ":module-paths")
            .ok_or_else(|| anyhow::anyhow!("manifest missing :module-paths"))?,
        ":module-paths",
    )?;
    let required_symbols = parse_required_symbols_vec(
        super::map_get(&root, ":required-symbols")
            .ok_or_else(|| anyhow::anyhow!("manifest missing :required-symbols"))?,
        ":required-symbols",
    )?;
    Ok(ToolchainManifest {
        module_paths,
        required_symbols,
    })
}

pub(super) fn toolchain_manifest() -> anyhow::Result<&'static ToolchainManifest> {
    let r = TOOLCHAIN_MANIFEST.get_or_init(|| {
        parse_toolchain_manifest_src(SELFHOST_TOOLCHAIN_MANIFEST_SRC).map_err(|e| e.to_string())
    });
    match r {
        Ok(m) => Ok(m),
        Err(e) => Err(anyhow::anyhow!("selfhost toolchain manifest: {e}")),
    }
}

fn parse_embedded_artifact_sources() -> anyhow::Result<BTreeMap<String, String>> {
    let term = parse_term(SELFHOST_TOOLCHAIN_EMBEDDED_ARTIFACT_SRC)
        .map_err(|e| anyhow::anyhow!("embedded toolchain artifact parse: {e}"))?;
    let root = match term {
        Term::Map(m) => m,
        _ => return Err(anyhow::anyhow!("embedded artifact root must be a map")),
    };
    let kind = match super::map_get(&root, ":kind") {
        Some(Term::Str(s)) => s.as_str(),
        _ => return Err(anyhow::anyhow!("embedded artifact missing :kind string")),
    };
    if kind != SELFHOST_TOOLCHAIN_ARTIFACT_KIND {
        return Err(anyhow::anyhow!(
            "embedded artifact :kind mismatch: expected {SELFHOST_TOOLCHAIN_ARTIFACT_KIND}, got {kind}"
        ));
    }
    let modules = match super::map_get(&root, ":modules") {
        Some(Term::Vector(v)) => v,
        _ => return Err(anyhow::anyhow!("embedded artifact missing :modules vector")),
    };
    let mut out = BTreeMap::new();
    for m in modules {
        let Term::Map(mm) = m else {
            return Err(anyhow::anyhow!(
                "embedded artifact module entry must be map"
            ));
        };
        let path = match super::map_get(mm, ":path") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(anyhow::anyhow!(
                    "embedded artifact module missing :path string"
                ));
            }
        };
        let src = match super::map_get(mm, ":source") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(anyhow::anyhow!(
                    "embedded artifact module {path} missing :source string"
                ));
            }
        };
        if out.insert(path.clone(), src).is_some() {
            return Err(anyhow::anyhow!(
                "embedded artifact module has duplicate path: {path}"
            ));
        }
    }
    Ok(out)
}

fn read_manifest_sources_from_workspace(
    manifest: &ToolchainManifest,
) -> anyhow::Result<Vec<(String, String)>> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::with_capacity(manifest.module_paths.len());
    for path in &manifest.module_paths {
        let full = repo_root.join(path);
        let src = std::fs::read_to_string(&full)
            .with_context(|| format!("read selfhost module {}", full.display()))?;
        out.push((path.clone(), src));
    }
    Ok(out)
}

pub fn selfhost_coreform_toolchain_v1_sources() -> anyhow::Result<Vec<(String, String)>> {
    let r = EMBEDDED_MODULE_SOURCES.get_or_init(|| {
        let manifest = toolchain_manifest().map_err(|e| e.to_string())?;
        if let Ok(sources) = read_manifest_sources_from_workspace(manifest) {
            return Ok(sources);
        }

        let mut embedded = parse_embedded_artifact_sources().map_err(|e| e.to_string())?;
        let mut ordered = Vec::with_capacity(manifest.module_paths.len());
        for path in &manifest.module_paths {
            let src = embedded.remove(path).ok_or_else(|| {
                format!("embedded artifact missing manifest module source: {path}")
            })?;
            ordered.push((path.clone(), src));
        }
        Ok(ordered)
    });
    match r {
        Ok(v) => Ok(v.clone()),
        Err(e) => Err(anyhow::anyhow!(
            "selfhost toolchain embedded sources unavailable: {e}"
        )),
    }
}

pub fn embedded_bootstrap_available() -> bool {
    cfg!(feature = "embedded-bootstrap")
}
