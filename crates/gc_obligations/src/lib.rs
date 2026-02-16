mod error;
mod registry_policy;
mod signing;
mod store;
mod transparency;
mod verify;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, canonicalize_module, hash_module, parse_module, print_term};
use gc_effects::{CapsPolicy, EffectLog};
use gc_kernel::{Apply, Env, EvalCtx, MemLimits, StepLimit, Value, eval_term, value_hash};
use gc_prelude::build_prelude;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

pub use crate::error::ObligationError;
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
struct LoadedModule {
    entry: ModuleEntry,
    abs_path: PathBuf,
    forms: Vec<Term>,
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

fn mk_eval_ctx(limits: KernelLimits) -> EvalCtx {
    let mut ctx = EvalCtx::with_step_limit(limits.step_limit.resolve());
    ctx.set_mem_limits(limits.mem_limits);
    ctx
}

pub fn pack(pkg_toml: &Path) -> Result<String, ObligationError> {
    let (manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| ObligationError::Manifest(e.to_string()))?;
    let modules = load_modules(&pkg_dir, &manifest.modules)?;

    // Compute dependency package hashes (recursive) to lock.
    let deps = pack_dep_hashes(&pkg_dir, &manifest.dependencies)?;

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
    compute_package_artifact_hash(pkg_toml, true, &mut visited)
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
    let modules = match load_modules(&pkg_dir, &manifest.modules) {
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
        && let Err(e) = check_dep_hashes(&pkg_dir, &manifest.dependencies)
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

    // Discover test ids (suite_sym + test_name) once.
    let test_ids = discover_tests(&pkg_dir, &manifest, &modules, limits)?;

    // Execute tests (each test gets a fresh ctx/env build).
    let mut test_runs = Vec::new();
    for id in &test_ids {
        test_runs.push(run_one_test(
            &pkg_dir,
            &manifest,
            &modules,
            &caps,
            id.clone(),
            limits,
        )?);
    }

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
            "core/obligation::typecheck" => obligation_typecheck(&store, &modules),
            "core/obligation::stage1-validation" => {
                obligation_stage1_validation(&store, &manifest, &modules)
            }
            "core/obligation::translation-validation" => obligation_translation_validation(
                &store, &pkg_dir, &manifest, &modules, &caps, &test_ids, limits,
            ),
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

    Ok(PackageTestResult {
        ok: ok_all,
        acceptance_artifact,
        obligation_results,
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

    // Both are finite (Default or explicit Limit).
    let cli_n = cli.resolve().expect("finite step limit");
    let pkg_n = pkg.resolve().expect("finite step limit");
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

fn write_last_acceptance(pkg_dir: &Path, hex: &str) -> Result<(), ObligationError> {
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
) -> Result<Vec<LoadedModule>, ObligationError> {
    let mut out = Vec::new();
    for e in entries {
        let abs = pkg_dir.join(&e.path);
        let src = std::fs::read_to_string(&abs)?;
        let forms = parse_module(&src).map_err(|pe| ObligationError::Module(format!("{pe}")))?;
        let forms =
            canonicalize_module(forms).map_err(|e| ObligationError::Module(e.to_string()))?;
        let h = hash_module(&forms);
        out.push(LoadedModule {
            entry: e.clone(),
            abs_path: abs,
            forms,
            hash: h,
        });
    }
    Ok(out)
}

fn pack_dep_hashes(
    pkg_dir: &Path,
    deps: &[DepEntry],
) -> Result<Vec<(String, String, String)>, ObligationError> {
    let mut out = Vec::new();
    for d in deps {
        let dep_path = pkg_dir.join(&d.path);
        let dep_pkg = if dep_path.is_dir() {
            dep_path.join("package.toml")
        } else {
            dep_path
        };
        let hex = pack(&dep_pkg)?;
        out.push((d.name.clone(), d.path.clone(), hex));
    }
    Ok(out)
}

fn check_dep_hashes(pkg_dir: &Path, deps: &[DepEntry]) -> Result<(), ObligationError> {
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
        let got = compute_package_artifact_hash(&dep_pkg, true, &mut visited)?;
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
    let modules = load_modules(&pkg_dir, &manifest.modules)?;
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
        let dep_hash = compute_package_artifact_hash(&dep_pkg, require_pinned, visited)?;
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

fn discover_tests(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<Vec<TestId>, ObligationError> {
    if manifest.tests.is_empty() {
        return Ok(Vec::new());
    }

    let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
    let mut ids = Vec::new();
    for suite in &manifest.tests {
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

fn run_one_test(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    id: TestId,
    limits: KernelLimits,
) -> Result<TestRun, ObligationError> {
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;

    // Evaluate dependencies (export-only) into base env.
    base = eval_dependencies(&mut ctx, pkg_dir, &base, &manifest.dependencies)?;

    // Evaluate modules and collect module envs for internal lookup.
    let evals = eval_modules(&mut ctx, &base, modules)?;
    let pkg = PackageEval::from_modules(base, evals)?;

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
        .apply(&mut ctx, Value::Data(Term::Nil))
        .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;

    let (final_value, effect_log) = match value {
        Value::EffectProgram(_) => {
            let prog_h = value_hash(&value);
            let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
            let r = gc_effects::run(&mut ctx, caps, value, prog_h, toolchain)
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

fn obligation_budgets(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();

    let max_steps = manifest.budgets.max_steps_per_test;
    let max_entries = manifest.budgets.max_effect_entries_per_test;
    let max_log_bytes = manifest.budgets.max_effect_log_bytes_per_test;

    let mut test_terms: Vec<Term> = Vec::new();
    for t in tests {
        let mut t_ok = true;
        if let Some(ms) = max_steps
            && t.steps > ms
        {
            t_ok = false;
            errors.push(format!(
                "test {}::{} exceeded max_steps_per_test: {} > {}",
                t.id.suite_sym, t.id.test_name, t.steps, ms
            ));
        }
        if let Some(me) = max_entries
            && t.effect_entries > me
        {
            t_ok = false;
            errors.push(format!(
                "test {}::{} exceeded max_effect_entries_per_test: {} > {}",
                t.id.suite_sym, t.id.test_name, t.effect_entries, me
            ));
        }
        if let Some(ml) = max_log_bytes
            && t.effect_log_bytes > ml
        {
            t_ok = false;
            errors.push(format!(
                "test {}::{} exceeded max_effect_log_bytes_per_test: {} > {}",
                t.id.suite_sym, t.id.test_name, t.effect_log_bytes, ml
            ));
        }
        ok &= t_ok;

        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(t.id.suite_sym.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(t.id.test_name.clone()),
        );
        m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(t_ok));
        m.insert(
            TermOrdKey(Term::symbol(":steps")),
            Term::Int((t.steps as i64).into()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":effect-entries")),
            Term::Int((t.effect_entries as i64).into()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":effect-log-bytes")),
            Term::Int((t.effect_log_bytes as i64).into()),
        );
        test_terms.push(Term::Map(m));
    }

    let mut limits = BTreeMap::new();
    if let Some(ms) = max_steps {
        limits.insert(
            TermOrdKey(Term::symbol(":max-steps-per-test")),
            Term::Int((ms as i64).into()),
        );
    }
    if let Some(me) = max_entries {
        limits.insert(
            TermOrdKey(Term::symbol(":max-effect-entries-per-test")),
            Term::Int((me as i64).into()),
        );
    }
    if let Some(ml) = max_log_bytes {
        limits.insert(
            TermOrdKey(Term::symbol(":max-effect-log-bytes-per-test")),
            Term::Int((ml as i64).into()),
        );
    }

    let mut report = BTreeMap::new();
    report.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/budgets-v0.2".to_string()),
    );
    report.insert(
        TermOrdKey(Term::symbol(":package")),
        Term::Str(manifest.name.clone()),
    );
    report.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    report.insert(TermOrdKey(Term::symbol(":limits")), Term::Map(limits));
    report.insert(TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms));
    if !errors.is_empty() {
        report.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        );
    }

    let report = Term::Map(report);
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::budgets".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

fn obligation_coverage(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    // Coverage definition (v0.2): each non-test exported symbol must be *looked up as a variable*
    // at least once during the package unit tests.
    //
    // "Non-test export" means: exports listed in module ::meta :exports, excluding any suite
    // symbols configured in package.toml `tests` or `property_tests`.
    let mut exports: BTreeSet<String> = BTreeSet::new();
    for m in modules {
        let Some(meta) = extract_meta_static(&m.forms) else {
            continue;
        };
        let Some(es) = meta_exports(&meta) else {
            continue;
        };
        exports.extend(es);
    }

    let mut excluded: BTreeSet<String> = BTreeSet::new();
    excluded.extend(manifest.tests.iter().cloned());
    excluded.extend(manifest.property_tests.iter().cloned());

    let tracked: BTreeSet<String> = exports.difference(&excluded).cloned().collect();
    if tracked.is_empty() {
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/coverage-v0.2".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str(manifest.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":note")),
                    Term::Str("no non-test exports".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let artifact = store.put_term(&report)?;
        return Ok(ObligationResult {
            name: "core/obligation::coverage".to_string(),
            ok: true,
            artifact: Some(artifact),
            errors: Vec::new(),
        });
    }

    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();

    if tests.is_empty() {
        ok = false;
        errors.push("coverage requires unit tests (package.toml `tests` is empty)".to_string());
    }

    // Used for replaying effectful tests without re-running capabilities.
    let effect_store = gc_effects::ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
        .map_err(|e| ObligationError::Test(format!("artifact store open failed: {e}")))?;

    let mut total_hits: BTreeMap<String, u64> = BTreeMap::new();
    let mut test_terms: Vec<Term> = Vec::new();

    for t in tests {
        let mut ctx = mk_eval_ctx(limits);
        ctx.enable_coverage(tracked.clone());

        let prelude = build_prelude(&mut ctx);
        let mut base = prelude.env;
        base = eval_dependencies(&mut ctx, pkg_dir, &base, &manifest.dependencies)?;
        let evals = eval_modules(&mut ctx, &base, modules)?;
        let pkg = PackageEval::from_modules(base, evals)?;

        let suite_v = pkg.lookup_any(&t.id.suite_sym).ok_or_else(|| {
            ObligationError::Test(format!("missing test suite symbol {}", t.id.suite_sym))
        })?;
        let suite_map = value_as_map(&suite_v).ok_or_else(|| {
            ObligationError::Test(format!("test suite {} must be a map", t.id.suite_sym))
        })?;
        let (test_body, _expect) = parse_test_entry(
            suite_map
                .get(&TermOrdKey(Term::Str(t.id.test_name.clone())))
                .or_else(|| suite_map.get(&TermOrdKey(Term::Symbol(t.id.test_name.clone()))))
                .ok_or_else(|| {
                    ObligationError::Test(format!(
                        "missing test {} in suite {}",
                        t.id.test_name, t.id.suite_sym
                    ))
                })?,
        )?;

        let value = test_body
            .apply(&mut ctx, Value::Data(Term::Nil))
            .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;

        match (value, &t.effect_log) {
            (v @ Value::EffectProgram(_), Some(log)) => {
                let _ = gc_effects::replay_with_store(&mut ctx, v, log, Some(&effect_store))
                    .map_err(|e| ObligationError::Test(format!("replay failed: {e}")))?;
            }
            (Value::EffectProgram(_), None) => {
                ok = false;
                errors.push(format!(
                    "coverage: test {} returned effect program but no effect log was recorded",
                    t.id.test_name
                ));
            }
            _ => {}
        }

        let mut hits_vec: Vec<Term> = Vec::new();
        if let Some(hits) = ctx.coverage_hits() {
            for (sym, c) in hits {
                if *c == 0 {
                    continue;
                }
                *total_hits.entry(sym.clone()).or_insert(0) += *c;
                hits_vec.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":sym")), Term::Symbol(sym.clone())),
                        (
                            TermOrdKey(Term::symbol(":hits")),
                            Term::Int((*c as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }
        }

        test_terms.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":suite")),
                    Term::Symbol(t.id.suite_sym.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":name")),
                    Term::Str(t.id.test_name.clone()),
                ),
                (TermOrdKey(Term::symbol(":hits")), Term::Vector(hits_vec)),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let mut missing: Vec<Term> = Vec::new();
    let mut export_terms: Vec<Term> = Vec::new();
    for sym in &tracked {
        let c = *total_hits.get(sym).unwrap_or(&0);
        if c == 0 {
            ok = false;
            missing.push(Term::Symbol(sym.clone()));
            errors.push(format!("export not covered: {sym}"));
        }
        export_terms.push(Term::Map(
            [
                (TermOrdKey(Term::symbol(":sym")), Term::Symbol(sym.clone())),
                (
                    TermOrdKey(Term::symbol(":hits")),
                    Term::Int((c as i64).into()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/coverage-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":definition")),
                Term::Str("exports minus (tests, property_tests)".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":exports")),
                Term::Vector(export_terms),
            ),
            (TermOrdKey(Term::symbol(":missing")), Term::Vector(missing)),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms)),
        ]
        .into_iter()
        .collect(),
    );
    let report = if errors.is_empty() {
        report
    } else {
        let Term::Map(mut m) = report else {
            unreachable!()
        };
        m.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        );
        Term::Map(m)
    };

    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::coverage".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

#[derive(Debug, Clone)]
struct PropertyTest {
    id: TestId,
    body: Value,
    cases: u64,
}

fn obligation_property_tests(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let default_cases = manifest.property.cases_per_test.unwrap_or(64);
    if manifest.property_tests.is_empty() {
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/property-tests-v0.2".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":package")),
                    Term::Str(manifest.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":note")),
                    Term::Str("no property tests".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let artifact = store.put_term(&report)?;
        return Ok(ObligationResult {
            name: "core/obligation::property-tests".to_string(),
            ok: true,
            artifact: Some(artifact),
            errors: Vec::new(),
        });
    }

    // Evaluate package once to extract property bodies and per-test case counts.
    let eval = eval_package_once(pkg_dir, manifest, modules, limits)?;
    let mut props: Vec<PropertyTest> = Vec::new();

    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();
    let mut test_terms: Vec<Term> = Vec::new();

    for suite in &manifest.property_tests {
        let Some(suite_v) = eval.lookup_any(suite) else {
            ok = false;
            errors.push(format!("missing property suite symbol {suite}"));
            continue;
        };
        let Some(suite_map) = value_as_map(&suite_v) else {
            ok = false;
            errors.push(format!("property suite {suite} must be a map"));
            continue;
        };
        for (k, vv) in suite_map.iter() {
            let name = match &k.0 {
                Term::Str(s) => s.clone(),
                Term::Symbol(s) => s.clone(),
                other => {
                    ok = false;
                    errors.push(format!(
                        "property suite {suite}: key must be string/symbol, got {}",
                        print_term(other)
                    ));
                    continue;
                }
            };
            match parse_property_entry(vv, default_cases) {
                Ok((body, cases)) => props.push(PropertyTest {
                    id: TestId {
                        suite_sym: suite.clone(),
                        test_name: name,
                    },
                    body,
                    cases,
                }),
                Err(e) => {
                    ok = false;
                    errors.push(format!("property suite {suite}::{name}: {e}"));
                }
            }
        }
    }

    for p in &props {
        let mut seeds: Vec<u64> = Vec::with_capacity(p.cases as usize);
        for i in 0..p.cases {
            seeds.push(seed_for_case(
                &manifest.name,
                &p.id.suite_sym,
                &p.id.test_name,
                i,
            ));
        }

        let mut t_ok = true;
        let mut first_failure: Option<Term> = None;

        for (i, seed) in seeds.iter().copied().enumerate() {
            let mut ctx = mk_eval_ctx(limits);
            let arg = Value::Data(Term::Int(BigInt::from(seed)));
            let r = match p.body.clone().apply(&mut ctx, arg) {
                Ok(v) => v,
                Err(e) => {
                    t_ok = false;
                    first_failure = Some(Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":i")), Term::Int((i as i64).into())),
                            (
                                TermOrdKey(Term::symbol(":seed")),
                                Term::Int(BigInt::from(seed)),
                            ),
                            (
                                TermOrdKey(Term::symbol(":result")),
                                Term::Str(format!("apply failed: {e}")),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ));
                    errors.push(format!(
                        "property test apply failed {}::{} at case {}: {e}",
                        p.id.suite_sym, p.id.test_name, i
                    ));
                    break;
                }
            };

            if matches!(r, Value::EffectProgram(_)) {
                t_ok = false;
                first_failure = Some(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":i")), Term::Int((i as i64).into())),
                        (
                            TermOrdKey(Term::symbol(":seed")),
                            Term::Int(BigInt::from(seed)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":result")),
                            Term::Str(
                                "effect program returned (property tests must be pure)".to_string(),
                            ),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
                errors.push(format!(
                    "property test {}::{} returned an effect program (must be pure)",
                    p.id.suite_sym, p.id.test_name
                ));
                break;
            }

            let is_error = ctx
                .protocol
                .is_some_and(|pt| matches!(r, Value::Sealed { token, .. } if token == pt.error));

            let pass = matches!(r, Value::Data(Term::Bool(true))) && !is_error;
            if !pass {
                t_ok = false;
                let proto_err = ctx.protocol.map(|pt| pt.error);
                let rt = r.to_term_for_log(proto_err);
                first_failure = Some(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":i")), Term::Int((i as i64).into())),
                        (
                            TermOrdKey(Term::symbol(":seed")),
                            Term::Int(BigInt::from(seed)),
                        ),
                        (TermOrdKey(Term::symbol(":result")), rt),
                    ]
                    .into_iter()
                    .collect(),
                ));
                errors.push(format!(
                    "property test failed {}::{} at case {}",
                    p.id.suite_sym, p.id.test_name, i
                ));
                break;
            }
        }

        ok &= t_ok;

        let mut tm = BTreeMap::new();
        tm.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(p.id.suite_sym.clone()),
        );
        tm.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(p.id.test_name.clone()),
        );
        tm.insert(
            TermOrdKey(Term::symbol(":cases")),
            Term::Int((p.cases as i64).into()),
        );
        tm.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(t_ok));
        tm.insert(
            TermOrdKey(Term::symbol(":seeds")),
            Term::Vector(
                seeds
                    .iter()
                    .copied()
                    .map(|s| Term::Int(BigInt::from(s)))
                    .collect(),
            ),
        );
        if let Some(ff) = first_failure {
            tm.insert(TermOrdKey(Term::symbol(":first-failure")), ff);
        }
        test_terms.push(Term::Map(tm));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/property-tests-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":config")),
                Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":cases-per-test")),
                        Term::Int((default_cases as i64).into()),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms)),
        ]
        .into_iter()
        .collect(),
    );
    let report = if errors.is_empty() {
        report
    } else {
        let Term::Map(mut m) = report else {
            unreachable!()
        };
        m.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        );
        Term::Map(m)
    };
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::property-tests".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

fn parse_property_entry(v: &Value, default_cases: u64) -> Result<(Value, u64), ObligationError> {
    if matches!(v, Value::Closure { .. } | Value::NativeFn(_)) {
        return Ok((v.clone(), default_cases));
    }
    let Some(m) = value_as_map(v) else {
        return Err(ObligationError::Test(format!(
            "invalid property entry: {}",
            v.debug_repr()
        )));
    };
    let body = m
        .get(&TermOrdKey(Term::Symbol(":body".to_string())))
        .ok_or_else(|| ObligationError::Test("property map missing :body".to_string()))?;
    if !matches!(body, Value::Closure { .. } | Value::NativeFn(_)) {
        return Err(ObligationError::Test(
            "property :body must be callable".to_string(),
        ));
    }
    let cases = match m.get(&TermOrdKey(Term::Symbol(":cases".to_string()))) {
        None => default_cases,
        Some(Value::Data(Term::Int(i))) => i
            .to_u64()
            .ok_or_else(|| ObligationError::Test("property :cases must fit u64".to_string()))?,
        Some(other) => {
            return Err(ObligationError::Test(format!(
                "property :cases must be int, got {}",
                other.debug_repr()
            )));
        }
    };
    Ok((body.clone(), cases))
}

fn seed_for_case(pkg: &str, suite: &str, name: &str, i: u64) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(b"GCv0.2\0property\0seed\0");
    h.update(pkg.as_bytes());
    h.update(b"\0");
    h.update(suite.as_bytes());
    h.update(b"\0");
    h.update(name.as_bytes());
    h.update(b"\0");
    h.update(&i.to_le_bytes());
    let out = h.finalize();
    let mut b = [0u8; 8];
    b.copy_from_slice(&out.as_bytes()[0..8]);
    u64::from_le_bytes(b)
}

fn parse_test_entry(v: &Value) -> Result<(Value, Option<Term>), ObligationError> {
    // Either a callable directly, or a map { :body callable :expect datum }
    if matches!(v, Value::Closure { .. } | Value::NativeFn(_)) {
        return Ok((v.clone(), None));
    }
    if let Some(m) = value_as_map(v) {
        let body = m
            .get(&TermOrdKey(Term::Symbol(":body".to_string())))
            .ok_or_else(|| ObligationError::Test("test map missing :body".to_string()))?;
        if !matches!(body, Value::Closure { .. } | Value::NativeFn(_)) {
            return Err(ObligationError::Test(
                "test :body must be callable".to_string(),
            ));
        }
        let expect = match m.get(&TermOrdKey(Term::Symbol(":expect".to_string()))) {
            None => None,
            Some(Value::Data(t)) => Some(t.clone()),
            Some(other) => {
                return Err(ObligationError::Test(format!(
                    "test :expect must be a datum, got {}",
                    other.debug_repr()
                )));
            }
        };
        return Ok((body.clone(), expect));
    }
    Err(ObligationError::Test(format!(
        "invalid test entry: {}",
        v.debug_repr()
    )))
}

fn obligation_unit_tests(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut test_terms = Vec::new();
    for t in tests {
        ok &= t.ok;
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(t.id.suite_sym.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(t.id.test_name.clone()),
        );
        m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(t.ok));
        m.insert(
            TermOrdKey(Term::symbol(":value-h")),
            Term::Bytes(t.value_hash.to_vec().into()),
        );
        if let Some(e) = &t.error {
            m.insert(TermOrdKey(Term::symbol(":error")), Term::Str(e.clone()));
        }
        if let Some(log) = &t.effect_log {
            let log_h = store.put_term(&log.to_term())?;
            m.insert(TermOrdKey(Term::symbol(":log-artifact")), Term::Str(log_h));
        }
        test_terms.push(Term::Map(m));
    }
    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/unit-tests-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(test_terms)),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::unit-tests".to_string(),
        ok,
        artifact: Some(artifact),
        errors: Vec::new(),
    })
}

fn obligation_determinism(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    // Rule: if a module declares :caps = [], then its inferred effect ops must be empty,
    // and any tests defined by that module must not perform effects.
    let mut errors = Vec::new();
    let mut ok = true;

    // Static scan.
    for m in modules {
        let inf = gc_types::infer_effects(&m.forms);
        let meta = extract_meta_static(&m.forms);
        if let Some(meta) = meta
            && let Some(caps) = meta_caps(&meta)
            && caps.is_empty()
            && (inf.unknown || !inf.ops.is_empty())
        {
            ok = false;
            errors.push(format!(
                "{} declares :caps [] but has inferred effects (unknown={}, ops={:?})",
                m.entry.path, inf.unknown, inf.ops
            ));
        }
    }

    // Runtime check: any effectful test for a module with :caps [] fails.
    // We approximate by mapping suite symbol -> module (static def scan).
    let suite_to_mod = suite_to_module(modules);
    for t in tests {
        if let Some(mod_i) = suite_to_mod.get(&t.id.suite_sym)
            && let Some(meta) = extract_meta_static(&modules[*mod_i].forms)
            && let Some(caps) = meta_caps(&meta)
        {
            let observed_effects = t.effect_log.as_ref().is_some_and(|l| !l.entries.is_empty());
            if caps.is_empty() && observed_effects {
                ok = false;
                errors.push(format!(
                    "test {} in {} performed effects but module declares :caps []",
                    t.id.test_name, t.id.suite_sym
                ));
            }
        }
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/determinism-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::determinism".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

fn obligation_caps_declared(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors = Vec::new();
    let suite_to_mod = suite_to_module(modules);

    for t in tests {
        let Some(log) = &t.effect_log else { continue };
        let used: BTreeSet<String> = log.entries.iter().map(|e| e.op.clone()).collect();
        let Some(mod_i) = suite_to_mod.get(&t.id.suite_sym) else {
            ok = false;
            errors.push(format!(
                "cannot find defining module for suite {}",
                t.id.suite_sym
            ));
            continue;
        };
        let meta = extract_meta_static(&modules[*mod_i].forms).ok_or_else(|| {
            ObligationError::Test(format!(
                "module {} missing ::meta for caps check",
                modules[*mod_i].entry.path
            ))
        })?;
        let declared = meta_caps(&meta).ok_or_else(|| {
            ObligationError::Test(format!(
                "module {} ::meta missing :caps",
                modules[*mod_i].entry.path
            ))
        })?;
        let declared: BTreeSet<String> = declared.into_iter().collect();
        for op in used {
            if !declared.contains(&op) {
                ok = false;
                errors.push(format!(
                    "test {} used op {} but module {} did not declare it in :caps",
                    t.id.test_name, op, modules[*mod_i].entry.path
                ));
            }
        }
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/caps-declared-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::capabilities-declared".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

fn obligation_replayable(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    tests: &[TestRun],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors = Vec::new();
    let effect_store = gc_effects::ArtifactStore::open(&pkg_dir.join(".genesis").join("store"))
        .map_err(|e| ObligationError::Test(format!("artifact store open failed: {e}")))?;
    for t in tests {
        let Some(log) = &t.effect_log else { continue };

        // Re-evaluate and replay.
        let mut ctx = mk_eval_ctx(limits);
        let prelude = build_prelude(&mut ctx);
        let mut base = prelude.env;
        base = eval_dependencies(&mut ctx, pkg_dir, &base, &manifest.dependencies)?;
        let evals = eval_modules(&mut ctx, &base, modules)?;
        let pkg = PackageEval::from_modules(base, evals)?;

        let suite_v = pkg.lookup_any(&t.id.suite_sym).ok_or_else(|| {
            ObligationError::Test(format!("missing test suite symbol {}", t.id.suite_sym))
        })?;
        let suite_map = value_as_map(&suite_v).ok_or_else(|| {
            ObligationError::Test(format!("test suite {} must be a map", t.id.suite_sym))
        })?;
        let (test_body, _expect) = parse_test_entry(
            suite_map
                .get(&TermOrdKey(Term::Str(t.id.test_name.clone())))
                .or_else(|| suite_map.get(&TermOrdKey(Term::Symbol(t.id.test_name.clone()))))
                .ok_or_else(|| {
                    ObligationError::Test(format!(
                        "missing test {} in suite {}",
                        t.id.test_name, t.id.suite_sym
                    ))
                })?,
        )?;
        let value = test_body
            .apply(&mut ctx, Value::Data(Term::Nil))
            .map_err(|e| ObligationError::Test(format!("test apply failed: {e}")))?;
        let Value::EffectProgram(_) = value else {
            ok = false;
            errors.push(format!(
                "test {} expected effect program for replayability",
                t.id.test_name
            ));
            continue;
        };
        let v2 = gc_effects::replay_with_store(&mut ctx, value, log, Some(&effect_store))
            .map_err(|e| ObligationError::Test(format!("replay failed: {e}")))?;
        let h2 = value_hash(&v2);
        if h2 != t.value_hash {
            ok = false;
            errors.push(format!(
                "replay mismatch for {}: {}",
                t.id.test_name,
                hex32(h2)
            ));
        }

        // Store log artifact too (for provenance).
        let _ = store.put_term(&log.to_term())?;
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/replayable-tests-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::replayable-tests".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

fn obligation_typecheck(
    store: &EvidenceStore,
    modules: &[LoadedModule],
) -> Result<ObligationResult, ObligationError> {
    let mut mods = Vec::new();
    for m in modules {
        let meta = extract_meta_static(&m.forms);
        mods.push(gc_types::ModuleForTypecheck {
            path: m.entry.path.clone(),
            forms: m.forms.clone(),
            meta,
        });
    }
    let report = gc_types::typecheck_package(&mods);
    let ok = report.ok;
    let artifact = store.put_term(&report.to_term())?;
    Ok(ObligationResult {
        name: "core/obligation::typecheck".to_string(),
        ok,
        artifact: Some(artifact),
        errors: report.errors,
    })
}

fn obligation_stage1_validation(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
) -> Result<ObligationResult, ObligationError> {
    let mut ok = true;
    let mut errors = Vec::new();
    let mut module_reports = Vec::new();

    for m in modules {
        let out =
            gc_opt::stage1_pipeline(&m.forms).map_err(|e| ObligationError::Opt(format!("{e}")))?;
        if !out.gate_report.ok {
            ok = false;
            for e in &out.gate_report.errors {
                errors.push(format!("{}: {e}", m.entry.path));
            }
        }
        module_reports.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(m.entry.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":ok")),
                    Term::Bool(out.gate_report.ok),
                ),
                (
                    TermOrdKey(Term::symbol(":original-module-h")),
                    Term::Bytes(out.gate_report.original_module_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":transformed-module-h")),
                    Term::Bytes(out.gate_report.transformed_module_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":original-value-h")),
                    out.gate_report
                        .original_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":transformed-value-h")),
                    out.gate_report
                        .transformed_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(
                        out.gate_report
                            .errors
                            .iter()
                            .cloned()
                            .map(Term::Str)
                            .collect(),
                    ),
                ),
                (
                    TermOrdKey(Term::symbol(":optimizer")),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":egg-runs")),
                                Term::Int((out.optimize_report.stats.egg_runs as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":egg-iterations")),
                                Term::Int((out.optimize_report.stats.iterations as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":egg-eclasses")),
                                Term::Int((out.optimize_report.stats.eclasses as i64).into()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":egg-enodes")),
                                Term::Int((out.optimize_report.stats.enodes as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/stage1-validation-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":obligation")),
                Term::Str("core/obligation::stage1-validation".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_reports),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::stage1-validation".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

fn obligation_translation_validation(
    store: &EvidenceStore,
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    caps: &CapsPolicy,
    test_ids: &[TestId],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    // Conservative v0.2: we only validate optimization by re-running the *whole package*
    // tests against an optimized copy of each module and comparing per-test hashes.
    //
    // If there are no tests, treat as pass.
    if test_ids.is_empty() {
        let report = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str("genesis/translation-validation-v0.2".to_string()),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":note")),
                    Term::Str("no tests".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let artifact = store.put_term(&report)?;
        return Ok(ObligationResult {
            name: "core/obligation::translation-validation".to_string(),
            ok: true,
            artifact: Some(artifact),
            errors: Vec::new(),
        });
    }

    let mut ok = true;
    let mut errors = Vec::new();
    let mut per_test = Vec::new();
    let mut stage2_entries = Vec::new();
    let mut stage2_supported: u64 = 0;
    let mut stage2_validated: u64 = 0;

    // Optimize modules once and record optimizer statistics as evidence.
    let mut opt_modules = Vec::new();
    let mut opt_stats = gc_opt::OptimizeStats::default();
    let mut mod_terms: Vec<Term> = Vec::new();
    for m in modules {
        let orig_h = hash_module(&m.forms);
        let (opt_forms, rep) = gc_opt::optimize_module_with_report(&m.forms);
        opt_stats.egg_runs = opt_stats.egg_runs.saturating_add(rep.stats.egg_runs);
        opt_stats.iterations = opt_stats.iterations.saturating_add(rep.stats.iterations);
        opt_stats.eclasses = opt_stats.eclasses.saturating_add(rep.stats.eclasses);
        opt_stats.enodes = opt_stats.enodes.saturating_add(rep.stats.enodes);
        for (k, v) in rep.stats.rewrites_applied {
            *opt_stats.rewrites_applied.entry(k).or_insert(0) += v;
        }
        let opt_h = hash_module(&opt_forms);

        let s2 = gc_opt::stage2_validation_report(&opt_forms);
        if s2.supported {
            stage2_supported = stage2_supported.saturating_add(1);
            if s2.ok {
                stage2_validated = stage2_validated.saturating_add(1);
            } else {
                ok = false;
                for e in &s2.errors {
                    errors.push(format!("stage2 {}: {e}", m.entry.path));
                }
            }
        }
        stage2_entries.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(m.entry.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":supported")),
                    Term::Bool(s2.supported),
                ),
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(s2.ok)),
                (
                    TermOrdKey(Term::symbol(":module-h")),
                    Term::Bytes(s2.module_hash.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":wasm-h")),
                    s2.wasm_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":value-kind")),
                    match s2.value_kind {
                        Some(gc_opt::Stage2ValueKind::Int) => Term::Symbol(":int".to_string()),
                        Some(gc_opt::Stage2ValueKind::Bool) => Term::Symbol(":bool".to_string()),
                        Some(gc_opt::Stage2ValueKind::Nil) => Term::Symbol(":nil".to_string()),
                        None => Term::Nil,
                    },
                ),
                (
                    TermOrdKey(Term::symbol(":orig-value-h")),
                    s2.original_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":wasm-value-h")),
                    s2.wasm_value_hash
                        .map(|h| Term::Bytes(h.to_vec().into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":wasm-bytes")),
                    s2.wasm_bytes_len
                        .map(|n| Term::Int((n as i64).into()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":errors")),
                    Term::Vector(s2.errors.iter().cloned().map(Term::Str).collect()),
                ),
            ]
            .into_iter()
            .collect(),
        ));

        mod_terms.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(m.entry.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":orig-h")),
                    Term::Bytes(orig_h.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":opt-h")),
                    Term::Bytes(opt_h.to_vec().into()),
                ),
                (
                    TermOrdKey(Term::symbol(":changed")),
                    Term::Bool(orig_h != opt_h),
                ),
            ]
            .into_iter()
            .collect(),
        ));
        opt_modules.push(LoadedModule {
            entry: m.entry.clone(),
            abs_path: m.abs_path.clone(),
            hash: opt_h,
            forms: opt_forms,
        });
    }

    for id in test_ids {
        // original
        let orig = run_one_test(pkg_dir, manifest, modules, caps, id.clone(), limits)?;
        // optimized
        let opt = run_one_test(pkg_dir, manifest, &opt_modules, caps, id.clone(), limits)?;

        if orig.value_hash != opt.value_hash {
            ok = false;
            errors.push(format!(
                "hash mismatch for {}::{}",
                id.suite_sym, id.test_name
            ));
        }
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":suite")),
            Term::Symbol(id.suite_sym.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Str(id.test_name.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":orig-h")),
            Term::Bytes(orig.value_hash.to_vec().into()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":opt-h")),
            Term::Bytes(opt.value_hash.to_vec().into()),
        );
        per_test.push(Term::Map(m));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/translation-validation-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":optimizer")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":egg-runs")),
                            Term::Int((opt_stats.egg_runs as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-iterations")),
                            Term::Int((opt_stats.iterations as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-eclasses")),
                            Term::Int((opt_stats.eclasses as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-enodes")),
                            Term::Int((opt_stats.enodes as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":egg-rewrites")),
                            Term::Vector(
                                opt_stats
                                    .rewrites_applied
                                    .iter()
                                    .map(|(k, v)| {
                                        Term::Map(
                                            [
                                                (
                                                    TermOrdKey(Term::symbol(":name")),
                                                    Term::Str(k.clone()),
                                                ),
                                                (
                                                    TermOrdKey(Term::symbol(":n")),
                                                    Term::Int((*v as i64).into()),
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
                ),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(mod_terms),
            ),
            (
                TermOrdKey(Term::symbol(":stage2")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":supported-modules")),
                            Term::Int((stage2_supported as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":validated-modules")),
                            Term::Int((stage2_validated as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":entries")),
                            Term::Vector(stage2_entries),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":tests")), Term::Vector(per_test)),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::translation-validation".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

struct PackageEval {
    modules: Vec<ModuleEval>,
    exports_env: Env,
    // A fast lookup map for "internal" names: suite/test symbol -> module env
    internal_index: BTreeMap<String, usize>,
}

impl PackageEval {
    fn from_modules(base_env: Env, modules: Vec<ModuleEval>) -> Result<Self, ObligationError> {
        let mut exports = BTreeMap::new();
        let mut internal_index = BTreeMap::new();
        for (i, m) in modules.iter().enumerate() {
            for name in m.defined.keys() {
                internal_index.entry(name.clone()).or_insert(i);
            }
            for e in &m.exports {
                let v = m.defined.get(e).ok_or_else(|| {
                    ObligationError::Module(format!(
                        "module {} exports {} but does not define it",
                        m.path.display(),
                        e
                    ))
                })?;
                exports.insert(e.clone(), v.clone());
            }
        }
        let exports_env = Env::with_bindings(&base_env, exports);
        Ok(Self {
            modules,
            exports_env,
            internal_index,
        })
    }

    fn lookup_any(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.exports_env.get(name) {
            return Some(v);
        }
        let idx = self.internal_index.get(name)?;
        self.modules[*idx].env.get(name)
    }
}

fn eval_package_once(
    pkg_dir: &Path,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<PackageEval, ObligationError> {
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let mut base = prelude.env;
    base = eval_dependencies(&mut ctx, pkg_dir, &base, &manifest.dependencies)?;
    let evals = eval_modules(&mut ctx, &base, modules)?;
    PackageEval::from_modules(base, evals)
}

fn eval_dependencies(
    ctx: &mut EvalCtx,
    pkg_dir: &Path,
    base: &Env,
    deps: &[DepEntry],
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
        let dep_modules = load_modules(&dep_dir, &dep_manifest.modules)?;

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
    let mut defined: BTreeMap<String, Value> = BTreeMap::new();

    let mut last = Value::Data(Term::Nil);
    for form in forms {
        if let Some((name, expr)) = parse_def(form) {
            let v = eval_term(ctx, &env, &expr).map_err(|e| {
                ObligationError::Module(format!("{}: def {name} eval failed: {e}", path.display()))
            })?;
            env = Env::with_binding(&env, name.clone(), v.clone());
            defined.insert(name, v);
            last = Value::Data(Term::Nil);
            continue;
        }
        last = eval_term(ctx, &env, form).map_err(|e| {
            ObligationError::Module(format!("{}: expr eval failed: {e}", path.display()))
        })?;
    }
    let _ = last;

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
}
