#[derive(Debug, Clone)]
pub struct ObligationResult {
    pub name: String,
    pub ok: bool,
    pub artifact: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PackageTestResult {
    pub ok: bool,
    pub acceptance_artifact: String,
    pub obligation_results: Vec<ObligationResult>,
}

#[derive(Debug, Clone)]
pub struct PackageTypecheckResult {
    pub ok: bool,
    pub report_coreform: String,
}

#[derive(Debug, Clone)]
struct LoadedModule {
    entry: ModuleEntry,
    abs_path: PathBuf,
    forms: Vec<Term>,
    meta: Option<Term>,
    hash: [u8; 32],
}

#[derive(Debug, Clone)]
struct ModuleEval {
    path: PathBuf,
    env: Env,
    defined: BTreeMap<String, Value>,
    exports: Vec<String>,
}

#[derive(Debug, Clone)]
struct TestId {
    suite_sym: String,
    test_name: String,
}

#[derive(Debug, Clone)]
struct TestRun {
    id: TestId,
    ok: bool,
    effect_log: Option<EffectLog>,
    steps: u64,
    effect_entries: u64,
    effect_log_bytes: u64,
    value_hash: [u8; 32],
    error: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct KernelLimits {
    step_limit: StepLimit,
    mem_limits: MemLimits,
}

const OBLIGATION_TEST_WORKERS_ENV: &str = "GENESIS_TEST_WORKERS";
const OBLIGATION_CACHE_DISABLE_ENV: &str = "GENESIS_OBLIGATION_CACHE_DISABLE";

pub fn parse_canonicalize_module_source_with_frontend(
    src: &str,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<Vec<Term>, ObligationError> {
    enforce_frontend_allowed(frontend, "parse/canonicalize")?;
    let limits = KernelLimits {
        step_limit,
        mem_limits,
    };
    if frontend_is_rust(frontend) {
        let forms = parse_module(src).map_err(|pe| ObligationError::Module(format!("{pe}")))?;
        canonicalize_module(forms).map_err(|e| ObligationError::Module(e.to_string()))
    } else {
        let CoreformFrontend::Selfhost(cfg) = frontend else {
            return Err(ObligationError::Module(
                "invalid frontend dispatch in parse/canonicalize".to_string(),
            ));
        };
        // Toolchain bootstrap is trusted and therefore uncharged.
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(limits.mem_limits);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            cfg.bootstrap_mode,
            cfg.artifact.as_deref(),
        )
        .map_err(|e| ObligationError::Module(format!("selfhost/init: {e}")))?;

        // Apply user/configured limits to parse+canonicalize work.
        ctx.steps = 0;
        ctx.step_limit = limits.step_limit.resolve();
        selfhost_parse_canonicalize_module(&mut ctx, &env, src)
    }
}

pub fn hash_module_forms_with_frontend(
    forms: &[Term],
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<[u8; 32], ObligationError> {
    enforce_frontend_allowed(frontend, "module hash")?;
    let limits = KernelLimits {
        step_limit,
        mem_limits,
    };
    if frontend_is_rust(frontend) {
        Ok(hash_module(forms))
    } else {
        let CoreformFrontend::Selfhost(cfg) = frontend else {
            return Err(ObligationError::Module(
                "invalid frontend dispatch in module hash".to_string(),
            ));
        };
        // Toolchain bootstrap is trusted and therefore uncharged.
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(limits.mem_limits);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            cfg.bootstrap_mode,
            cfg.artifact.as_deref(),
        )
        .map_err(|e| ObligationError::Module(format!("selfhost/init: {e}")))?;

        // Apply user/configured limits to hash work.
        ctx.steps = 0;
        ctx.step_limit = limits.step_limit.resolve();
        selfhost_hash_module_forms(&mut ctx, &env, forms)
    }
}

fn mk_eval_ctx(limits: KernelLimits) -> EvalCtx {
    let mut ctx = EvalCtx::with_step_limit(limits.step_limit.resolve());
    ctx.set_mem_limits(limits.mem_limits);
    ctx
}

pub fn pack(pkg_toml: &Path) -> Result<String, ObligationError> {
    pack_with_frontend(pkg_toml, default_coreform_frontend())
}

pub fn pack_with_frontend(
    pkg_toml: &Path,
    frontend: CoreformFrontend,
) -> Result<String, ObligationError> {
    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let limits = KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    };
    let modules = load_modules(&pkg_dir, &manifest.modules, &frontend, limits)?;

    // Compute dependency package hashes (recursive) to lock.
    let deps = pack_dep_hashes(&pkg_dir, &manifest.dependencies, &frontend)?;

    // Update package.toml in-place with pinned module + dependency hashes.
    pin_manifest_hashes(pkg_toml, &manifest, &modules, &deps)?;

    // Create a canonical package record artifact and return its content hash.
    let record = package_record_term(&manifest, &modules, &deps);
    let store = EvidenceStore::open(&pkg_dir)?;
    store.put_term(&record)
}

/// Compute the package artifact hash without modifying `package.toml`.
///
/// This requires pinned module hashes and pinned dependency hashes to match.
pub fn package_artifact_hash(pkg_toml: &Path) -> Result<String, ObligationError> {
    let mut visited = std::collections::BTreeSet::new();
    let limits = KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    };
    compute_package_artifact_hash(
        pkg_toml,
        true,
        &mut visited,
        &default_coreform_frontend(),
        limits,
    )
}

pub fn test_package(
    pkg_toml: &Path,
    caps_override: Option<&Path>,
) -> Result<PackageTestResult, ObligationError> {
    test_package_with_step_limit(
        pkg_toml,
        caps_override,
        StepLimit::Default,
        MemLimits::default(),
    )
}

pub fn test_package_with_step_limit(
    pkg_toml: &Path,
    caps_override: Option<&Path>,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<PackageTestResult, ObligationError> {
    test_package_with_step_limit_and_frontend(
        pkg_toml,
        caps_override,
        step_limit,
        mem_limits,
        default_coreform_frontend(),
    )
}

pub fn test_package_with_step_limit_and_frontend(
    pkg_toml: &Path,
    caps_override: Option<&Path>,
    step_limit: StepLimit,
    mem_limits: MemLimits,
    frontend: CoreformFrontend,
) -> Result<PackageTestResult, ObligationError> {
    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let step_limit = effective_step_limit(&manifest, step_limit)?;
    let mem_limits = effective_mem_limits(&manifest, mem_limits);
    let limits = KernelLimits {
        step_limit,
        mem_limits,
    };
    let store = EvidenceStore::open(&pkg_dir)?;

    let mut preflight_errors: Vec<String> = Vec::new();

    // Load & hash modules (also validates pinned module hashes if present).
    let modules = match load_modules(&pkg_dir, &manifest.modules, &frontend, limits) {
        Ok(ms) => ms,
        Err(e) => {
            preflight_errors.push(format!("{e}"));
            Vec::new()
        }
    };
    if preflight_errors.is_empty() {
        for m in &modules {
            let want = m.entry.hash.as_deref().unwrap_or("");
            if want.is_empty() {
                preflight_errors.push(format!(
                    "module {} is missing pinned hash; run `genesis pack --pkg {}`",
                    m.entry.path,
                    pkg_toml.display()
                ));
                continue;
            }
            let got_hex = hex32(m.hash);
            if want != got_hex {
                preflight_errors.push(format!(
                    "module hash mismatch for {}: manifest has {}, computed {}",
                    m.entry.path, want, got_hex
                ));
            }
        }
    }

    // Validate dependency hashes too.
    if preflight_errors.is_empty()
        && let Err(e) = check_dep_hashes(&pkg_dir, &manifest.dependencies, &frontend, limits)
    {
        preflight_errors.push(format!("{e}"));
    }

    // Load capability policy for effect runs.
    let policy_path = caps_override
        .map(PathBuf::from)
        .or_else(|| manifest.caps_policy.as_ref().map(|p| pkg_dir.join(p)));
    let caps = if preflight_errors.is_empty() {
        if let Some(p) = policy_path.as_ref() {
            match CapsPolicy::load(p) {
                Ok(c) => c,
                Err(e) => {
                    preflight_errors.push(format!("{e}"));
                    CapsPolicy::empty()
                }
            }
        } else {
            CapsPolicy::empty()
        }
    } else {
        CapsPolicy::empty()
    };
    let caps_policy_hash = hash_optional_file(policy_path.as_deref())?;

    if !preflight_errors.is_empty() {
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/preflight-v0.2".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str(manifest.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(preflight_errors.iter().cloned().map(Term::Str).collect()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let artifact = store.put_term(&report)?;
        let ob = ObligationResult {
            name: "core/obligation::preflight".to_string(),
            ok: false,
            artifact: Some(artifact),
            errors: preflight_errors,
        };
        let acceptance = acceptance_term(&manifest, false, std::slice::from_ref(&ob));
        let acceptance_artifact = store.put_term(&acceptance)?;
        write_last_acceptance(&pkg_dir, &acceptance_artifact)?;
        return Ok(PackageTestResult {
            ok: false,
            acceptance_artifact,
            obligation_results: vec![ob],
        });
    }

    let cache_key = obligation_cache_key(
        pkg_toml,
        &manifest,
        &modules,
        caps_policy_hash.as_deref(),
        limits,
        &frontend,
    )?;
    if let Some(cached) = try_load_cached_test_result(&pkg_dir, &store, &cache_key)? {
        return Ok(cached);
    }

    // Evaluate once and reuse the prepared package for all test lookups/runs.
    let test_runs =
        run_tests_with_frontend(&pkg_dir, &manifest, &modules, &caps, limits, &frontend)?;

    let obligation_plan = obligation_plan_symbols(&manifest.obligations)?;
    let mut obligation_results = Vec::new();
    let mut ok_all = true;
    for ob in &obligation_plan {
        let r = match ob.as_str() {
            "core/obligation::unit-tests" => obligation_unit_tests(&store, &manifest, &test_runs),
            "core/obligation::budgets" => obligation_budgets(&store, &manifest, &test_runs),
            "core/obligation::property-tests" => {
                obligation_property_tests(&store, &pkg_dir, &manifest, &modules, limits)
            }
            "core/obligation::coverage" => {
                obligation_coverage(CoverageRunArgs {
                    store: &store,
                    pkg_dir: &pkg_dir,
                    manifest: &manifest,
                    modules: &modules,
                    tests: &test_runs,
                    limits,
                    profile: CoverageProfile::Symbol,
                    obligation_name: "core/obligation::coverage",
                })
            }
            "core/obligation::coverage-decision" => {
                obligation_coverage(CoverageRunArgs {
                    store: &store,
                    pkg_dir: &pkg_dir,
                    manifest: &manifest,
                    modules: &modules,
                    tests: &test_runs,
                    limits,
                    profile: CoverageProfile::Decision,
                    obligation_name: "core/obligation::coverage-decision",
                })
            }
            "core/obligation::coverage-mcdc" => {
                obligation_coverage(CoverageRunArgs {
                    store: &store,
                    pkg_dir: &pkg_dir,
                    manifest: &manifest,
                    modules: &modules,
                    tests: &test_runs,
                    limits,
                    profile: CoverageProfile::Mcdc,
                    obligation_name: "core/obligation::coverage-mcdc",
                })
            }
            "core/obligation::determinism" => {
                obligation_determinism(&store, &manifest, &modules, &test_runs)
            }
            "core/obligation::capabilities-declared" => {
                obligation_caps_declared(&store, &manifest, &modules, &test_runs)
            }
            "core/obligation::replayable-tests" => {
                obligation_replayable(&store, &pkg_dir, &manifest, &modules, &test_runs, limits)
            }
            "core/obligation::concurrency-replay" => obligation_concurrency_replay(
                &store, &pkg_dir, &manifest, &modules, &test_runs, limits,
            ),
            "core/obligation::typecheck" => {
                obligation_typecheck(&store, &modules, &frontend, limits)
            }
            "core/obligation::stage1-validation" => {
                obligation_stage1_validation(&store, &manifest, &modules)
            }
            "core/obligation::translation-validation" => obligation_translation_validation(
                &store, &pkg_dir, &manifest, &modules, &caps, &test_runs, limits, &frontend,
            ),
            "core/obligation::gfx-golden-images" => obligation_gfx::obligation_gfx_golden_images(
                &store, &pkg_dir, &manifest, &modules, limits,
            ),
            "core/obligation::gfx-frame-budgets" => obligation_gfx::obligation_gfx_frame_budgets(
                &store, &pkg_dir, &manifest, &modules, limits,
            ),
            "core/obligation::gfx-api-stability" => {
                obligation_gfx::obligation_gfx_api_stability(&store, &manifest, &modules, limits)
            }
            "core/obligation::lint" => obligation_lint(&store, &manifest, &modules, limits),
            "core/obligation::ai-style" => obligation_ai_style(&store, &manifest, &modules, limits),
            other => Ok(ObligationResult {
                name: other.to_string(),
                ok: false,
                artifact: None,
                errors: vec![format!("unknown obligation {other}")],
            }),
        }?;
        ok_all &= r.ok;
        obligation_results.push(r);
    }

    let acceptance = acceptance_term(&manifest, ok_all, &obligation_results);
    let acceptance_artifact = store.put_term(&acceptance)?;
    write_last_acceptance(&pkg_dir, &acceptance_artifact)?;
    let result = PackageTestResult {
        ok: ok_all,
        acceptance_artifact,
        obligation_results,
    };
    write_cached_test_result(&pkg_dir, &cache_key, &result)?;
    Ok(result)
}

pub fn typecheck_package_with_step_limit_and_frontend(
    pkg_toml: &Path,
    step_limit: StepLimit,
    mem_limits: MemLimits,
    frontend: CoreformFrontend,
) -> Result<PackageTypecheckResult, ObligationError> {
    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let limits = KernelLimits {
        step_limit,
        mem_limits,
    };
    let modules = load_modules(&pkg_dir, &manifest.modules, &frontend, limits)?;
    let report = typecheck_report_with_frontend(&modules, &frontend, limits)?;
    let report_coreform = print_term(&report.to_term());
    Ok(PackageTypecheckResult {
        ok: report.ok,
        report_coreform,
    })
}
