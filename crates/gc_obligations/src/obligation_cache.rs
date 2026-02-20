use super::*;

pub(super) fn hash_optional_file(path: Option<&Path>) -> Result<Option<String>, ObligationError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = std::fs::read(path)?;
    Ok(Some(blake3::hash(&bytes).to_hex().to_string()))
}

pub(super) fn step_limit_term(step_limit: StepLimit) -> Term {
    match step_limit {
        StepLimit::Default => Term::symbol(":default"),
        StepLimit::Unlimited => Term::symbol(":unlimited"),
        StepLimit::Limit(n) => Term::Int(BigInt::from(n)),
    }
}

pub(super) fn option_u64_term(v: Option<u64>) -> Term {
    match v {
        Some(n) => Term::Int(BigInt::from(n)),
        None => Term::Nil,
    }
}

pub(super) fn mem_limits_term(mem: MemLimits) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":max-pair-cells")),
                option_u64_term(mem.max_pair_cells),
            ),
            (
                TermOrdKey(Term::symbol(":max-vec-len")),
                option_u64_term(mem.max_vec_len),
            ),
            (
                TermOrdKey(Term::symbol(":max-map-len")),
                option_u64_term(mem.max_map_len),
            ),
            (
                TermOrdKey(Term::symbol(":max-bytes-len")),
                option_u64_term(mem.max_bytes_len),
            ),
            (
                TermOrdKey(Term::symbol(":max-string-len")),
                option_u64_term(mem.max_string_len),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn frontend_term(frontend: &CoreformFrontend) -> Term {
    match frontend {
        #[cfg(feature = "parity-harness")]
        CoreformFrontend::Rust => Term::Map(
            [(
                TermOrdKey(Term::symbol(":kind")),
                Term::symbol(":frontend/rust"),
            )]
            .into_iter()
            .collect(),
        ),
        CoreformFrontend::Selfhost(cfg) => {
            let mode = match cfg.bootstrap_mode {
                SelfhostBootstrapMode::ArtifactOnly => ":artifact-only",
                SelfhostBootstrapMode::ArtifactPreferred => ":artifact-preferred",
                SelfhostBootstrapMode::Embedded => ":embedded",
            };
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":kind")),
                        Term::symbol(":frontend/selfhost"),
                    ),
                    (TermOrdKey(Term::symbol(":mode")), Term::symbol(mode)),
                    (
                        TermOrdKey(Term::symbol(":artifact")),
                        cfg.artifact
                            .as_ref()
                            .map(|p| Term::Str(p.display().to_string()))
                            .unwrap_or(Term::Nil),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        }
    }
}

pub(super) fn obligation_cache_key(
    pkg_toml: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps_policy_hash: Option<&str>,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<String, ObligationError> {
    let pkg_toml_hash = hash_optional_file(Some(pkg_toml))?.unwrap_or_default();
    let module_hashes = Term::Vector(
        modules
            .iter()
            .map(|m| {
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(m.entry.path.clone()),
                        ),
                        (TermOrdKey(Term::symbol(":hash")), Term::Str(hex32(m.hash))),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    );
    let key_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-cache-key-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg-name")),
                Term::Str(manifest.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg-version")),
                Term::Str(manifest.version.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg-toml-h")),
                Term::Str(pkg_toml_hash),
            ),
            (TermOrdKey(Term::symbol(":module-hashes")), module_hashes),
            (
                TermOrdKey(Term::symbol(":caps-policy-h")),
                caps_policy_hash
                    .map(|s| Term::Str(s.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    manifest
                        .obligations
                        .iter()
                        .cloned()
                        .map(Term::Symbol)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(manifest.tests.iter().cloned().map(Term::Symbol).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":property-tests")),
                Term::Vector(
                    manifest
                        .property_tests
                        .iter()
                        .cloned()
                        .map(Term::Symbol)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":step-limit")),
                step_limit_term(limits.step_limit),
            ),
            (
                TermOrdKey(Term::symbol(":mem-limits")),
                mem_limits_term(limits.mem_limits),
            ),
            (
                TermOrdKey(Term::symbol(":frontend")),
                frontend_term(frontend),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(hex32(hash_term(&key_term)))
}

pub(super) fn obligation_cache_dir(pkg_dir: &Path) -> PathBuf {
    pkg_dir.join(".genesis").join("cache").join("obligations")
}

pub(super) fn obligation_cache_path(pkg_dir: &Path, key: &str) -> PathBuf {
    obligation_cache_dir(pkg_dir).join(format!("{key}.gc"))
}

pub(super) fn obligation_result_to_cache_term(r: &ObligationResult) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Symbol(r.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(r.ok)),
            (
                TermOrdKey(Term::symbol(":artifact")),
                r.artifact
                    .as_ref()
                    .map(|a| Term::Str(a.clone()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(r.errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn cache_term_to_obligation_result(t: &Term) -> Option<ObligationResult> {
    let Term::Map(m) = t else { return None };
    let name = match m.get(&TermOrdKey(Term::symbol(":name")))? {
        Term::Symbol(s) | Term::Str(s) => s.clone(),
        _ => return None,
    };
    let ok = match m.get(&TermOrdKey(Term::symbol(":ok")))? {
        Term::Bool(b) => *b,
        _ => return None,
    };
    let artifact = match m.get(&TermOrdKey(Term::symbol(":artifact"))) {
        None | Some(Term::Nil) => None,
        Some(Term::Str(s)) | Some(Term::Symbol(s)) => Some(s.clone()),
        Some(_) => return None,
    };
    let errors = match m.get(&TermOrdKey(Term::symbol(":errors"))) {
        None => Vec::new(),
        Some(Term::Vector(xs)) => xs
            .iter()
            .filter_map(|x| match x {
                Term::Str(s) | Term::Symbol(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        Some(_) => return None,
    };
    Some(ObligationResult {
        name,
        ok,
        artifact,
        errors,
    })
}

pub(super) fn cached_result_to_term(key: &str, result: &PackageTestResult) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/obligation-cache-v0.1".to_string()),
            ),
            (TermOrdKey(Term::symbol(":key")), Term::Str(key.to_string())),
            (
                TermOrdKey(Term::symbol(":acceptance")),
                Term::Str(result.acceptance_artifact.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(result.ok)),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    result
                        .obligation_results
                        .iter()
                        .map(obligation_result_to_cache_term)
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

pub(super) fn parse_cached_result_term(key: &str, t: &Term) -> Option<PackageTestResult> {
    let Term::Map(m) = t else { return None };
    if !matches!(
        m.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(Term::Str(s)) if s == "genesis/obligation-cache-v0.1"
    ) {
        return None;
    }
    if !matches!(
        m.get(&TermOrdKey(Term::symbol(":key"))),
        Some(Term::Str(s)) if s == key
    ) {
        return None;
    }
    let acceptance_artifact = match m.get(&TermOrdKey(Term::symbol(":acceptance")))? {
        Term::Str(s) | Term::Symbol(s) => s.clone(),
        _ => return None,
    };
    let ok = match m.get(&TermOrdKey(Term::symbol(":ok")))? {
        Term::Bool(b) => *b,
        _ => return None,
    };
    let obligation_results = match m.get(&TermOrdKey(Term::symbol(":obligations")))? {
        Term::Vector(xs) => xs
            .iter()
            .map(cache_term_to_obligation_result)
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    Some(PackageTestResult {
        ok,
        acceptance_artifact,
        obligation_results,
    })
}

pub(super) fn cache_artifacts_present_and_valid(
    store: &EvidenceStore,
    result: &PackageTestResult,
) -> Result<bool, ObligationError> {
    let acceptance_path = store.path_for(&result.acceptance_artifact);
    if !acceptance_path.exists() {
        return Ok(false);
    }
    store.verify_hex(&result.acceptance_artifact)?;
    for ob in &result.obligation_results {
        if let Some(artifact) = &ob.artifact {
            let path = store.path_for(artifact);
            if !path.exists() {
                return Ok(false);
            }
            store.verify_hex(artifact)?;
        }
    }
    Ok(true)
}

pub(super) fn try_load_cached_test_result(
    pkg_dir: &Path,
    store: &EvidenceStore,
    key: &str,
) -> Result<Option<PackageTestResult>, ObligationError> {
    if env_truthy(OBLIGATION_CACHE_DISABLE_ENV) {
        return Ok(None);
    }
    let path = obligation_cache_path(pkg_dir, key);
    if !path.exists() {
        return Ok(None);
    }
    let src = std::fs::read_to_string(&path)?;
    let parsed = match parse_term(&src) {
        Ok(t) => t,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        }
    };
    let Some(result) = parse_cached_result_term(key, &parsed) else {
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    };
    if !cache_artifacts_present_and_valid(store, &result)? {
        return Ok(None);
    }
    write_last_acceptance(pkg_dir, &result.acceptance_artifact)?;
    Ok(Some(result))
}

pub(super) fn write_cached_test_result(
    pkg_dir: &Path,
    key: &str,
    result: &PackageTestResult,
) -> Result<(), ObligationError> {
    if env_truthy(OBLIGATION_CACHE_DISABLE_ENV) {
        return Ok(());
    }
    let dir = obligation_cache_dir(pkg_dir);
    std::fs::create_dir_all(&dir)?;
    let path = obligation_cache_path(pkg_dir, key);
    let payload = print_term(&cached_result_to_term(key, result));

    let mut i: u64 = 0;
    let tmp = loop {
        let cand = dir.join(format!(".tmp-{}-{}-{}", key, std::process::id(), i));
        i = i.saturating_add(1);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&cand)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(payload.as_bytes())?;
                let _ = f.sync_all();
                break cand;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    std::fs::rename(&tmp, &path)?;
    #[cfg(unix)]
    {
        let d = std::fs::File::open(&dir)?;
        let _ = d.sync_all();
    }
    Ok(())
}

pub(super) fn write_last_acceptance(pkg_dir: &Path, hex: &str) -> Result<(), ObligationError> {
    let genesis_dir = pkg_dir.join(".genesis");
    std::fs::create_dir_all(&genesis_dir)?;
    let path = genesis_dir.join("last_acceptance");
    let mut i: u64 = 0;
    let tmp = loop {
        let cand = genesis_dir.join(format!(".tmp-last_acceptance-{}-{}", std::process::id(), i));
        i = i.saturating_add(1);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&cand)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(format!("{hex}\n").as_bytes())?;
                let _ = f.sync_all();
                break cand;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    std::fs::rename(&tmp, &path)?;
    #[cfg(unix)]
    {
        let d = std::fs::File::open(&genesis_dir)?;
        let _ = d.sync_all();
    }
    Ok(())
}
