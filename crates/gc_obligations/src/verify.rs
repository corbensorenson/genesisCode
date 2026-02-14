use std::path::Path;

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};

use crate::{EvidenceStore, ObligationError, PackageManifest};

#[derive(Debug, Clone)]
pub struct PackageVerifyResult {
    pub ok: bool,
    pub errors: Vec<String>,

    pub checked_modules: usize,
    pub checked_deps: usize,
    pub checked_artifacts: usize,

    pub acceptance_artifact: Option<String>,
    pub store_scanned: bool,
}

pub fn verify_package(
    pkg_toml: &Path,
    acceptance_artifact: Option<&str>,
    scan_store: bool,
) -> Result<PackageVerifyResult, ObligationError> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg_toml)?;
    let store = EvidenceStore::open(&pkg_dir)?;

    let mut errors: Vec<String> = Vec::new();

    let mut checked_modules = 0usize;
    let mut checked_artifacts = 0usize;
    let checked_deps = manifest.dependencies.len();

    // Modules: pinned hashes must exist and match computed hashes.
    match super::load_modules(&pkg_dir, &manifest.modules) {
        Ok(modules) => {
            for m in &modules {
                checked_modules = checked_modules.saturating_add(1);
                let want = m.entry.hash.as_deref().unwrap_or("");
                if want.is_empty() {
                    errors.push(format!(
                        "module {} is missing pinned hash; run `genesis pack --pkg {}`",
                        m.entry.path,
                        pkg_toml.display()
                    ));
                    continue;
                }
                let got = super::hex32(m.hash);
                if want != got {
                    errors.push(format!(
                        "module hash mismatch for {}: manifest has {}, computed {}",
                        m.entry.path, want, got
                    ));
                }
            }
        }
        Err(e) => errors.push(format!("{e}")),
    }

    // Dependencies: pinned package hashes must exist and match.
    if let Err(e) = super::check_dep_hashes(&pkg_dir, &manifest.dependencies) {
        errors.push(format!("{e}"));
    }

    // Evidence: verify the latest acceptance artifact (or caller-specified) and any referenced
    // obligation artifacts.
    let acceptance_artifact = acceptance_artifact
        .map(|s| s.trim().to_string())
        .or_else(|| read_last_acceptance(&pkg_dir));
    if let Some(hex) = acceptance_artifact.as_deref() {
        if let Err(e) = store.verify_hex(hex) {
            errors.push(format!("{e}"));
        } else {
            checked_artifacts = checked_artifacts.saturating_add(1);
        }

        match read_term_from_store(&store, hex) {
            Ok(t) => {
                if let Err(es) = verify_acceptance_kind(&t) {
                    errors.extend(es);
                }
                for a in referenced_artifacts(&t) {
                    match store.verify_hex(&a) {
                        Ok(()) => checked_artifacts = checked_artifacts.saturating_add(1),
                        Err(e) => errors.push(format!("{e}")),
                    }
                }
            }
            Err(e) => errors.push(format!("{e}")),
        }
    }

    if scan_store {
        // Verify all store artifacts by name->content hash.
        match std::fs::read_dir(store.root_dir()) {
            Ok(it) => {
                for entry in it.flatten() {
                    let Ok(ft) = entry.file_type() else { continue };
                    if !ft.is_file() {
                        continue;
                    }
                    let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                        continue;
                    };
                    if name.starts_with(".tmp-") {
                        continue;
                    }
                    if !looks_like_hex32(&name) {
                        continue;
                    }
                    match store.verify_hex(&name) {
                        Ok(()) => checked_artifacts = checked_artifacts.saturating_add(1),
                        Err(e) => errors.push(format!("{e}")),
                    }
                }
            }
            Err(e) => errors.push(format!("cannot scan store: {e}")),
        }
    }

    let ok = errors.is_empty();
    Ok(PackageVerifyResult {
        ok,
        errors,
        checked_modules,
        checked_deps,
        checked_artifacts,
        acceptance_artifact,
        store_scanned: scan_store,
    })
}

fn read_last_acceptance(pkg_dir: &Path) -> Option<String> {
    let p = pkg_dir.join(".genesis").join("last_acceptance");
    let s = std::fs::read_to_string(p).ok()?;
    let t = s.trim();
    if looks_like_hex32(t) {
        Some(t.to_string())
    } else {
        None
    }
}

fn looks_like_hex32(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn read_term_from_store(store: &EvidenceStore, hex: &str) -> Result<Term, ObligationError> {
    let p = store.path_for(hex);
    let s = std::fs::read_to_string(&p)?;
    parse_term(&s).map_err(|e| ObligationError::Store(format!("bad artifact {}: {e}", p.display())))
}

fn verify_acceptance_kind(t: &Term) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let Term::Map(m) = t else {
        return Err(vec!["acceptance artifact must be a map".to_string()]);
    };
    let kind = m.get(&TermOrdKey(Term::symbol(":kind")));
    if !matches!(kind, Some(Term::Str(s)) if s == "genesis/acceptance-v0.2") {
        errors.push(format!(
            "acceptance artifact has wrong :kind: expected \"genesis/acceptance-v0.2\", got {}",
            kind.map(print_term).unwrap_or_else(|| "nil".to_string())
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn referenced_artifacts(t: &Term) -> Vec<String> {
    let Term::Map(m) = t else {
        return Vec::new();
    };
    let Some(Term::Vector(obs)) = m.get(&TermOrdKey(Term::symbol(":obligations"))) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for o in obs {
        let Term::Map(om) = o else {
            continue;
        };
        let Some(Term::Str(hex)) = om.get(&TermOrdKey(Term::symbol(":artifact"))) else {
            continue;
        };
        if looks_like_hex32(hex) {
            out.push(hex.clone());
        }
    }
    out
}
