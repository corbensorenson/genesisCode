use std::path::Path;

use gc_coreform::{Term, parse_term};
use gc_kernel::{MemLimits, StepLimit};

use crate::load_package_manifest_with_frontend;
use crate::{
    EvidenceFact, EvidenceStore, EvidenceVerifyAuthority, ObligationError,
    PackageVerificationRequest, PolicyKeyObservation, RegistryPolicy, RegistryPolicyObservation,
    SignatureObservation, StoreHashObservation, signatures_file_path,
    verify_acceptance_signature_mechanism,
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
    let frontend = super::default_coreform_frontend();
    let (manifest, pkg_dir) = load_package_manifest_with_frontend(pkg_toml, &frontend)?;
    let store = EvidenceStore::open(&pkg_dir)?;

    let mut facts: Vec<EvidenceFact> = Vec::new();

    let mut checked_modules = 0usize;
    let mut checked_artifacts = 0usize;
    let mut checked_signatures = 0usize;
    let checked_deps = manifest.dependencies.len();

    // Modules: pinned hashes must exist and match computed hashes.
    let limits = super::KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    };
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
        Err(e) => facts.push(transport_fact("package/module-load", false, e.to_string())),
    }

    // Dependencies: transport every computed and declared hash; GenesisCode compares them.
    match super::observe_dep_hashes(&pkg_dir, &manifest.dependencies, &frontend, limits) {
        Ok(observations) => {
            for (name, required, observed) in observations {
                facts.push(identity_fact(
                    &format!("package/dependency-hash-mismatch:{name}"),
                    Term::Str(observed),
                    required.map(Term::Str).unwrap_or(Term::Nil),
                ));
            }
        }
        Err(error) => facts.push(transport_fact(
            "package/dependency-load",
            false,
            error.to_string(),
        )),
    }

    // Evidence: verify the latest acceptance artifact (or caller-specified) and any referenced
    // obligation artifacts.
    let acceptance_artifact = if let Some(value) = acceptance_artifact {
        Some(value.trim().to_string())
    } else {
        match read_last_acceptance(&pkg_dir) {
            Ok(value) => value,
            Err(error) => {
                facts.push(transport_fact("evidence/last-acceptance", false, error));
                None
            }
        }
    };
    let mut store_observations = Vec::new();
    let mut acceptance = Term::Nil;
    let mut acceptance_hash = None;
    if let Some(hex) = acceptance_artifact.as_deref() {
        match hex32_to_bytes(hex) {
            Ok(hash) => acceptance_hash = Some(hash),
            Err(()) => facts.push(transport_fact(
                "evidence/acceptance-hash",
                false,
                "invalid acceptance hash",
            )),
        }
        let (observation, payload) = observe_store_payload(&store, ":acceptance", hex);
        if observation.observed_hash.is_some() {
            checked_artifacts = checked_artifacts.saturating_add(1);
        }
        let load_error = observation.load_error.clone();
        store_observations.push(observation);

        match payload
            .as_deref()
            .ok_or_else(|| {
                ObligationError::Store(
                    load_error.unwrap_or_else(|| "acceptance payload unavailable".to_string()),
                )
            })
            .and_then(|bytes| parse_observed_term(bytes, &store.path_for(hex)))
        {
            Ok(t) => {
                for artifact in proposed_referenced_artifacts(&t) {
                    let observation = observe_store(&store, ":acceptance-reference", &artifact);
                    if observation.observed_hash.is_some() {
                        checked_artifacts = checked_artifacts.saturating_add(1);
                    }
                    store_observations.push(observation);
                }
                acceptance = t;
            }
            Err(e) => facts.push(transport_fact(
                "evidence/acceptance-load",
                false,
                e.to_string(),
            )),
        }
    }

    // Registry policy enforcement (optional).
    let mut policy_min_signatures = None;
    let mut policy_observation = None;
    let mut signature_set = Term::Nil;
    let mut signature_observations = Vec::new();
    if let Some(policy_path) = policy {
        match RegistryPolicy::observe(policy_path) {
            Ok(pol) => {
                policy_min_signatures = Some(pol.min_signatures);
                let allowed_keys = pol
                    .allowed_public_keys
                    .iter()
                    .cloned()
                    .zip(pol.decoded_public_keys())
                    .map(|(encoded, decoded)| match decoded {
                        Ok(decoded) => PolicyKeyObservation {
                            encoded,
                            decoded: Some(decoded),
                            decode_error: None,
                            key_valid: ed25519_dalek::VerifyingKey::from_bytes(&decoded).is_ok(),
                        },
                        Err(error) => PolicyKeyObservation {
                            encoded,
                            decoded: None,
                            decode_error: Some(error),
                            key_valid: false,
                        },
                    })
                    .collect();
                policy_observation = Some(RegistryPolicyObservation {
                    version: pol.version,
                    min_signatures: pol.min_signatures,
                    allowed_keys,
                });
                if pol.min_signatures > 0 {
                    let sigset_path = signatures
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| signatures_file_path(&pkg_dir));
                    match read_term_file(&sigset_path) {
                        Ok(term) => {
                            for artifact_hash in proposed_signature_artifacts(&term) {
                                let (observation, payload) =
                                    observe_store_payload(&store, ":signature", &artifact_hash);
                                if observation.observed_hash.is_some() {
                                    checked_artifacts = checked_artifacts.saturating_add(1);
                                }
                                let load_error = observation.load_error.clone();
                                store_observations.push(observation);
                                checked_signatures = checked_signatures.saturating_add(1);
                                match payload
                                    .as_deref()
                                    .ok_or_else(|| {
                                        ObligationError::Store(load_error.unwrap_or_else(|| {
                                            "signature payload unavailable".to_string()
                                        }))
                                    })
                                    .and_then(|bytes| {
                                        parse_observed_term(bytes, &store.path_for(&artifact_hash))
                                    }) {
                                    Ok(term) => signature_observations.push(SignatureObservation {
                                        artifact_hash,
                                        crypto_valid: verify_acceptance_signature_mechanism(&term),
                                        term,
                                    }),
                                    Err(error) => facts.push(transport_fact(
                                        "policy/signature-load",
                                        false,
                                        error.to_string(),
                                    )),
                                }
                            }
                            signature_set = term;
                        }
                        Err(error) => facts.push(transport_fact(
                            "policy/signature-set-load",
                            false,
                            error.to_string(),
                        )),
                    }
                }
            }
            Err(e) => facts.push(transport_fact("policy/load", false, e.to_string())),
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
                    let observation = observe_store(&store, ":scan", &name);
                    if observation.observed_hash.is_some() {
                        checked_artifacts = checked_artifacts.saturating_add(1);
                    }
                    store_observations.push(observation);
                }
            }
            Err(e) => facts.push(transport_fact("evidence/store-scan", false, e.to_string())),
        }
    }

    let mut authority = EvidenceVerifyAuthority::load(authority_artifact)
        .map_err(|error| ObligationError::Store(error.to_string()))?;
    let decision = authority
        .package(PackageVerificationRequest {
            facts,
            acceptance_hash,
            acceptance,
            store: store_observations,
            policy: policy_observation,
            signature_set,
            signatures: signature_observations,
        })
        .map_err(|error| ObligationError::Store(error.to_string()))?;
    let valid_signatures = decision.valid_signatures;
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

fn transport_fact(code: &str, valid: bool, detail: impl Into<String>) -> EvidenceFact {
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

fn parse_observed_term(bytes: &[u8], path: &Path) -> Result<Term, ObligationError> {
    let source = std::str::from_utf8(bytes).map_err(|error| {
        ObligationError::Store(format!("bad artifact {}: {error}", path.display()))
    })?;
    parse_term(source).map_err(|error| {
        ObligationError::Store(format!("bad artifact {}: {error}", path.display()))
    })
}

fn read_term_file(path: &Path) -> Result<Term, ObligationError> {
    let source = std::fs::read_to_string(path)?;
    parse_term(&source).map_err(|error| {
        ObligationError::Store(format!("bad artifact {}: {error}", path.display()))
    })
}

fn proposed_referenced_artifacts(t: &Term) -> Vec<String> {
    let Term::Map(m) = t else {
        return Vec::new();
    };
    let Some(Term::Vector(obs)) = m.get(&gc_coreform::TermOrdKey(Term::symbol(":obligations")))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for o in obs {
        let Term::Map(om) = o else {
            continue;
        };
        let Some(Term::Str(hex)) = om.get(&gc_coreform::TermOrdKey(Term::symbol(":artifact")))
        else {
            continue;
        };
        if looks_like_hex32(hex) {
            out.push(hex.clone());
        }
    }
    out
}

fn proposed_signature_artifacts(term: &Term) -> Vec<String> {
    let Term::Vector(values) = term else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| match value {
            Term::Str(hash) if looks_like_hex32(hash) => Some(hash.clone()),
            _ => None,
        })
        .collect()
}

fn observe_store(
    store: &EvidenceStore,
    role: &'static str,
    required_hash: &str,
) -> StoreHashObservation {
    observe_store_payload(store, role, required_hash).0
}

fn observe_store_payload(
    store: &EvidenceStore,
    role: &'static str,
    required_hash: &str,
) -> (StoreHashObservation, Option<Vec<u8>>) {
    match store.observe_bytes(required_hash) {
        Ok((bytes, observed_hash)) => (
            StoreHashObservation {
                role,
                required_hash: required_hash.to_string(),
                observed_hash: Some(observed_hash),
                load_error: None,
            },
            Some(bytes),
        ),
        Err(error) => (
            StoreHashObservation {
                role,
                required_hash: required_hash.to_string(),
                observed_hash: None,
                load_error: Some(error.to_string()),
            },
            None,
        ),
    }
}
