mod error;
mod frontend;
mod obligation_cache;
mod obligation_exec;
mod obligation_gfx;
mod obligation_lint;
mod obligation_stage;
mod registry_policy;
mod signing;
mod store;
mod transparency;
mod verify;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, hash_term, parse_module, parse_term,
    print_term,
};
use gc_effects::{CapsPolicy, EffectLog};
use gc_kernel::{
    Apply, Env, EvalCtx, MemLimits, StepLimit, Value, compile_module, eval_compiled_module,
    value_hash,
};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

pub use crate::error::ObligationError;
pub use crate::frontend::{CoreformFrontend, SelfhostFrontendConfig, default_coreform_frontend};
use crate::frontend::{
    enforce_frontend_allowed, env_truthy,
};
use crate::obligation_cache::*;
use crate::obligation_exec::*;
use crate::obligation_lint::{obligation_ai_style, obligation_lint};
use crate::obligation_stage::{
    PackageEval, obligation_stage1_validation, obligation_translation_validation,
};
pub use crate::registry_policy::{RegistryPolicy, RegistryPolicyError};
pub use crate::signing::{
    AcceptanceSignature, KeyFile, SigningError, load_signature_set, read_acceptance_hash_from_last,
    sign_acceptance_hash, signatures_file_path, write_signature_set,
};
pub use crate::store::EvidenceStore;
pub use crate::transparency::{
    TransparencyError, TransparencyVerifyResult, append_transparency_entry, verify_transparency_log,
};
pub use crate::verify::{PackageVerifyResult, verify_package, verify_package_with_policy};
pub use gc_pkg::{DepEntry, ModuleEntry, PackageManifest};

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
    match frontend {
        CoreformFrontend::Rust => {
            let forms = parse_module(src).map_err(|pe| ObligationError::Module(format!("{pe}")))?;
            canonicalize_module(forms).map_err(|e| ObligationError::Module(e.to_string()))
        }
        CoreformFrontend::Selfhost(cfg) => {
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
    match frontend {
        CoreformFrontend::Rust => Ok(hash_module(forms)),
        CoreformFrontend::Selfhost(cfg) => {
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

    let mut obligation_results = Vec::new();
    let mut ok_all = true;
    for ob in &manifest.obligations {
        let r = match ob.as_str() {
            "core/obligation::unit-tests" => obligation_unit_tests(&store, &manifest, &test_runs),
            "core/obligation::budgets" => obligation_budgets(&store, &manifest, &test_runs),
            "core/obligation::property-tests" => {
                obligation_property_tests(&store, &pkg_dir, &manifest, &modules, limits)
            }
            "core/obligation::coverage" => {
                obligation_coverage(&store, &pkg_dir, &manifest, &modules, &test_runs, limits)
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

fn effective_step_limit(
    manifest: &PackageManifest,
    cli: StepLimit,
) -> Result<StepLimit, ObligationError> {
    let pkg = manifest
        .limits
        .step_limit
        .map(StepLimit::Limit)
        .unwrap_or(StepLimit::Default);

    if cli == StepLimit::Unlimited && !manifest.limits.allow_unlimited {
        return Err(ObligationError::Manifest(
            "package policy forbids --no-step-limit (set [limits].allow_unlimited = true to permit)"
                .to_string(),
        ));
    }

    if cli == StepLimit::Unlimited {
        return Ok(StepLimit::Unlimited);
    }

    // Both are expected finite here (Default or explicit Limit), but keep this path
    // non-panicking so malformed/internal states surface as typed errors.
    let cli_n = cli.resolve().ok_or_else(|| {
        ObligationError::Manifest("invalid CLI step limit resolution (expected finite)".to_string())
    })?;
    let pkg_n = pkg.resolve().ok_or_else(|| {
        ObligationError::Manifest(
            "invalid package step limit resolution (expected finite)".to_string(),
        )
    })?;
    Ok(StepLimit::Limit(cli_n.min(pkg_n)))
}

fn effective_mem_limits(manifest: &PackageManifest, cli: MemLimits) -> MemLimits {
    fn min_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
        match (a, b) {
            (None, None) => None,
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (Some(x), Some(y)) => Some(x.min(y)),
        }
    }

    MemLimits {
        max_pair_cells: min_opt(cli.max_pair_cells, manifest.limits.max_pair_cells),
        max_vec_len: min_opt(cli.max_vec_len, manifest.limits.max_vec_len),
        max_map_len: min_opt(cli.max_map_len, manifest.limits.max_map_len),
        max_bytes_len: min_opt(cli.max_bytes_len, manifest.limits.max_bytes_len),
        max_string_len: min_opt(cli.max_string_len, manifest.limits.max_string_len),
    }
}

fn extract_protocol_error(ctx: &EvalCtx, v: &Value) -> Option<String> {
    let tok = ctx.protocol?.error;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != tok {
        return None;
    }
    let payload_term = payload.to_term_for_log(Some(tok));
    match &payload_term {
        Term::Map(m) => {
            let code = m
                .get(&TermOrdKey(Term::symbol(":error/code")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("core/error");
            let msg = m
                .get(&TermOrdKey(Term::symbol(":error/message")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("error");
            Some(format!("{code}: {msg}"))
        }
        _ => Some(print_term(&payload_term)),
    }
}

fn selfhost_parse_canonicalize_module(
    ctx: &mut EvalCtx,
    env: &Env,
    src: &str,
) -> Result<Vec<Term>, ObligationError> {
    if let Some(canon_src_fn) = env.get("core/cli::canonicalize-module-src") {
        let out = canon_src_fn
            .apply(ctx, Value::Data(Term::Str(src.to_string())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli canonicalize-module-src failed: {e}"
            )));
        }
        let Some(Term::Vector(forms)) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli canonicalize-module-src returned non-vector: {}",
                out.debug_repr()
            )));
        };
        return Ok(forms.clone());
    }

    let parse_fn = env.get("selfhost/parse::parse-module").ok_or_else(|| {
        ObligationError::Module("missing binding selfhost/parse::parse-module".to_string())
    })?;
    let parsed = parse_fn
        .apply(ctx, Value::Data(Term::Str(src.to_string())))
        .map_err(|e| ObligationError::Module(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &parsed) {
        return Err(ObligationError::Module(format!(
            "selfhost parse-module failed: {e}"
        )));
    }
    let Some(Term::Vector(parsed_forms)) = parsed.as_data() else {
        return Err(ObligationError::Module(format!(
            "selfhost parse-module returned non-vector: {}",
            parsed.debug_repr()
        )));
    };

    let canon_fn = env
        .get("selfhost/canon::canonicalize-module")
        .ok_or_else(|| {
            ObligationError::Module(
                "missing binding selfhost/canon::canonicalize-module".to_string(),
            )
        })?;
    let canon = canon_fn
        .apply(ctx, Value::Data(Term::Vector(parsed_forms.clone())))
        .map_err(|e| ObligationError::Module(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &canon) {
        return Err(ObligationError::Module(format!(
            "selfhost canonicalize-module failed: {e}"
        )));
    }
    let Some(Term::Vector(forms)) = canon.as_data() else {
        return Err(ObligationError::Module(format!(
            "selfhost canonicalize-module returned non-vector: {}",
            canon.debug_repr()
        )));
    };
    Ok(forms.clone())
}

fn selfhost_extract_module_meta(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<Option<Term>, ObligationError> {
    if let Some(meta_fn) = env.get("core/cli::module-meta") {
        let out = meta_fn
            .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli module-meta failed: {e}"
            )));
        }
        let Some(meta_term) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli module-meta returned non-data: {}",
                out.debug_repr()
            )));
        };
        return match meta_term {
            Term::Map(m) => Ok(Some(Term::Map(m.clone()))),
            Term::Nil => Ok(None),
            _ => Err(ObligationError::Module(format!(
                "selfhost core/cli module-meta returned non-map/non-nil: {}",
                out.debug_repr()
            ))),
        };
    }
    Ok(extract_meta_static(forms))
}

fn parse_hex32_str(hex: &str, context: &str) -> Result<[u8; 32], ObligationError> {
    let s = hex.trim();
    if s.len() != 64 {
        return Err(ObligationError::Module(format!(
            "{context} returned non-64-byte hex hash"
        )));
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16).ok_or_else(|| {
            ObligationError::Module(format!("{context} returned invalid hex hash"))
        })?;
        let lo = (chunk[1] as char).to_digit(16).ok_or_else(|| {
            ObligationError::Module(format!("{context} returned invalid hex hash"))
        })?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}

fn selfhost_hash_module_forms(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<[u8; 32], ObligationError> {
    if let Some(hash_forms_fn) = env.get("core/cli::hash-module-forms") {
        let out = hash_forms_fn
            .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli hash-module-forms failed: {e}"
            )));
        }
        let Some(Term::Str(hex)) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost core/cli hash-module-forms returned non-string: {}",
                out.debug_repr()
            )));
        };
        return parse_hex32_str(hex, "selfhost core/cli hash-module-forms");
    }

    if let Some(hash_fn) = env.get("selfhost/hash::hash-module") {
        let out = hash_fn
            .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
            .map_err(|e| ObligationError::Module(e.to_string()))?;
        if let Some(e) = extract_protocol_error(ctx, &out) {
            return Err(ObligationError::Module(format!(
                "selfhost hash-module failed: {e}"
            )));
        }
        let Some(Term::Str(hex)) = out.as_data() else {
            return Err(ObligationError::Module(format!(
                "selfhost hash-module returned non-string: {}",
                out.debug_repr()
            )));
        };
        return parse_hex32_str(hex, "selfhost hash-module");
    }

    Err(ObligationError::Module(
        "missing binding core/cli::hash-module-forms or selfhost/hash::hash-module".to_string(),
    ))
}

fn selfhost_optimize_module_forms(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<Vec<Term>, ObligationError> {
    let optimize_fn = env.get("core/cli::optimize-module").ok_or_else(|| {
        ObligationError::Module("missing binding core/cli::optimize-module".to_string())
    })?;
    let out = optimize_fn
        .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
        .map_err(|e| ObligationError::Opt(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &out) {
        return Err(ObligationError::Opt(format!(
            "selfhost core/cli optimize-module failed: {e}"
        )));
    }
    let Some(Term::Vector(opt_forms)) = out.as_data() else {
        return Err(ObligationError::Opt(format!(
            "selfhost core/cli optimize-module returned non-vector: {}",
            out.debug_repr()
        )));
    };
    Ok(opt_forms.clone())
}

fn selfhost_infer_effects_forms(
    ctx: &mut EvalCtx,
    env: &Env,
    forms: &[Term],
) -> Result<gc_types::InferredEffects, ObligationError> {
    let infer_fn = env.get("core/cli::infer-effects").ok_or_else(|| {
        ObligationError::Typecheck("missing binding core/cli::infer-effects".to_string())
    })?;
    let out = infer_fn
        .apply(ctx, Value::Data(Term::Vector(forms.to_vec())))
        .map_err(|e| ObligationError::Typecheck(e.to_string()))?;
    if let Some(e) = extract_protocol_error(ctx, &out) {
        return Err(ObligationError::Typecheck(format!(
            "selfhost core/cli infer-effects failed: {e}"
        )));
    }
    let out_term = out
        .as_data()
        .cloned()
        .unwrap_or_else(|| out.to_term_for_log(ctx.protocol.map(|p| p.error)));
    let Term::Map(m) = out_term else {
        return Err(ObligationError::Typecheck(format!(
            "selfhost core/cli infer-effects returned non-map: {}",
            out.debug_repr()
        )));
    };

    let mut ops = BTreeSet::new();
    let ops_term = m
        .get(&TermOrdKey(Term::symbol(":ops")))
        .ok_or_else(|| {
            ObligationError::Typecheck(
                "selfhost core/cli infer-effects result missing :ops".to_string(),
            )
        })?
        .clone();
    let Term::Vector(xs) = ops_term else {
        return Err(ObligationError::Typecheck(
            "selfhost core/cli infer-effects :ops must be vector".to_string(),
        ));
    };
    for x in xs {
        match x {
            Term::Symbol(s) => {
                ops.insert(s);
            }
            other => {
                return Err(ObligationError::Typecheck(format!(
                    "selfhost core/cli infer-effects :ops must contain symbols, got {}",
                    print_term(&other)
                )));
            }
        }
    }

    let unknown = match m.get(&TermOrdKey(Term::symbol(":unknown"))) {
        Some(Term::Bool(b)) => *b,
        Some(other) => {
            return Err(ObligationError::Typecheck(format!(
                "selfhost core/cli infer-effects :unknown must be bool, got {}",
                print_term(other)
            )));
        }
        None => false,
    };

    Ok(gc_types::InferredEffects { ops, unknown })
}

fn pin_manifest_hashes(
    pkg_toml: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    deps: &[(String, String, String)],
) -> Result<(), ObligationError> {
    let mut doc: toml::Value = toml::from_str(&std::fs::read_to_string(pkg_toml)?)
        .map_err(|e| ObligationError::Manifest(format!("{}: {e}", pkg_toml.display())))?;
    let tbl = doc
        .as_table_mut()
        .ok_or_else(|| ObligationError::Manifest("package.toml must be a table".to_string()))?;

    // modules = [{ path, hash }, ...]
    if let Some(arr) = tbl.get_mut("modules").and_then(|v| v.as_array_mut()) {
        for (i, m) in modules.iter().enumerate() {
            if let Some(entry) = arr.get_mut(i).and_then(|v| v.as_table_mut()) {
                entry.insert("hash".to_string(), toml::Value::String(hex32(m.hash)));
            }
        }
    }

    // dependencies = [{ name, path, hash }, ...]
    if let Some(arr) = tbl.get_mut("dependencies").and_then(|v| v.as_array_mut()) {
        for dep in deps {
            let (name, _path, hash_hex) = dep;
            for item in arr.iter_mut() {
                let Some(t) = item.as_table_mut() else {
                    continue;
                };
                if t.get("name").and_then(|v| v.as_str()) == Some(name.as_str()) {
                    t.insert("hash".to_string(), toml::Value::String(hash_hex.clone()));
                }
            }
        }
    }

    let out = toml::to_string_pretty(&doc)
        .map_err(|e| ObligationError::Manifest(format!("cannot write manifest: {e}")))?;
    std::fs::write(pkg_toml, out)?;
    let _ = manifest;
    Ok(())
}

fn load_modules(
    pkg_dir: &Path,
    entries: &[ModuleEntry],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<Vec<LoadedModule>, ObligationError> {
    enforce_frontend_allowed(frontend, "module loading")?;
    let mut out = Vec::new();
    match frontend {
        CoreformFrontend::Rust => {
            for e in entries {
                let abs = pkg_dir.join(&e.path);
                let src = std::fs::read_to_string(&abs)?;
                let forms =
                    parse_module(&src).map_err(|pe| ObligationError::Module(format!("{pe}")))?;
                let forms = canonicalize_module(forms)
                    .map_err(|e| ObligationError::Module(e.to_string()))?;
                let meta = extract_meta_static(&forms);
                let h = hash_module(&forms);
                out.push(LoadedModule {
                    entry: e.clone(),
                    abs_path: abs,
                    forms,
                    meta,
                    hash: h,
                });
            }
        }
        CoreformFrontend::Selfhost(cfg) => {
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
            for e in entries {
                let abs = pkg_dir.join(&e.path);
                let src = std::fs::read_to_string(&abs)?;
                let forms = selfhost_parse_canonicalize_module(&mut ctx, &env, &src)?;
                let meta = selfhost_extract_module_meta(&mut ctx, &env, &forms)?;
                let h = selfhost_hash_module_forms(&mut ctx, &env, &forms)?;
                out.push(LoadedModule {
                    entry: e.clone(),
                    abs_path: abs,
                    forms,
                    meta,
                    hash: h,
                });
            }
        }
    }
    Ok(out)
}

fn pack_dep_hashes(
    pkg_dir: &Path,
    deps: &[DepEntry],
    frontend: &CoreformFrontend,
) -> Result<Vec<(String, String, String)>, ObligationError> {
    let mut out = Vec::new();
    for d in deps {
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let hex = pack_with_frontend(&dep_pkg, frontend.clone())?;
        out.push((d.name.clone(), d.path.clone(), hex));
    }
    Ok(out)
}

fn check_dep_hashes(
    pkg_dir: &Path,
    deps: &[DepEntry],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<(), ObligationError> {
    let mut visited = std::collections::BTreeSet::new();
    for d in deps {
        let want = d.hash.as_deref().unwrap_or("");
        if want.is_empty() {
            return Err(ObligationError::Manifest(format!(
                "dependency {} is missing pinned hash; run `genesis pack` and commit the updated package.toml",
                d.name
            )));
        }
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let got = compute_package_artifact_hash(&dep_pkg, true, &mut visited, frontend, limits)?;
        if got != want {
            return Err(ObligationError::Manifest(format!(
                "dependency hash mismatch for {}: manifest has {}, computed {}",
                d.name, want, got
            )));
        }
    }
    Ok(())
}

fn compute_package_artifact_hash(
    pkg_toml: &Path,
    require_pinned: bool,
    visited: &mut std::collections::BTreeSet<PathBuf>,
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<String, ObligationError> {
    let canon = std::fs::canonicalize(pkg_toml)?;
    if !visited.insert(canon.clone()) {
        return Err(ObligationError::Manifest(format!(
            "dependency cycle detected at {}",
            canon.display()
        )));
    }

    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let modules = load_modules(&pkg_dir, &manifest.modules, frontend, limits)?;
    if require_pinned {
        for m in &modules {
            let want = m.entry.hash.as_deref().unwrap_or("");
            if want.is_empty() {
                return Err(ObligationError::Manifest(format!(
                    "{}: module {} missing pinned hash",
                    pkg_toml.display(),
                    m.entry.path
                )));
            }
            let got = hex32(m.hash);
            if want != got {
                return Err(ObligationError::Manifest(format!(
                    "{}: module hash mismatch for {}: manifest has {}, computed {}",
                    pkg_toml.display(),
                    m.entry.path,
                    want,
                    got
                )));
            }
        }
    }

    let mut deps_out = Vec::new();
    for d in &manifest.dependencies {
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let dep_hash =
            compute_package_artifact_hash(&dep_pkg, require_pinned, visited, frontend, limits)?;
        if require_pinned {
            let want = d.hash.as_deref().unwrap_or("");
            if want.is_empty() || want != dep_hash {
                return Err(ObligationError::Manifest(format!(
                    "{}: dependency hash mismatch for {}",
                    pkg_toml.display(),
                    d.name
                )));
            }
        }
        deps_out.push((d.name.clone(), d.path.clone(), dep_hash));
    }

    let record = package_record_term(&manifest, &modules, &deps_out);
    let bytes = gc_coreform::print_term(&record).into_bytes();
    let hex = blake3::hash(&bytes).to_hex().to_string();
    visited.remove(&canon);
    Ok(hex)
}

fn package_record_term(
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    deps: &[(String, String, String)],
) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/package-v0.2".to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":name")),
        Term::Str(manifest.name.clone()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":version")),
        Term::Str(manifest.version.clone()),
    );

    let mods: Vec<Term> = modules
        .iter()
        .map(|x| {
            let mut mm = BTreeMap::new();
            mm.insert(
                TermOrdKey(Term::symbol(":path")),
                Term::Str(x.entry.path.clone()),
            );
            mm.insert(
                TermOrdKey(Term::symbol(":hash")),
                Term::Bytes(x.hash.to_vec().into()),
            );
            Term::Map(mm)
        })
        .collect();
    m.insert(TermOrdKey(Term::symbol(":modules")), Term::Vector(mods));

    let deps_t: Vec<Term> = deps
        .iter()
        .map(|(name, path, hash_hex)| {
            let mut dm = BTreeMap::new();
            dm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
            dm.insert(TermOrdKey(Term::symbol(":path")), Term::Str(path.clone()));
            dm.insert(
                TermOrdKey(Term::symbol(":hash")),
                Term::Str(hash_hex.clone()),
            );
            Term::Map(dm)
        })
        .collect();
    m.insert(
        TermOrdKey(Term::symbol(":dependencies")),
        Term::Vector(deps_t),
    );

    m.insert(
        TermOrdKey(Term::symbol(":obligations")),
        Term::Vector(
            manifest
                .obligations
                .iter()
                .cloned()
                .map(Term::Symbol)
                .collect(),
        ),
    );
    m.insert(
        TermOrdKey(Term::symbol(":tests")),
        Term::Vector(manifest.tests.iter().cloned().map(Term::Symbol).collect()),
    );

    Term::Map(m)
}

fn acceptance_term(manifest: &PackageManifest, ok: bool, obs: &[ObligationResult]) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/acceptance-v0.2".to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    m.insert(
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
            ]
            .into_iter()
            .collect(),
        ),
    );
    let entries: Vec<Term> = obs
        .iter()
        .map(|r| {
            let mut rm = BTreeMap::new();
            rm.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Symbol(r.name.clone()),
            );
            rm.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(r.ok));
            if let Some(a) = &r.artifact {
                rm.insert(TermOrdKey(Term::symbol(":artifact")), Term::Str(a.clone()));
            }
            if !r.errors.is_empty() {
                rm.insert(
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(r.errors.iter().cloned().map(Term::Str).collect()),
                );
            }
            Term::Map(rm)
        })
        .collect();
    m.insert(
        TermOrdKey(Term::symbol(":obligations")),
        Term::Vector(entries),
    );
    Term::Map(m)
}

fn collect_test_ids(eval: &PackageEval, suites: &[String]) -> Result<Vec<TestId>, ObligationError> {
    let mut ids = Vec::new();
    for suite in suites {
        let v = eval
            .lookup_any(suite)
            .ok_or_else(|| ObligationError::Test(format!("missing test suite symbol {suite}")))?;
        let suite_map = value_as_map(&v)
            .ok_or_else(|| ObligationError::Test(format!("test suite {suite} must be a map")))?;
        for (k, _vv) in suite_map.iter() {
            let name = match &k.0 {
                Term::Str(s) => s.clone(),
                Term::Symbol(s) => s.clone(),
                other => {
                    return Err(ObligationError::Test(format!(
                        "test key must be string/symbol, got {}",
                        print_term(other)
                    )));
                }
            };
            ids.push(TestId {
                suite_sym: suite.clone(),
                test_name: name,
            });
        }
    }
    Ok(ids)
}

fn configured_test_workers(max_tests: usize) -> usize {
    let default_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8);
    let parsed = std::env::var(OBLIGATION_TEST_WORKERS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default_workers);
    parsed.clamp(1, 64).min(max_tests.max(1))
}

fn run_test_batch_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
    batch: Vec<(usize, TestId)>,
) -> Result<Vec<(usize, TestRun)>, ObligationError> {
    if batch.is_empty() {
        return Ok(Vec::new());
    }
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;
    let evals = eval_modules(&mut ctx, &base, modules)?;
    let pkg = PackageEval::from_modules(base, evals)?;
    let baseline_state = ctx.state;

    let mut out = Vec::with_capacity(batch.len());
    for (idx, id) in batch {
        ctx.state = baseline_state;
        ctx.step_limit = limits.step_limit.resolve();
        ctx.reset_counters();
        let run = run_test_from_package(&mut ctx, &pkg, caps, id)?;
        out.push((idx, run));
    }
    Ok(out)
}

fn run_tests_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<Vec<TestRun>, ObligationError> {
    if manifest.tests.is_empty() {
        return Ok(Vec::new());
    }

    // First pass builds a deterministic test-id list using one package evaluation.
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;
    let evals = eval_modules(&mut ctx, &base, modules)?;
    let pkg = PackageEval::from_modules(base, evals)?;
    let test_ids = collect_test_ids(&pkg, &manifest.tests)?;
    if test_ids.is_empty() {
        return Ok(Vec::new());
    }
    let workers = configured_test_workers(test_ids.len());

    // Single-worker path reuses the prepared package snapshot and is the lowest-overhead option.
    if workers == 1 {
        let baseline_state = ctx.state;
        let mut out = Vec::with_capacity(test_ids.len());
        for id in test_ids {
            ctx.state = baseline_state;
            ctx.step_limit = limits.step_limit.resolve();
            ctx.reset_counters();
            out.push(run_test_from_package(&mut ctx, &pkg, caps, id)?);
        }
        return Ok(out);
    }

    // Multi-worker path: deterministic partitioning by original index, isolated eval contexts per worker.
    let mut buckets: Vec<Vec<(usize, TestId)>> = vec![Vec::new(); workers];
    for (i, id) in test_ids.iter().cloned().enumerate() {
        buckets[i % workers].push((i, id));
    }

    let pkg_dir = pkg_dir.to_path_buf();
    let manifest = manifest.clone();
    let modules = modules.to_vec();
    let caps = caps.clone();
    let frontend = frontend.clone();
    let mut worker_results: Vec<Vec<(usize, TestRun)>> = Vec::new();
    std::thread::scope(|scope| -> Result<(), ObligationError> {
        let mut handles = Vec::new();
        for batch in buckets {
            if batch.is_empty() {
                continue;
            }
            let pkg_dir = pkg_dir.clone();
            let manifest = manifest.clone();
            let modules = modules.clone();
            let caps = caps.clone();
            let frontend = frontend.clone();
            handles.push(scope.spawn(move || {
                run_test_batch_with_frontend(
                    &pkg_dir, &manifest, &modules, &caps, limits, &frontend, batch,
                )
            }));
        }

        for h in handles {
            match h.join() {
                Ok(Ok(rows)) => worker_results.push(rows),
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(ObligationError::Test(
                        "parallel test worker panicked".to_string(),
                    ));
                }
            }
        }
        Ok(())
    })?;

    let mut ordered: Vec<Option<TestRun>> = (0..test_ids.len()).map(|_| None).collect();
    for rows in worker_results {
        for (idx, run) in rows {
            if idx >= ordered.len() || ordered[idx].is_some() {
                return Err(ObligationError::Test(
                    "parallel test collation mismatch".to_string(),
                ));
            }
            ordered[idx] = Some(run);
        }
    }
    let mut out = Vec::with_capacity(test_ids.len());
    for row in ordered {
        let Some(run) = row else {
            return Err(ObligationError::Test(
                "parallel test collation dropped a test".to_string(),
            ));
        };
        out.push(run);
    }
    Ok(out)
}

fn run_test_from_package(
    ctx: &mut EvalCtx,
    pkg: &PackageEval,
    caps: &CapsPolicy,
    id: TestId,
) -> Result<TestRun, ObligationError> {
    let suite_v = pkg.lookup_any(&id.suite_sym).ok_or_else(|| {
        ObligationError::Test(format!("missing test suite symbol {}", id.suite_sym))
    })?;
    let suite_map = value_as_map(&suite_v).ok_or_else(|| {
        ObligationError::Test(format!("test suite {} must be a map", id.suite_sym))
    })?;
    let (test_body, expect) = parse_test_entry(
        suite_map
            .get(&TermOrdKey(Term::Str(id.test_name.clone())))
            .or_else(|| suite_map.get(&TermOrdKey(Term::Symbol(id.test_name.clone()))))
            .ok_or_else(|| {
                ObligationError::Test(format!(
                    "missing test {} in suite {}",
                    id.test_name, id.suite_sym
                ))
            })?,
    )?;

    let value = test_body
        .apply(ctx, Value::Data(Term::Nil))
        .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;

    let (final_value, effect_log) = match value {
        Value::EffectProgram(_) => {
            let prog_h = value_hash(&value);
            let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
            let r = gc_effects::run(ctx, caps, value, prog_h, toolchain)
                .map_err(|e| ObligationError::Test(format!("effect run failed: {e}")))?;
            (r.value, Some(r.log))
        }
        other => (other, None),
    };
    let steps = ctx.steps;
    let effect_entries = effect_log
        .as_ref()
        .map(|l| l.entries.len() as u64)
        .unwrap_or(0);
    let effect_log_bytes = effect_log
        .as_ref()
        .map(|l| l.to_string_canonical().len() as u64)
        .unwrap_or(0);

    let is_error = ctx
        .protocol
        .is_some_and(|p| matches!(final_value, Value::Sealed { token, .. } if token == p.error));

    let fv_hash = value_hash(&final_value);
    let ok = if is_error {
        false
    } else if let Some(exp) = expect {
        fv_hash == value_hash(&Value::Data(exp))
    } else {
        true
    };

    Ok(TestRun {
        id,
        ok,
        effect_log,
        steps,
        effect_entries,
        effect_log_bytes,
        value_hash: fv_hash,
        error: if ok {
            None
        } else {
            Some("test failed".to_string())
        },
    })
}

fn run_one_test(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    id: TestId,
    limits: KernelLimits,
) -> Result<TestRun, ObligationError> {
    run_one_test_with_frontend(
        pkg_dir,
        manifest,
        modules,
        caps,
        id,
        limits,
        &default_coreform_frontend(),
    )
}

fn run_one_test_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    id: TestId,
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<TestRun, ObligationError> {
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;

    // Evaluate dependencies (export-only) into base env.
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;

    // Evaluate modules and collect module envs for internal lookup.
    let evals = eval_modules(&mut ctx, &base, modules)?;
    let pkg = PackageEval::from_modules(base, evals)?;
    ctx.reset_counters();
    run_test_from_package(&mut ctx, &pkg, caps, id)
}

fn eval_package_once(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<PackageEval, ObligationError> {
    eval_package_once_with_frontend(
        pkg_dir,
        manifest,
        modules,
        limits,
        &default_coreform_frontend(),
    )
}

fn eval_package_once_with_frontend(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<PackageEval, ObligationError> {
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;
    base = eval_dependencies_with_frontend(
        &mut ctx,
        pkg_dir,
        &base,
        &manifest.dependencies,
        limits,
        frontend,
    )?;
    let evals = eval_modules(&mut ctx, &base, modules)?;
    PackageEval::from_modules(base, evals)
}

fn eval_dependencies(
    ctx: &mut EvalCtx,
    pkg_dir: &Path,
    base: &Env,
    deps: &[DepEntry],
) -> Result<Env, ObligationError> {
    let limits = KernelLimits {
        step_limit: StepLimit::Default,
        mem_limits: MemLimits::default(),
    };
    eval_dependencies_with_frontend(
        ctx,
        pkg_dir,
        base,
        deps,
        limits,
        &default_coreform_frontend(),
    )
}

fn eval_dependencies_with_frontend(
    ctx: &mut EvalCtx,
    pkg_dir: &Path,
    base: &Env,
    deps: &[DepEntry],
    limits: KernelLimits,
    frontend: &CoreformFrontend,
) -> Result<Env, ObligationError> {
    let mut cur = base.clone();
    for d in deps {
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let (dep_manifest, dep_dir) = PackageManifest::load(&dep_pkg)
            .map_err(|e| ObligationError::Manifest(e.to_string()))?;
        let dep_modules = load_modules(&dep_dir, &dep_manifest.modules, frontend, limits)?;

        // Evaluate dependency modules and merge their exports into env.
        let evals = eval_modules(ctx, &cur, &dep_modules)?;
        let dep_eval = PackageEval::from_modules(cur.clone(), evals)?;
        cur = dep_eval.exports_env;
    }
    Ok(cur)
}

fn eval_modules(
    ctx: &mut EvalCtx,
    base: &Env,
    modules: &[LoadedModule],
) -> Result<Vec<ModuleEval>, ObligationError> {
    let mut out = Vec::new();
    let mut cur_base = base.clone();
    for m in modules {
        let eval = eval_one_module(ctx, &cur_base, &m.forms, &m.abs_path)?;
        // Export-only merge for next modules.
        let mut exports = BTreeMap::new();
        for e in &eval.exports {
            if let Some(v) = eval.defined.get(e) {
                exports.insert(e.clone(), v.clone());
            }
        }
        cur_base = Env::with_bindings(&cur_base, exports);
        out.push(eval);
    }
    Ok(out)
}

fn eval_one_module(
    ctx: &mut EvalCtx,
    base: &Env,
    forms: &[Term],
    path: &Path,
) -> Result<ModuleEval, ObligationError> {
    let mut env = base.clone();
    let def_names: Vec<String> = forms
        .iter()
        .filter_map(|form| parse_def(form).map(|(name, _)| name))
        .collect();
    eval_module_default(&mut env, ctx, forms).map_err(|e| {
        ObligationError::Module(format!("{}: module eval failed: {e}", path.display()))
    })?;

    let mut defined: BTreeMap<String, Value> = BTreeMap::new();
    for name in def_names {
        if let Some(value) = env.get(&name) {
            defined.insert(name, value);
        }
    }

    let meta = match defined.get("::meta") {
        None => None,
        Some(Value::Data(Term::Map(m))) => Some(Term::Map(m.clone())),
        Some(other) => {
            return Err(ObligationError::Module(format!(
                "{}: ::meta must be a quoted map datum, got {}",
                path.display(),
                other.debug_repr()
            )));
        }
    };
    let exports = meta.as_ref().and_then(meta_exports).unwrap_or_default();
    Ok(ModuleEval {
        path: path.to_path_buf(),
        env,
        defined,
        exports,
    })
}

fn eval_module_default(
    env: &mut Env,
    ctx: &mut EvalCtx,
    forms: &[Term],
) -> Result<Value, gc_kernel::KernelError> {
    let compiled = compile_module(forms)?;
    eval_compiled_module(ctx, env, &compiled)
}

fn parse_def(t: &Term) -> Option<(String, Term)> {
    let items = t.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

fn extract_meta_static(forms: &[Term]) -> Option<Term> {
    // Look for (def ::meta (quote <map>)) or (def ::meta '<map>)
    for f in forms {
        let Some((name, expr)) = parse_def(f) else {
            continue;
        };
        if name != "::meta" {
            continue;
        }
        let Some(items) = expr.as_proper_list() else {
            continue;
        };
        if items.len() == 2
            && matches!(items[0], Term::Symbol(s) if s == "quote")
            && let Term::Map(m) = items[1]
        {
            return Some(Term::Map(m.clone()));
        }
    }
    None
}

fn meta_exports(meta: &Term) -> Option<Vec<String>> {
    let Term::Map(m) = meta else { return None };
    let v = m.get(&TermOrdKey(Term::Symbol(":exports".to_string())))?;
    let Term::Vector(xs) = v else { return None };
    let mut out = Vec::new();
    for x in xs {
        if let Term::Symbol(s) = x {
            out.push(s.clone());
        }
    }
    Some(out)
}

fn meta_caps(meta: &Term) -> Option<Vec<String>> {
    let Term::Map(m) = meta else { return None };
    let v = m.get(&TermOrdKey(Term::Symbol(":caps".to_string())))?;
    let Term::Vector(xs) = v else { return None };
    let mut out = Vec::new();
    for x in xs {
        if let Term::Symbol(s) = x {
            out.push(s.clone());
        }
    }
    Some(out)
}

fn suite_to_module(modules: &[LoadedModule]) -> BTreeMap<String, usize> {
    // Best-effort: scan each module's defs for a name that ends with ::tests OR matches the suite
    // symbol string; for now we map by exact def name match.
    let mut out = BTreeMap::new();
    for (i, m) in modules.iter().enumerate() {
        for f in &m.forms {
            if let Some((name, _)) = parse_def(f) {
                out.entry(name).or_insert(i);
            }
        }
    }
    out
}

fn value_as_map(v: &Value) -> Option<&BTreeMap<TermOrdKey, Value>> {
    match v {
        Value::Map(m) => Some(m),
        _ => None,
    }
}

fn apply_curried_term_args(
    ctx: &mut EvalCtx,
    mut f: Value,
    args: &[Term],
) -> Result<Value, ObligationError> {
    for arg in args {
        f = f
            .apply(ctx, Value::Data(arg.clone()))
            .map_err(|e| ObligationError::Test(format!("gc helper apply failed: {e}")))?;
    }
    Ok(f)
}

fn term_map_get_bool(t: &Term, key: &str) -> Option<bool> {
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn term_map_get_string_vec(t: &Term, key: &str) -> Vec<String> {
    let Term::Map(m) = t else { return Vec::new() };
    let Some(Term::Vector(xs)) = m.get(&TermOrdKey(Term::symbol(key))) else {
        return Vec::new();
    };
    xs.iter()
        .filter_map(|x| match x {
            Term::Str(s) | Term::Symbol(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn hex32(h: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for b in h {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_coreform::{TermOrdKey, canonicalize_module, parse_module};
    use gc_kernel::eval_module;

    fn eval_gc_term(src: &str) -> Term {
        let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canon");
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
        v.to_term_for_log(ctx.protocol.map(|p| p.error))
    }

    fn map_get<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
        let Term::Map(m) = t else { return None };
        m.get(&TermOrdKey(Term::symbol(key)))
    }

    #[test]
    fn store_is_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let store = EvidenceStore::open(dir.path()).unwrap();
        let t = Term::Str("hello".to_string());
        let h1 = store.put_term(&t).unwrap();
        let h2 = store.put_term(&t).unwrap();
        assert_eq!(h1, h2);
        assert!(store.path_for(&h1).exists());
    }

    #[test]
    fn obligation_cache_term_roundtrip_preserves_result_shape() {
        let key = "abc123";
        let result = PackageTestResult {
            ok: false,
            acceptance_artifact: "acc-hash".to_string(),
            obligation_results: vec![
                ObligationResult {
                    name: "core/obligation::unit-tests".to_string(),
                    ok: true,
                    artifact: Some("art-1".to_string()),
                    errors: Vec::new(),
                },
                ObligationResult {
                    name: "core/obligation::caps".to_string(),
                    ok: false,
                    artifact: None,
                    errors: vec!["missing cap".to_string()],
                },
            ],
        };
        let t = cached_result_to_term(key, &result);
        let parsed = parse_cached_result_term(key, &t).expect("parse cached result");
        assert_eq!(parsed.ok, result.ok);
        assert_eq!(parsed.acceptance_artifact, result.acceptance_artifact);
        assert_eq!(parsed.obligation_results.len(), 2);
        assert_eq!(
            parsed.obligation_results[0].name,
            "core/obligation::unit-tests"
        );
        assert_eq!(
            parsed.obligation_results[0].artifact.as_deref(),
            Some("art-1")
        );
        assert_eq!(parsed.obligation_results[1].name, "core/obligation::caps");
        assert_eq!(
            parsed.obligation_results[1].errors,
            vec!["missing cap".to_string()]
        );
    }

    #[test]
    fn cache_artifact_presence_check_respects_missing_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let store = EvidenceStore::open(dir.path()).unwrap();
        let acceptance = store
            .put_term(&Term::Str("acceptance".to_string()))
            .unwrap();
        let ob_artifact = store.put_term(&Term::Str("ob-art".to_string())).unwrap();
        let ok_result = PackageTestResult {
            ok: true,
            acceptance_artifact: acceptance,
            obligation_results: vec![ObligationResult {
                name: "core/obligation::unit-tests".to_string(),
                ok: true,
                artifact: Some(ob_artifact),
                errors: Vec::new(),
            }],
        };
        assert!(cache_artifacts_present_and_valid(&store, &ok_result).unwrap());

        let miss_result = PackageTestResult {
            ok: true,
            acceptance_artifact: "missing".to_string(),
            obligation_results: Vec::new(),
        };
        assert!(!cache_artifacts_present_and_valid(&store, &miss_result).unwrap());
    }

    #[test]
    fn gfx_obligation_report_builders_match_rust_report_shapes() {
        let surface_h = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let src = format!(
            r#"
            {{
              :golden
                ((((core/gfx/obligation::golden-report "pkg/demo") true)
                  [{{:suite pkg/tests::gfx :name "case-a" :ok true :kind :frame-graph}}])
                  ["warn"])
              :frame
                (((((core/gfx/obligation::frame-budget-report "pkg/demo") false)
                   {{:max-render-passes-per-frame 2}})
                   [{{:suite pkg/tests::gfx :name "case-b" :ok false :render-passes 3}}])
                   ["budget failed"])
              :api
                ((((((core/gfx/obligation::api-stability-report "pkg/demo") true)
                   "{surface_h}") "{surface_h}")
                   {{:kind "genesis/gfx-api-surface-v0.2" :exports [core/gfx/runtime::plan-frame-2d] :defs []}})
                   [])
            }}
            "#
        );
        let term = eval_gc_term(&src);
        let Some(golden) = map_get(&term, ":golden") else {
            panic!("golden report missing");
        };
        let Some(frame) = map_get(&term, ":frame") else {
            panic!("frame report missing");
        };
        let Some(api) = map_get(&term, ":api") else {
            panic!("api report missing");
        };

        let expected_golden_case = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":suite")),
                    Term::symbol("pkg/tests::gfx"),
                ),
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str("case-a".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::symbol(":frame-graph"),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let expected_golden = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/gfx-golden-images-v0.2".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str("pkg/demo".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":cases")),
                    Term::Vector(vec![expected_golden_case]),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(vec![Term::Str("warn".to_string())]),
                ),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(golden, &expected_golden);

        let expected_frame_limits = Term::Map(
            [(
                TermOrdKey(Term::symbol(":max-render-passes-per-frame")),
                Term::Int(2.into()),
            )]
            .into_iter()
            .collect(),
        );
        let expected_frame_case = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":suite")),
                    Term::symbol("pkg/tests::gfx"),
                ),
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str("case-b".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
                (
                    TermOrdKey(Term::symbol(":render-passes")),
                    Term::Int(3.into()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let expected_frame = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/gfx-frame-budgets-v0.2".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str("pkg/demo".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(false)),
                (TermOrdKey(Term::symbol(":limits")), expected_frame_limits),
                (
                    TermOrdKey(Term::symbol(":cases")),
                    Term::Vector(vec![expected_frame_case]),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(vec![Term::Str("budget failed".to_string())]),
                ),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(frame, &expected_frame);

        let expected_surface = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/gfx-api-surface-v0.2".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":exports")),
                    Term::Vector(vec![Term::symbol("core/gfx/runtime::plan-frame-2d")]),
                ),
                (
                    TermOrdKey(Term::symbol(":defs")),
                    Term::Vector(Vec::<Term>::new()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let expected_api = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/gfx-api-stability-v0.2".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str("pkg/demo".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":surface-h")),
                    Term::Str(surface_h.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":expected-surface-h")),
                    Term::Str(surface_h.to_string()),
                ),
                (TermOrdKey(Term::symbol(":surface")), expected_surface),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(Vec::<Term>::new()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(api, &expected_api);
    }

    #[test]
    fn gfx_obligation_report_builders_are_hash_stable() {
        let surface_h = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let src = format!(
            r#"
            {{
              :golden-a
                (core/coreform::hash-term
                  ((((core/gfx/obligation::golden-report "pkg/demo") true) []) []))
              :golden-b
                (core/coreform::hash-term
                  ((((core/gfx/obligation::golden-report "pkg/demo") true) []) []))
              :frame-a
                (core/coreform::hash-term
                  (((((core/gfx/obligation::frame-budget-report "pkg/demo") true) {{}}) []) []))
              :frame-b
                (core/coreform::hash-term
                  (((((core/gfx/obligation::frame-budget-report "pkg/demo") true) {{}}) []) []))
              :api-a
                (core/coreform::hash-term
                  ((((((core/gfx/obligation::api-stability-report "pkg/demo") true)
                     "{surface_h}") "{surface_h}") {{:kind "genesis/gfx-api-surface-v0.2" :exports [] :defs []}}) []))
              :api-b
                (core/coreform::hash-term
                  ((((((core/gfx/obligation::api-stability-report "pkg/demo") true)
                     "{surface_h}") "{surface_h}") {{:kind "genesis/gfx-api-surface-v0.2" :exports [] :defs []}}) []))
            }}
            "#
        );
        let term = eval_gc_term(&src);
        assert_eq!(map_get(&term, ":golden-a"), map_get(&term, ":golden-b"));
        assert_eq!(map_get(&term, ":frame-a"), map_get(&term, ":frame-b"));
        assert_eq!(map_get(&term, ":api-a"), map_get(&term, ":api-b"));
    }

    #[test]
    fn lint_autofix_builds_replace_node_patch_for_missing_types() {
        let src = r#"
          (def ::meta (quote {:exports [pkg/a::x pkg/a::y]}))
          (def pkg/a::x 1)
          (def pkg/a::y 2)
        "#;
        let forms = parse_module(src).unwrap();
        let (patch, reasons) =
            obligation_lint::lint_autofix_patch_for_module("lint.gc", &forms).unwrap();
        assert!(reasons.iter().any(|r| r == "editor/lint/missing-types-map"));
        assert!(reasons.iter().any(|r| r == "editor/lint/missing-type"));

        let Term::Map(m) = patch else {
            panic!("patch must be map")
        };
        let ops = m
            .get(&TermOrdKey(Term::symbol(":ops")))
            .expect("patch must contain :ops");
        let Term::Vector(ops) = ops else {
            panic!(":ops must be vector")
        };
        assert_eq!(ops.len(), 1);
        let Term::Map(opm) = &ops[0] else {
            panic!("op must be map")
        };
        assert!(matches!(
            opm.get(&TermOrdKey(Term::symbol(":op"))),
            Some(Term::Symbol(s)) if s == ":replace-node"
        ));
    }

    #[test]
    fn lint_autofix_returns_none_when_types_are_complete() {
        let src = r#"
          (def ::meta (quote {:exports [pkg/a::x] :types {pkg/a::x Int}}))
          (def pkg/a::x 1)
        "#;
        let forms = parse_module(src).unwrap();
        assert!(obligation_lint::lint_autofix_patch_for_module("lint.gc", &forms).is_none());
    }

    #[test]
    fn env_truthy_accepts_expected_values() {
        let is_truthy = |v: &str| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        };
        for v in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(is_truthy(v), "expected truthy: {v}");
        }
        for v in ["0", "false", "no", "", "off", "wat"] {
            assert!(!is_truthy(v), "expected falsey: {v}");
        }
    }

    #[test]
    fn eval_module_default_executes_with_compiled_fast_path() {
        let forms = parse_module("(def pkg/a::x 41)\n(prim int/add pkg/a::x 1)\n").expect("parse");
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let value = eval_module_default(&mut env, &mut ctx, &forms).expect("eval");
        let Value::Data(Term::Int(n)) = value else {
            panic!("expected int result");
        };
        assert_eq!(n, BigInt::from(42));
    }

    #[test]
    fn selfhost_only_rejects_rust_frontend_at_library_boundary() {
        let err = crate::frontend::enforce_frontend_allowed_with_flag(
            &CoreformFrontend::Rust,
            "test",
            true,
            true,
        )
        .expect_err("rust frontend must be blocked in selfhost-only mode");
        assert!(format!("{err}").contains("selfhost-only mode forbids Rust frontend"));
        crate::frontend::enforce_frontend_allowed_with_flag(
            &default_coreform_frontend(),
            "test",
            true,
            true,
        )
        .expect("selfhost frontend must be allowed");
    }

    #[test]
    fn rust_frontend_requires_compat_flag_at_library_boundary() {
        let err = crate::frontend::enforce_frontend_allowed_with_flag(
            &CoreformFrontend::Rust,
            "test",
            false,
            false,
        )
        .expect_err("rust frontend must require explicit compatibility mode");
        assert!(format!("{err}").contains("Rust frontend is disabled in this profile"));
        crate::frontend::enforce_frontend_allowed_with_flag(
            &CoreformFrontend::Rust,
            "test",
            false,
            true,
        )
        .expect("rust frontend should be permitted when compatibility mode is enabled");
    }

    #[test]
    fn non_artifact_bootstrap_mode_is_dev_only_at_library_boundary() {
        let frontend = CoreformFrontend::Selfhost(SelfhostFrontendConfig {
            bootstrap_mode: SelfhostBootstrapMode::Embedded,
            artifact: None,
        });
        let err = crate::frontend::enforce_frontend_bootstrap_mode_with_flag(
            &frontend, "test", false,
        )
        .expect_err("embedded bootstrap should be blocked outside development mode");
        assert!(format!("{err}").contains("development-only"));
        crate::frontend::enforce_frontend_bootstrap_mode_with_flag(&frontend, "test", true)
            .expect("embedded bootstrap should be allowed in development mode");
    }

    #[test]
    fn selfhost_parse_prefers_core_cli_canonicalize_handler_when_present() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        // Shadow the core/cli binding with an invalid value. If the function prefers
        // core/cli when present, this must fail before falling back to low-level bindings.
        env.set_local(
            "core/cli::canonicalize-module-src",
            Value::Data(Term::Str("shadowed".to_string())),
        );

        let err =
            selfhost_parse_canonicalize_module(&mut ctx, &env, "(def x 1)\n x\n").unwrap_err();
        assert!(
            format!("{err}").contains("not callable"),
            "expected core/cli path to be attempted first, got: {err}"
        );
    }

    #[test]
    fn selfhost_meta_prefers_core_cli_module_meta_handler_when_present() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        // Shadow the core/cli meta extractor with invalid data. If module-meta prefers
        // core/cli when present, this must fail before any static fallback path.
        env.set_local(
            "core/cli::module-meta",
            Value::Data(Term::Str("shadowed".to_string())),
        );

        let forms = canonicalize_module(parse_module("(def ::meta (quote {:caps []}))\n").unwrap())
            .expect("canonical module");
        let err = selfhost_extract_module_meta(&mut ctx, &env, &forms).unwrap_err();
        assert!(
            format!("{err}").contains("not callable"),
            "expected core/cli module-meta path to be attempted first, got: {err}"
        );
    }

    #[test]
    fn selfhost_hash_prefers_core_cli_hash_module_forms_handler_when_present() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        env.set_local(
            "core/cli::hash-module-forms",
            Value::Data(Term::Str("shadowed".to_string())),
        );

        let forms = canonicalize_module(parse_module("(def x 1)\n x\n").unwrap()).unwrap();
        let err = selfhost_hash_module_forms(&mut ctx, &env, &forms).unwrap_err();
        assert!(
            format!("{err}").contains("not callable"),
            "expected core/cli hash-module-forms path to be attempted first, got: {err}"
        );
    }

    #[test]
    fn selfhost_hash_requires_a_selfhost_hash_binding_and_does_not_fallback_to_rust() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let env = prelude.env;
        let forms = canonicalize_module(parse_module("(def x 1)\n x\n").unwrap()).unwrap();
        let err = selfhost_hash_module_forms(&mut ctx, &env, &forms).unwrap_err();
        assert!(
            format!("{err}").contains("missing binding core/cli::hash-module-forms"),
            "expected missing-binding error, got: {err}"
        );
    }

    #[test]
    fn selfhost_optimize_prefers_core_cli_optimize_module_handler_when_present() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        env.set_local(
            "core/cli::optimize-module",
            Value::Data(Term::Str("shadowed".to_string())),
        );

        let forms = canonicalize_module(parse_module("(def x (prim int/add 1 2))\n x\n").unwrap())
            .expect("canonical module");
        let err = selfhost_optimize_module_forms(&mut ctx, &env, &forms).unwrap_err();
        assert!(
            format!("{err}").contains("not callable"),
            "expected core/cli optimize-module path to be attempted first, got: {err}"
        );
    }

    #[test]
    fn selfhost_optimize_requires_core_cli_binding_and_does_not_fallback_to_rust() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let env = prelude.env;
        let forms = canonicalize_module(parse_module("(def x (prim int/add 1 2))\n x\n").unwrap())
            .expect("canonical module");
        let err = selfhost_optimize_module_forms(&mut ctx, &env, &forms).unwrap_err();
        assert!(
            format!("{err}").contains("missing binding core/cli::optimize-module"),
            "expected missing-binding error, got: {err}"
        );
    }

    #[test]
    fn selfhost_infer_effects_prefers_core_cli_handler_when_present() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        env.set_local(
            "core/cli::infer-effects",
            Value::Data(Term::Str("shadowed".to_string())),
        );

        let forms = canonicalize_module(
            parse_module("(def p (core/effect::perform 'sys/time::now {} (fn (x) x)))\n").unwrap(),
        )
        .expect("canonical module");
        let err = selfhost_infer_effects_forms(&mut ctx, &env, &forms).unwrap_err();
        assert!(
            format!("{err}").contains("not callable"),
            "expected core/cli infer-effects path to be attempted first, got: {err}"
        );
    }

    #[test]
    fn selfhost_infer_effects_matches_gc_types_for_pkg_basic_fixture() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        let forms = canonicalize_module(
            parse_module(include_str!("../../../tests/spec/pkg_basic/basic.gc")).unwrap(),
        )
        .expect("canonical module");
        let rust = gc_types::infer_effects(&forms);
        let selfhost = selfhost_infer_effects_forms(&mut ctx, &env, &forms).expect("infer");
        assert_eq!(selfhost.unknown, rust.unknown);
        assert_eq!(selfhost.ops, rust.ops);
    }

    #[test]
    fn selfhost_infer_effects_matches_gc_types_for_pkg_fail_caps_declared_fixture() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        let forms = canonicalize_module(
            parse_module(include_str!(
                "../../../tests/spec/pkg_fail_caps_declared/fail.gc"
            ))
            .unwrap(),
        )
        .expect("canonical module");
        let rust = gc_types::infer_effects(&forms);
        let selfhost = selfhost_infer_effects_forms(&mut ctx, &env, &forms).expect("infer");
        assert_eq!(selfhost.unknown, rust.unknown);
        assert_eq!(selfhost.ops, rust.ops);
    }

    #[test]
    fn selfhost_literal_op_and_flatten_app_detect_quoted_effect_op() {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("selfhost/toolchain.gc");
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .expect("load selfhost toolchain");

        let forms = canonicalize_module(parse_module("(((core/effect::perform (quote io/fs::write)) {:data \"x\" :path \"out.txt\"}) (fn (_) (core/effect::pure nil)))\n").unwrap())
            .expect("canonical module");
        let app = forms.first().expect("one form").clone();
        let app_items = app.as_proper_list().expect("app proper list");
        let inner = app_items[0].clone();
        let inner_debug = format!("{inner:?}");

        let flatten = env
            .get("core/cli::flatten-app")
            .expect("flatten-app binding");
        let flat_v = flatten
            .clone()
            .apply(&mut ctx, Value::Data(app.clone()))
            .expect("flatten apply");
        let flat_t = flat_v.to_term_for_log(ctx.protocol.map(|p| p.error));
        let flat_map = match flat_t {
            Term::Map(m) => m,
            other => panic!("flatten-app returned non-map: {}", print_term(&other)),
        };
        let args = match flat_map.get(&TermOrdKey(Term::symbol(":args"))) {
            Some(Term::Vector(v)) => v.clone(),
            other => panic!("flatten-app args missing/non-vector: {:?}", other),
        };
        let args_debug = format!("{args:?}");
        assert_eq!(args.len(), 3, "flatten-app args length mismatch");

        let lit = env
            .get("core/cli::literal-op-sym-or-nil")
            .expect("literal-op binding");
        let mut found = false;
        let mut debug_rows: Vec<String> = Vec::new();
        let app_render = print_term(&app);
        let inner_render = print_term(&inner);
        let flat_render = print_term(&Term::Map(flat_map.clone()));
        let flat_inner_v = flatten
            .clone()
            .apply(&mut ctx, Value::Data(inner))
            .expect("flatten inner apply");
        let flat_inner_t = flat_inner_v.to_term_for_log(ctx.protocol.map(|p| p.error));
        let flat_inner_render = print_term(&flat_inner_t);
        for arg in args {
            let arg_render = print_term(&arg);
            let op_v = lit
                .clone()
                .apply(&mut ctx, Value::Data(arg))
                .expect("literal-op apply");
            let op_t = op_v.to_term_for_log(ctx.protocol.map(|p| p.error));
            debug_rows.push(format!("{arg_render} => {}", print_term(&op_t)));
            if let Term::Symbol(s) = op_t
                && s == "io/fs::write"
            {
                found = true;
            }
        }
        assert!(
            found,
            "literal-op-sym-or-nil failed to detect io/fs::write; app={} inner={} inner_debug={} flat={} flat_inner={} args_debug={} rows={}",
            app_render,
            inner_render,
            inner_debug,
            flat_render,
            flat_inner_render,
            args_debug,
            debug_rows.join(" | ")
        );
    }
}
