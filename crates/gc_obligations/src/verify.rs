use std::path::Path;

use gc_coreform::{Term, TermOrdKey, parse_term, print_term};
use gc_kernel::{MemLimits, StepLimit};

use crate::{
    AcceptanceSignature, EvidenceFact, EvidenceStore, EvidenceVerifyAuthority, ObligationError,
    PackageManifest, RegistryPolicy, load_signature_set, signatures_file_path,
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

#[cfg(feature = "parity-oracle")]
pub fn verify_package(
    pkg_toml: &Path,
    acceptance_artifact: Option<&str>,
    scan_store: bool,
) -> Result<PackageVerifyResult, ObligationError> {
    verify_package_with_policy(pkg_toml, acceptance_artifact, scan_store, None, None)
}

#[cfg(feature = "parity-oracle")]
pub fn verify_package_with_policy(
    pkg_toml: &Path,
    acceptance_artifact: Option<&str>,
    scan_store: bool,
    policy: Option<&Path>,
    signatures: Option<&Path>,
) -> Result<PackageVerifyResult, ObligationError> {
    let artifact = Path::new("selfhost/toolchain.gc");
    verify_package_with_policy_and_authority(
        pkg_toml,
        acceptance_artifact,
        scan_store,
        policy,
        signatures,
        artifact,
    )
}

pub fn verify_package_with_policy_and_authority(
    pkg_toml: &Path,
    acceptance_artifact: Option<&str>,
    scan_store: bool,
    policy: Option<&Path>,
    signatures: Option<&Path>,
    authority_artifact: &Path,
) -> Result<PackageVerifyResult, ObligationError> {
    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let store = EvidenceStore::open(&pkg_dir)?;

    let mut facts: Vec<EvidenceFact> = Vec::new();

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
                    facts.push(identity_fact(
                        "package/module-hash-missing",
                        Term::Nil,
                        Term::Str(super::hex32(m.hash)),
                    ));
                    continue;
                }
                let got = super::hex32(m.hash);
                facts.push(identity_fact(
                    "package/module-hash-mismatch",
                    Term::Str(got),
                    Term::Str(want.to_string()),
                ));
            }
        }
        Err(e) => facts.push(mechanism_fact("package/module-load", false, e.to_string())),
    }

    // Dependencies: pinned package hashes must exist and match.
    if let Err(e) = super::check_dep_hashes(&pkg_dir, &manifest.dependencies, &frontend, limits) {
        facts.push(mechanism_fact(
            "package/dependency-integrity",
            false,
            e.to_string(),
        ));
    } else {
        facts.push(mechanism_fact("package/dependency-integrity", true, ""));
    }

    // Evidence: verify the latest acceptance artifact (or caller-specified) and any referenced
    // obligation artifacts.
    let acceptance_artifact = if let Some(value) = acceptance_artifact {
        Some(value.trim().to_string())
    } else {
        match read_last_acceptance(&pkg_dir) {
            Ok(value) => value,
            Err(error) => {
                facts.push(mechanism_fact("evidence/last-acceptance", false, error));
                None
            }
        }
    };
    if let Some(hex) = acceptance_artifact.as_deref() {
        if let Err(e) = store.verify_hex(hex) {
            facts.push(mechanism_fact(
                "evidence/acceptance-store-integrity",
                false,
                e.to_string(),
            ));
        } else {
            checked_artifacts = checked_artifacts.saturating_add(1);
            facts.push(mechanism_fact(
                "evidence/acceptance-store-integrity",
                true,
                "",
            ));
        }

        match read_term_from_store(&store, hex) {
            Ok(t) => {
                if let Err(es) = verify_acceptance_kind(&t) {
                    for error in es {
                        facts.push(mechanism_fact("evidence/acceptance-schema", false, error));
                    }
                } else {
                    facts.push(mechanism_fact("evidence/acceptance-schema", true, ""));
                }
                for a in referenced_artifacts(&t) {
                    match store.verify_hex(&a) {
                        Ok(()) => {
                            checked_artifacts = checked_artifacts.saturating_add(1);
                            facts.push(mechanism_fact(
                                "evidence/referenced-artifact-integrity",
                                true,
                                "",
                            ));
                        }
                        Err(e) => facts.push(mechanism_fact(
                            "evidence/referenced-artifact-integrity",
                            false,
                            e.to_string(),
                        )),
                    }
                }
            }
            Err(e) => facts.push(mechanism_fact(
                "evidence/acceptance-load",
                false,
                e.to_string(),
            )),
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
                        facts.push(presence_fact("policy/acceptance-required", false, true));
                    }

                    let acc_bytes = acc_hex.and_then(|h| hex32_to_bytes(h).ok());
                    if acc_hex.is_some() && acc_bytes.is_none() {
                        facts.push(mechanism_fact(
                            "policy/acceptance-hash",
                            false,
                            "invalid acceptance hash",
                        ));
                    }

                    match (pol.allowed_verifying_keys(), acc_hex, acc_bytes) {
                        (Ok(allowed), Some(_acc_hex), Some(acc_bytes)) => {
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
                                                facts.push(mechanism_fact(
                                                    "policy/signature-store-integrity",
                                                    false,
                                                    e.to_string(),
                                                ));
                                                continue;
                                            }
                                        }
                                        checked_signatures = checked_signatures.saturating_add(1);
                                        match read_term_from_store(&store, &sh) {
                                            Ok(t) => {
                                                match AcceptanceSignature::from_term(&t) {
                                                    Ok(rec) => {
                                                        if rec.acceptance_hash != acc_bytes {
                                                            facts.push(identity_fact(
                                                            "policy/signature-acceptance-mismatch",
                                                            Term::Bytes(rec.acceptance_hash.to_vec().into()),
                                                            Term::Bytes(acc_bytes.to_vec().into()),
                                                        ));
                                                            continue;
                                                        }
                                                        if rec.verify(&allowed).is_ok() {
                                                            valid_signatures =
                                                                valid_signatures.saturating_add(1);
                                                            facts.push(crypto_fact(
                                                                "policy/signature-invalid",
                                                                true,
                                                            ));
                                                        } else {
                                                            facts.push(crypto_fact(
                                                                "policy/signature-invalid",
                                                                false,
                                                            ));
                                                        }
                                                    }
                                                    Err(e) => facts.push(mechanism_fact(
                                                        "policy/signature-schema",
                                                        false,
                                                        e.to_string(),
                                                    )),
                                                }
                                            }
                                            Err(e) => facts.push(mechanism_fact(
                                                "policy/signature-load",
                                                false,
                                                e.to_string(),
                                            )),
                                        }
                                    }
                                }
                                Err(e) => facts.push(mechanism_fact(
                                    "policy/signature-set",
                                    false,
                                    e.to_string(),
                                )),
                            }

                            facts.push(at_least_fact(
                                "policy/signature-threshold",
                                valid_signatures,
                                pol.min_signatures as usize,
                            ));
                        }
                        (Err(e), _, _) => {
                            facts.push(mechanism_fact("policy/key-decode", false, e.to_string()))
                        }
                        (_, _, _) => {}
                    }
                }
            }
            Err(e) => facts.push(mechanism_fact("policy/load", false, e.to_string())),
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
                        Ok(()) => {
                            checked_artifacts = checked_artifacts.saturating_add(1);
                            facts.push(mechanism_fact("evidence/store-scan", true, ""));
                        }
                        Err(e) => {
                            facts.push(mechanism_fact("evidence/store-scan", false, e.to_string()))
                        }
                    }
                }
            }
            Err(e) => facts.push(mechanism_fact("evidence/store-scan", false, e.to_string())),
        }
    }

    let mut authority = EvidenceVerifyAuthority::load(authority_artifact)
        .map_err(|error| ObligationError::Store(error.to_string()))?;
    let decision = authority
        .package(facts)
        .map_err(|error| ObligationError::Store(error.to_string()))?;
    Ok(PackageVerifyResult {
        ok: decision.verified,
        errors: decision.errors,
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

fn read_last_acceptance(pkg_dir: &Path) -> Result<Option<String>, String> {
    let p = pkg_dir.join(".genesis").join("last_acceptance");
    let s = match std::fs::read_to_string(&p) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", p.display())),
    };
    let t = s.trim();
    if looks_like_hex32(t) {
        Ok(Some(t.to_string()))
    } else {
        Err(format!("{}: malformed acceptance pointer", p.display()))
    }
}

fn identity_fact(code: &str, observed: Term, required: Term) -> EvidenceFact {
    EvidenceFact {
        class: ":identity",
        code: code.to_string(),
        mechanism_ok: true,
        observed,
        required,
    }
}

fn presence_fact(code: &str, observed: bool, required: bool) -> EvidenceFact {
    EvidenceFact {
        class: ":presence",
        code: code.to_string(),
        mechanism_ok: true,
        observed: Term::Bool(observed),
        required: Term::Bool(required),
    }
}

fn crypto_fact(code: &str, valid: bool) -> EvidenceFact {
    EvidenceFact {
        class: ":crypto",
        code: code.to_string(),
        mechanism_ok: true,
        observed: Term::Bool(valid),
        required: Term::Bool(true),
    }
}

fn at_least_fact(code: &str, observed: usize, required: usize) -> EvidenceFact {
    EvidenceFact {
        class: ":at-least",
        code: code.to_string(),
        mechanism_ok: true,
        observed: Term::Int(observed.into()),
        required: Term::Int(required.into()),
    }
}

fn mechanism_fact(code: &str, valid: bool, detail: impl Into<String>) -> EvidenceFact {
    EvidenceFact {
        class: ":schema",
        code: if valid {
            code.to_string()
        } else {
            format!("{code}: {}", detail.into())
        },
        mechanism_ok: valid,
        observed: Term::Bool(valid),
        required: Term::Bool(true),
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
