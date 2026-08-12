#[derive(Debug)]
pub(super) enum PreflightAuthorityOutcome {
    Passed {
        modules: Vec<LoadedModule>,
        caps: CapsPolicy,
        caps_policy_hash: Option<String>,
    },
    Failed(ObligationResult),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreflightModuleObservation {
    path: String,
    pinned_hash: Option<String>,
    computed_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreflightObservation {
    module_load_error: Option<String>,
    modules: Vec<PreflightModuleObservation>,
    dependency_error: Option<String>,
    caps_error: Option<String>,
}

fn preflight_authority_limits() -> KernelLimits {
    KernelLimits {
        step_limit: StepLimit::Limit(5_000_000),
        mem_limits: MemLimits {
            max_alloc_units: Some(10_000_000),
            ..MemLimits::default()
        },
    }
}

fn stable_preflight_error(message: String, roots: &[&Path]) -> String {
    roots.iter().fold(message, |stable, root| {
        let displayed = root.display().to_string();
        if !root.is_absolute() || displayed.len() <= 1 {
            stable
        } else {
            stable.replace(&displayed, ".")
        }
    })
}

fn optional_string_term(value: &Option<String>) -> Term {
    value.clone().map(Term::Str).unwrap_or(Term::Nil)
}

fn preflight_inputs(observation: &PreflightObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":caps-error")),
                optional_string_term(&observation.caps_error),
            ),
            (
                TermOrdKey(Term::symbol(":dependency-error")),
                optional_string_term(&observation.dependency_error),
            ),
            (
                TermOrdKey(Term::symbol(":module-load-error")),
                optional_string_term(&observation.module_load_error),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(
                    observation
                        .modules
                        .iter()
                        .map(|module| {
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":computed-h")),
                                        Term::Bytes(module.computed_hash.to_vec().into()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":path")),
                                        Term::Str(module.path.clone()),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":pinned-h")),
                                        optional_string_term(&module.pinned_hash),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            )
                        })
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn preflight_expected(
    package: &str,
    observation: &PreflightObservation,
) -> (bool, Vec<String>, Term) {
    let mut errors = Vec::new();
    if let Some(error) = &observation.module_load_error {
        errors.push(error.clone());
    } else {
        for module in &observation.modules {
            match module.pinned_hash.as_deref() {
                None | Some("") => errors.push(format!(
                    "module {} is missing pinned hash; run `genesis pack --pkg package.toml`",
                    module.path
                )),
                Some(pinned) if pinned != hex32(module.computed_hash) => errors.push(format!(
                    "module hash mismatch for {}: manifest has {}, computed {}",
                    module.path,
                    pinned,
                    hex32(module.computed_hash)
                )),
                Some(_) => {}
            }
        }
        if errors.is_empty() {
            if let Some(error) = &observation.dependency_error {
                errors.push(error.clone());
            } else if let Some(error) = &observation.caps_error {
                errors.push(error.clone());
            }
        }
    }
    let ok = errors.is_empty();
    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/preflight-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(package.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    (ok, errors, report)
}

fn decode_preflight_result(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observation: &PreflightObservation,
    request_hash: [u8; 32],
    term: Term,
) -> Result<Option<ObligationResult>, ObligationError> {
    let map = exact_map(
        &term,
        "preflight authority result",
        &[
            ":errors",
            ":kind",
            ":name",
            ":ok",
            ":operation",
            ":report",
            ":request-h",
            ":v",
        ],
    )?;
    if string_field(map, ":kind", "preflight authority result")?
        != "genesis/obligation-authority-result-v0.2"
        || string_field(map, ":name", "preflight authority result")?
            != "core/obligation::preflight"
        || !matches!(map_field(map, ":operation"), Some(Term::Symbol(value)) if value == ":preflight")
        || string_field(map, ":request-h", "preflight authority result")? != hex32(request_hash)
        || !matches!(map_field(map, ":v"), Some(Term::Int(value)) if value == &2.into())
    {
        return Err(authority_error("preflight result identity mismatch"));
    }
    let expected = preflight_expected(&manifest.name, observation);
    let errors = string_vector(
        required_field(map, ":errors", "preflight authority result")?,
        "preflight authority result :errors",
    )?;
    if bool_field(map, ":ok", "preflight authority result")? != expected.0
        || errors != expected.1
        || required_field(map, ":report", "preflight authority result")? != &expected.2
    {
        return Err(authority_error(
            "preflight result contradicts package-loading observations",
        ));
    }
    if expected.0 {
        return Ok(None);
    }
    let artifact = store.put_term(&expected.2)?;
    Ok(Some(ObligationResult {
        name: "core/obligation::preflight".to_string(),
        ok: false,
        artifact: Some(artifact),
        errors: expected.1,
    }))
}

pub(super) fn evaluate_preflight_with_authority(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    policy_path: Option<&Path>,
    frontend: &CoreformFrontend,
    package_limits: KernelLimits,
) -> Result<PreflightAuthorityOutcome, ObligationError> {
    let module_result = load_modules(pkg_dir, &manifest.modules, frontend, package_limits);
    let dependency_error = check_dep_hashes(
        pkg_dir,
        &manifest.dependencies,
        frontend,
        package_limits,
    )
    .err()
    .map(|error| stable_preflight_error(error.to_string(), &[pkg_dir]));
    let (caps, caps_policy_hash, caps_error) = match policy_path {
        None => (CapsPolicy::empty(), None, None),
        Some(path) => match CapsPolicy::load(path) {
            Ok(caps) => match hash_optional_file(Some(path)) {
                Ok(hash) => (caps, hash, None),
                Err(error) => (
                    CapsPolicy::empty(),
                    None,
                    Some(stable_preflight_error(
                        error.to_string(),
                        &[pkg_dir, path.parent().unwrap_or(pkg_dir)],
                    )),
                ),
            },
            Err(error) => (
                CapsPolicy::empty(),
                None,
                Some(stable_preflight_error(
                    error.to_string(),
                    &[pkg_dir, path.parent().unwrap_or(pkg_dir)],
                )),
            ),
        },
    };
    let (modules, module_load_error) = match module_result {
        Ok(modules) => (modules, None),
        Err(error) => (
            Vec::new(),
            Some(stable_preflight_error(error.to_string(), &[pkg_dir])),
        ),
    };
    let observation = PreflightObservation {
        module_load_error,
        modules: modules
            .iter()
            .map(|module| PreflightModuleObservation {
                path: module.entry.path.clone(),
                pinned_hash: module.entry.hash.clone(),
                computed_hash: module.hash,
            })
            .collect(),
        dependency_error,
        caps_error,
    };
    let request = authority_request_term(
        ObligationAuthorityOperation::Preflight,
        &manifest.name,
        preflight_inputs(&observation),
    );
    let request_hash = hash_term(&request);
    let result = invoke_authority(request, frontend, preflight_authority_limits())?;
    match decode_preflight_result(store, manifest, &observation, request_hash, result)? {
        Some(failure) => Ok(PreflightAuthorityOutcome::Failed(failure)),
        None => Ok(PreflightAuthorityOutcome::Passed {
            modules,
            caps,
            caps_policy_hash,
        }),
    }
}
