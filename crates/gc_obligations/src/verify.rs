use std::path::Path;

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use gc_kernel::{MemLimits, StepLimit};

use crate::{
    AcceptanceSignature, EvidenceStore, ObligationError, PackageManifest, RegistryPolicy,
    load_signature_set, signatures_file_path,
};

#[derive(Debug, Clone)]
pub struct PackageVerifyResult {
    pub ok: bool,
    pub errors: Vec<String>,

    pub checked_modules: usize,
    pub checked_deps: usize,
    pub checked_artifacts: usize,

    pub acceptance_artifact: Option<String>,
    pub store_scanned: bool,

    pub checked_signatures: usize,
    pub valid_signatures: usize,
    pub policy_min_signatures: Option<u64>,
}

pub fn verify_package(
    pkg_toml: &Path,
    acceptance_artifact: Option<&str>,
    scan_store: bool,
) -> Result<PackageVerifyResult, ObligationError> {
    verify_package_with_policy(pkg_toml, acceptance_artifact, scan_store, None, None)
}

pub fn verify_package_with_policy(
    pkg_toml: &Path,
    acceptance_artifact: Option<&str>,
    scan_store: bool,
    policy: Option<&Path>,
    signatures: Option<&Path>,
) -> Result<PackageVerifyResult, ObligationError> {
    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let store = EvidenceStore::open(&pkg_dir)?;

    let mut errors: Vec<String> = Vec::new();

    let mut checked_modules = 0usize;
    let mut checked_artifacts = 0usize;
    let mut checked_signatures = 0usize;
    let mut valid_signatures = 0usize;
    let checked_deps = manifest.dependencies.len();

    // Modules: pinned hashes must exist and match computed hashes.
    let limits = super::KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    };
    let frontend = super::default_coreform_frontend();
    match super::load_modules(&pkg_dir, &manifest.modules, &frontend, limits) {
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
    if let Err(e) = super::check_dep_hashes(&pkg_dir, &manifest.dependencies, &frontend, limits) {
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

    // Registry policy enforcement (optional).
    let mut policy_min_signatures: Option<u64> = None;
    if let Some(policy_path) = policy {
        match RegistryPolicy::load(policy_path) {
            Ok(pol) => {
                policy_min_signatures = Some(pol.min_signatures);
                if pol.min_signatures > 0 {
                    let acc_hex = acceptance_artifact.as_deref();
                    if acc_hex.is_none() {
                        errors.push(
                            "policy requires acceptance artifact but none was found".to_string(),
                        );
                    }

                    let acc_bytes = acc_hex.and_then(|h| hex32_to_bytes(h).ok());
                    if acc_hex.is_some() && acc_bytes.is_none() {
                        errors.push("invalid acceptance artifact hash (not 64-hex)".to_string());
                    }

                    match (pol.allowed_verifying_keys(), acc_hex, acc_bytes) {
                        (Ok(allowed), Some(acc_hex), Some(acc_bytes)) => {
                            let sigset_path = signatures
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| signatures_file_path(&pkg_dir));
                            match load_signature_set(&sigset_path) {
                                Ok(sigs) => {
                                    for sh in sigs {
                                        match store.verify_hex(&sh) {
                                            Ok(()) => {
                                                checked_artifacts =
                                                    checked_artifacts.saturating_add(1)
                                            }
                                            Err(e) => {
                                                errors.push(format!("{e}"));
                                                continue;
                                            }
                                        }
                                        checked_signatures = checked_signatures.saturating_add(1);
                                        match read_term_from_store(&store, &sh) {
                                            Ok(t) => match AcceptanceSignature::from_term(&t) {
                                                Ok(rec) => {
                                                    if rec.acceptance_hash != acc_bytes {
                                                        errors.push(format!(
                                                            "signature {} does not match acceptance artifact {}",
                                                            sh,
                                                            acc_hex
                                                        ));
                                                        continue;
                                                    }
                                                    if rec.verify(&allowed).is_ok() {
                                                        valid_signatures =
                                                            valid_signatures.saturating_add(1);
                                                    } else {
                                                        errors.push(format!(
                                                            "invalid signature {}",
                                                            sh
                                                        ));
                                                    }
                                                }
                                                Err(e) => errors.push(format!("{e}")),
                                            },
                                            Err(e) => errors.push(format!("{e}")),
                                        }
                                    }
                                }
                                Err(e) => errors.push(format!("{e}")),
                            }

                            if valid_signatures < pol.min_signatures as usize {
                                errors.push(format!(
                                    "policy requires {} valid signatures but found {}",
                                    pol.min_signatures, valid_signatures
                                ));
                            }
                        }
                        (Err(e), _, _) => errors.push(format!("{e}")),
                        (_, _, _) => {}
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
        checked_signatures,
        valid_signatures,
        policy_min_signatures,
    })
}

fn hex32_to_bytes(s: &str) -> Result<[u8; 32], ()> {
    let t = s.trim();
    if t.len() != 64 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(());
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        let hi = hex_val(t.as_bytes()[2 * i]).ok_or(())?;
        let lo = hex_val(t.as_bytes()[2 * i + 1]).ok_or(())?;
        *b = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
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
