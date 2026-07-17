use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::Context;
#[cfg(feature = "embedded-bootstrap")]
use once_cell::sync::Lazy;

use gc_coreform::{
    HASH_DOMAIN_PREFIX, Term, TermOrdKey, canonicalize_module, hash_module, parse_module,
    parse_term, print_term,
};
use gc_kernel::{
    CompiledModule, Env, EvalCtx, EvalObservedCounters, MemLimits, Value, compile_module,
    eval_compiled_module,
};

#[path = "selfhost_compiled_cache.rs"]
mod selfhost_compiled_cache;

use selfhost_compiled_cache::{
    lock_artifact_compiled_cache, try_read_compiled_cache, write_compiled_cache,
};

const SELFHOST_TOOLCHAIN_MANIFEST_SRC: &str =
    include_str!("../../../selfhost/toolchain_manifest.gc");
const SELFHOST_TOOLCHAIN_EMBEDDED_ARTIFACT_SRC: &str =
    include_str!("../../../selfhost/toolchain.gc");

const SELFHOST_TOOLCHAIN_ARTIFACT_ENV: &str = "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT";
const SELFHOST_COMPILED_CACHE_DIR_ENV: &str = "GENESIS_SELFHOST_COMPILED_CACHE_DIR";
const SELFHOST_COMPILED_CACHE_DISABLE_ENV: &str = "GENESIS_SELFHOST_COMPILED_CACHE_DISABLE";
pub const BOOTSTRAP_PROFILE_ID: &str = "genesis/bootstrap-profile/v0.2";
pub const SELFHOST_TOOLCHAIN_ARTIFACT_KIND: &str = "genesis/selfhost-toolchain-artifact-v0.2";
pub const SELFHOST_TOOLCHAIN_ARTIFACT_VERSION: i64 = 1;
const DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = ".genesis/selfhost/toolchain.gc";
const DEFAULT_SELFHOST_COMPILED_CACHE_REL: &str = ".genesis/cache/selfhost_compiled_v1";
const SELFHOST_COMPILED_CACHE_FILE_MAGIC: &[u8] = b"GCSHC1\0";
const SELFHOST_BOOTSTRAP_EVIDENCE_SYMBOL: &str = "core/selfhost::bootstrap-evidence";
// Cache provenance is operational telemetry, not language-visible semantics.
const ARTIFACT_BOOTSTRAP_STAGE: &str = "artifact";
const PRODUCTION_BOOTSTRAP_STEP_LIMIT: u64 = 500_000_000;
const PARITY_BOOTSTRAP_STEP_LIMIT: u64 = 1_000_000_000;

#[path = "selfhost_coreform_manifest.rs"]
mod selfhost_coreform_manifest;

#[cfg(feature = "embedded-bootstrap")]
type SelfhostCompiledModules = Vec<(String, CompiledModule)>;

type CachedCompiledModules = Vec<(String, CompiledModule)>;

use selfhost_coreform_manifest::{ToolchainManifest, toolchain_manifest};

pub use selfhost_coreform_manifest::{
    embedded_bootstrap_available, selfhost_coreform_toolchain_v1_sources,
};

// Per-process cache: compiling the selfhost toolchain modules dominates costs for obligations and
// other workflows that create fresh ctx/env pairs. The artifact bytes are content-addressed, so
// caching by a content hash is safe and deterministic.
static ARTIFACT_COMPILED_CACHE: OnceLock<Mutex<BTreeMap<[u8; 32], CachedCompiledModules>>> =
    OnceLock::new();

#[cfg(feature = "embedded-bootstrap")]
static SELFHOST_COREFORM_V1: Lazy<Result<SelfhostCompiledModules, String>> = Lazy::new(|| {
    let sources = selfhost_coreform_toolchain_v1_sources().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (name, src) in sources {
        let forms = parse_module(&src).map_err(|e| format!("{name}: parse: {e}"))?;
        let forms = canonicalize_module(forms).map_err(|e| format!("{name}: canon: {e}"))?;
        let compiled = compile_module(&forms).map_err(|e| format!("{name}: compile: {e}"))?;
        out.push((name, compiled));
    }
    Ok(out)
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfhostBootstrapMode {
    ArtifactOnly,
    ArtifactPreferred,
    Embedded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootstrapRuntimeProfile {
    Production = 0,
    ParityHarness = 1,
}

static BOOTSTRAP_RUNTIME_PROFILE: AtomicU8 =
    AtomicU8::new(BootstrapRuntimeProfile::Production as u8);

pub fn set_bootstrap_runtime_profile_parity_harness(enabled: bool) {
    let value = if enabled {
        BootstrapRuntimeProfile::ParityHarness as u8
    } else {
        BootstrapRuntimeProfile::Production as u8
    };
    BOOTSTRAP_RUNTIME_PROFILE.store(value, Ordering::Relaxed);
}

fn non_artifact_bootstrap_modes_allowed() -> bool {
    BOOTSTRAP_RUNTIME_PROFILE.load(Ordering::Relaxed)
        == BootstrapRuntimeProfile::ParityHarness as u8
}

fn enforce_bootstrap_mode_allowed_with_flag(
    mode: SelfhostBootstrapMode,
    allow_non_artifact_bootstrap_modes: bool,
) -> anyhow::Result<()> {
    if mode == SelfhostBootstrapMode::ArtifactOnly || allow_non_artifact_bootstrap_modes {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "non-artifact selfhost bootstrap modes are development-only; release profile requires artifact-only"
    ))
}

fn enforce_bootstrap_mode_allowed(mode: SelfhostBootstrapMode) -> anyhow::Result<()> {
    enforce_bootstrap_mode_allowed_with_flag(mode, non_artifact_bootstrap_modes_allowed())
}

fn map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TrustedBootstrapBudget {
    profile: &'static str,
    step_limit: u64,
    mem_limits: MemLimits,
}

fn production_bootstrap_mem_limits() -> MemLimits {
    MemLimits {
        max_alloc_units: Some(500_000_000),
        max_live_units: Some(250_000_000),
        max_pair_cells: Some(12_000_000),
        max_vec_len: Some(2_000_000),
        max_map_len: Some(2_000_000),
        max_bytes_len: Some(128 * 1024 * 1024),
        max_string_len: Some(128 * 1024 * 1024),
    }
}

fn parity_bootstrap_mem_limits() -> MemLimits {
    MemLimits {
        max_alloc_units: Some(1_000_000_000),
        max_live_units: Some(500_000_000),
        max_pair_cells: Some(24_000_000),
        max_vec_len: Some(4_000_000),
        max_map_len: Some(4_000_000),
        max_bytes_len: Some(256 * 1024 * 1024),
        max_string_len: Some(256 * 1024 * 1024),
    }
}

fn trusted_bootstrap_budget() -> TrustedBootstrapBudget {
    if non_artifact_bootstrap_modes_allowed() {
        TrustedBootstrapBudget {
            profile: "parity-harness",
            step_limit: PARITY_BOOTSTRAP_STEP_LIMIT,
            mem_limits: parity_bootstrap_mem_limits(),
        }
    } else {
        TrustedBootstrapBudget {
            profile: "production",
            step_limit: PRODUCTION_BOOTSTRAP_STEP_LIMIT,
            mem_limits: production_bootstrap_mem_limits(),
        }
    }
}

fn bootstrap_limits_term(limits: MemLimits, step_limit: u64) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":step-limit")),
        Term::Int(step_limit.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-alloc-units")),
        Term::Int(limits.max_alloc_units.unwrap_or(0).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-live-units")),
        Term::Int(limits.max_live_units.unwrap_or(0).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-pair-cells")),
        Term::Int(limits.max_pair_cells.unwrap_or(0).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-vec-len")),
        Term::Int(limits.max_vec_len.unwrap_or(0).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-map-len")),
        Term::Int(limits.max_map_len.unwrap_or(0).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-bytes-len")),
        Term::Int(limits.max_bytes_len.unwrap_or(0).into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-string-len")),
        Term::Int(limits.max_string_len.unwrap_or(0).into()),
    );
    Term::Map(m)
}

fn bootstrap_observed_term(observed: EvalObservedCounters) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":steps")),
        Term::Int(observed.steps.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":allocated-units")),
        Term::Int(observed.mem.allocated_units.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":live-units")),
        Term::Int(observed.mem.live_units.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-live-units")),
        Term::Int(observed.mem.max_live_units.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":pair-cells")),
        Term::Int(observed.mem.pair_cells.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-vec-len")),
        Term::Int(observed.mem.max_vec_len.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-map-len")),
        Term::Int(observed.mem.max_map_len.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-bytes-len")),
        Term::Int(observed.mem.max_bytes_len.into()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":max-string-len")),
        Term::Int(observed.mem.max_string_len.into()),
    );
    Term::Map(m)
}

fn bootstrap_evidence_term(
    stage: &str,
    budget: TrustedBootstrapBudget,
    observed: EvalObservedCounters,
    err: Option<&str>,
) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::symbol(":selfhost/bootstrap-evidence"),
    );
    m.insert(TermOrdKey(Term::symbol(":v")), Term::Int(1.into()));
    m.insert(
        TermOrdKey(Term::symbol(":stage")),
        Term::Str(stage.to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":profile")),
        Term::symbol(if budget.profile == "production" {
            ":production"
        } else {
            ":parity-harness"
        }),
    );
    m.insert(
        TermOrdKey(Term::symbol(":limits")),
        bootstrap_limits_term(budget.mem_limits, budget.step_limit),
    );
    m.insert(
        TermOrdKey(Term::symbol(":observed")),
        bootstrap_observed_term(observed),
    );
    m.insert(
        TermOrdKey(Term::symbol(":result")),
        Term::symbol(if err.is_none() { ":ok" } else { ":error" }),
    );
    if let Some(msg) = err {
        m.insert(
            TermOrdKey(Term::symbol(":error")),
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":code")),
                        Term::symbol(":core/selfhost/bootstrap-resource-limit"),
                    ),
                    (
                        TermOrdKey(Term::symbol(":message")),
                        Term::Str(msg.to_string()),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );
    }
    Term::Map(m)
}

fn set_bootstrap_evidence_binding(env: &mut Env, evidence: Term) {
    *env = Env::with_binding(
        env,
        SELFHOST_BOOTSTRAP_EVIDENCE_SYMBOL,
        Value::data(evidence),
    );
}

fn with_trusted_bootstrap_limits<T, F>(
    ctx: &mut EvalCtx,
    env: &mut Env,
    stage: &str,
    f: F,
) -> anyhow::Result<T>
where
    F: FnOnce(&mut EvalCtx, &mut Env) -> anyhow::Result<T>,
{
    let budget = trusted_bootstrap_budget();
    let saved_step_limit = ctx.step_limit;
    let saved_mem_limits = ctx.mem_limits();
    ctx.step_limit = Some(budget.step_limit);
    ctx.set_mem_limits(budget.mem_limits);
    ctx.reset_counters();
    let out = f(ctx, env);
    let observed = ctx.observed_counters();
    let err_msg = out.as_ref().err().map(|e| e.to_string());
    set_bootstrap_evidence_binding(
        env,
        bootstrap_evidence_term(stage, budget, observed, err_msg.as_deref()),
    );
    ctx.step_limit = saved_step_limit;
    ctx.set_mem_limits(saved_mem_limits);
    ctx.reset_counters();
    out.map_err(|err| {
        anyhow::anyhow!(
            "selfhost bootstrap failed under bounded limits (stage={stage}, profile={}, step_limit={}, observed_steps={}, observed_allocated_units={}, observed_live_units={}, observed_max_live_units={}, observed_pair_cells={}, observed_max_vec_len={}, observed_max_map_len={}, observed_max_bytes_len={}, observed_max_string_len={}): {err}",
            budget.profile,
            budget.step_limit,
            observed.steps,
            observed.mem.allocated_units,
            observed.mem.live_units,
            observed.mem.max_live_units,
            observed.mem.pair_cells,
            observed.mem.max_vec_len,
            observed.mem.max_map_len,
            observed.mem.max_bytes_len,
            observed.mem.max_string_len
        )
    })
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Load the self-hosted CoreForm toolchain v1 from an artifact file.
///
/// Artifact schema (CoreForm map):
/// {
///   :kind "genesis/selfhost-toolchain-artifact-v0.2"
///   :v 1
///   :modules [
///     {
///       :path "selfhost/parse.gc"
///       :source "<module source>"
///       :forms [<TopForm> ...]          ; canonical module forms (required in production profile)
///       :module-h b"...32 bytes..."
///       :stage1-ok true
///       :stage2-supported bool
///       :stage2-ok bool
///     }
///   ]
/// }
///
/// Production bootstrap requires `:forms` for every module and does not parse `:source`.
/// Source-parse fallback is development-only (parity harness profile).
pub fn load_selfhost_coreform_toolchain_v1_from_artifact(
    ctx: &mut EvalCtx,
    env: &mut Env,
    artifact_path: &Path,
) -> anyhow::Result<()> {
    let src = std::fs::read_to_string(artifact_path)
        .with_context(|| format!("read {}", artifact_path.display()))?;
    load_selfhost_coreform_toolchain_v1_from_artifact_source(ctx, env, &src)
        .with_context(|| format!("decode {}", artifact_path.display()))
}

/// Load the self-hosted CoreForm toolchain v1 from artifact source text.
///
/// This is intended for hosts that do not expose filesystem access to the runtime (e.g. wasm-bindgen),
/// where the host can supply the artifact bytes/string directly.
pub fn load_selfhost_coreform_toolchain_v1_from_artifact_source(
    ctx: &mut EvalCtx,
    env: &mut Env,
    src: &str,
) -> anyhow::Result<()> {
    // Fast-path: reuse compiled modules for identical artifact bytes.
    let mut h = blake3::Hasher::new();
    h.update(HASH_DOMAIN_PREFIX);
    h.update(b"selfhost-artifact\0");
    h.update(src.as_bytes());
    let artifact_h: [u8; 32] = *h.finalize().as_bytes();

    if let Some(cache) = ARTIFACT_COMPILED_CACHE.get() {
        let cached = lock_artifact_compiled_cache(cache)?
            .get(&artifact_h)
            .cloned();
        if let Some(compiled) = cached {
            return with_trusted_bootstrap_limits(
                ctx,
                env,
                ARTIFACT_BOOTSTRAP_STAGE,
                |ctx, env| {
                    for (name, m) in &compiled {
                        eval_compiled_module(ctx, env, m)
                            .with_context(|| format!("eval {name}"))?;
                    }
                    Ok(())
                },
            );
        }
    }

    let manifest = toolchain_manifest()?;
    if let Some(compiled_in_order) = try_read_compiled_cache(artifact_h, manifest) {
        let out = with_trusted_bootstrap_limits(ctx, env, ARTIFACT_BOOTSTRAP_STAGE, |ctx, env| {
            for (path, module) in &compiled_in_order {
                eval_compiled_module(ctx, env, module).with_context(|| format!("eval {path}"))?;
            }
            for sym in &manifest.required_symbols {
                if env.get(sym).is_none() {
                    return Err(anyhow::anyhow!(
                        "compiled cache missing required manifest symbol: {sym}"
                    ));
                }
            }
            Ok(())
        });
        if out.is_ok() {
            let cache = ARTIFACT_COMPILED_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
            lock_artifact_compiled_cache(cache)?.insert(artifact_h, compiled_in_order);
            return out;
        }
    }

    let term = parse_term(src).map_err(|e| anyhow::anyhow!("artifact parse: {e}"))?;
    let root = match term {
        Term::Map(m) => m,
        _ => {
            return Err(anyhow::anyhow!(
                "artifact root must be a map, got {}",
                print_term(&term)
            ));
        }
    };

    let kind = match map_get(&root, ":kind") {
        Some(Term::Str(s)) => s.as_str(),
        _ => return Err(anyhow::anyhow!("artifact missing :kind string")),
    };
    if kind != SELFHOST_TOOLCHAIN_ARTIFACT_KIND {
        return Err(anyhow::anyhow!(
            "artifact :kind mismatch: expected {SELFHOST_TOOLCHAIN_ARTIFACT_KIND}, got {kind}"
        ));
    }

    let v = match map_get(&root, ":v") {
        Some(Term::Int(i)) => i,
        _ => return Err(anyhow::anyhow!("artifact missing :v int")),
    };
    if v != &SELFHOST_TOOLCHAIN_ARTIFACT_VERSION.into() {
        return Err(anyhow::anyhow!(
            "artifact :v must be {SELFHOST_TOOLCHAIN_ARTIFACT_VERSION}, got {v}"
        ));
    }

    let modules = match map_get(&root, ":modules") {
        Some(Term::Vector(v)) => v,
        _ => return Err(anyhow::anyhow!("artifact missing :modules vector")),
    };

    let expected_paths: BTreeSet<&str> = manifest.module_paths.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut compiled_by_path: BTreeMap<String, CompiledModule> = BTreeMap::new();

    for m in modules {
        let m_map = match m {
            Term::Map(mm) => mm,
            _ => return Err(anyhow::anyhow!("artifact module entry must be map")),
        };
        let path = match map_get(m_map, ":path") {
            Some(Term::Str(s)) => s.clone(),
            _ => return Err(anyhow::anyhow!("artifact module missing :path string")),
        };
        if !expected_paths.contains(path.as_str()) {
            return Err(anyhow::anyhow!(
                "artifact module path is not recognized: {path}"
            ));
        }
        if !seen.insert(path.clone()) {
            return Err(anyhow::anyhow!(
                "artifact contains duplicate module path: {path}"
            ));
        }

        let forms_from_artifact = match map_get(m_map, ":forms") {
            Some(Term::Vector(v)) => Some(v.clone()),
            Some(_) => {
                return Err(anyhow::anyhow!(
                    "artifact module {path} has invalid :forms (expected vector)"
                ));
            }
            None => None,
        };
        let src = match map_get(m_map, ":source") {
            Some(Term::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let module_h = match map_get(m_map, ":module-h") {
            Some(Term::Bytes(b)) if b.len() == 32 => {
                let mut h = [0u8; 32];
                h.copy_from_slice(b.as_ref());
                h
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "artifact module {path} missing :module-h 32-byte blob"
                ));
            }
        };
        let stage1_ok = matches!(map_get(m_map, ":stage1-ok"), Some(Term::Bool(true)));
        if !stage1_ok {
            return Err(anyhow::anyhow!(
                "artifact module {path} is missing stage1 validation"
            ));
        }
        let stage2_supported =
            matches!(map_get(m_map, ":stage2-supported"), Some(Term::Bool(true)));
        let stage2_ok = matches!(map_get(m_map, ":stage2-ok"), Some(Term::Bool(true)));
        if stage2_supported && !stage2_ok {
            return Err(anyhow::anyhow!(
                "artifact module {path} has failed stage2 validation"
            ));
        }

        let forms = if let Some(v) = &forms_from_artifact {
            v.clone()
        } else {
            if !non_artifact_bootstrap_modes_allowed() {
                return Err(anyhow::anyhow!(
                    "artifact module {path} missing :forms vector; production bootstrap forbids Rust source parse fallback"
                ));
            }
            let src = src.ok_or_else(|| {
                anyhow::anyhow!("artifact module {path} missing :source string or :forms vector")
            })?;
            parse_module(src).map_err(|e| anyhow::anyhow!("{path}: parse: {e}"))?
        };

        // Canonicalize always; toolchain identity is defined by canonical printed bytes.
        // If the artifact provides `:forms`, they must already be canonical (idempotent).
        let canon_forms = canonicalize_module(forms.clone())
            .map_err(|e| anyhow::anyhow!("{path}: canon: {e}"))?;
        if forms_from_artifact.is_some() && canon_forms != forms {
            return Err(anyhow::anyhow!(
                "artifact module {path} has non-canonical :forms; re-run `genesis selfhost-artifact`"
            ));
        }

        // Validate the module hash against the canonical printed bytes. This is the stable
        // content-addressed identity for toolchain modules in artifact-only mode.
        let got_h = hash_module(&canon_forms);
        if got_h != module_h {
            return Err(anyhow::anyhow!(
                "artifact module hash mismatch for {path}: expected {:x?}, computed {:x?}",
                module_h,
                got_h
            ));
        }

        let forms = canon_forms;
        let compiled =
            compile_module(&forms).map_err(|e| anyhow::anyhow!("{path}: compile: {e}"))?;
        compiled_by_path.insert(path, compiled);
    }

    for expected in &manifest.module_paths {
        if !seen.contains(expected) {
            return Err(anyhow::anyhow!(
                "artifact missing required module: {expected}"
            ));
        }
    }

    // Build a deterministic vector in manifest-declared order for caching and evaluation.
    let mut compiled_in_order: CachedCompiledModules =
        Vec::with_capacity(manifest.module_paths.len());
    for path in &manifest.module_paths {
        let module = compiled_by_path
            .remove(path)
            .ok_or_else(|| anyhow::anyhow!("artifact missing compiled module: {path}"))?;
        compiled_in_order.push((path.clone(), module));
    }

    let out = with_trusted_bootstrap_limits(ctx, env, ARTIFACT_BOOTSTRAP_STAGE, |ctx, env| {
        for (path, module) in &compiled_in_order {
            eval_compiled_module(ctx, env, module).with_context(|| format!("eval {path}"))?;
        }
        for sym in &manifest.required_symbols {
            if env.get(sym).is_none() {
                return Err(anyhow::anyhow!(
                    "artifact missing required manifest symbol: {sym}"
                ));
            }
        }
        Ok(())
    });

    if out.is_ok() {
        let cache = ARTIFACT_COMPILED_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
        lock_artifact_compiled_cache(cache)?.insert(artifact_h, compiled_in_order.clone());
        let _ = write_compiled_cache(artifact_h, &compiled_in_order);
    }
    out
}

fn load_selfhost_coreform_toolchain_v1_embedded(
    ctx: &mut EvalCtx,
    env: &mut Env,
) -> anyhow::Result<()> {
    #[cfg(feature = "embedded-bootstrap")]
    {
        let mods = SELFHOST_COREFORM_V1
            .as_ref()
            .map_err(|s| anyhow::anyhow!("selfhost toolchain init failed: {s}"))?;
        return with_trusted_bootstrap_limits(ctx, env, "embedded-modules", |ctx, env| {
            for (name, module) in mods {
                eval_compiled_module(ctx, env, module).with_context(|| format!("eval {name}"))?;
            }
            Ok(())
        });
    }

    #[cfg(not(feature = "embedded-bootstrap"))]
    {
        let _ = (ctx, env);
        Err(anyhow::anyhow!(
            "embedded selfhost bootstrap is disabled at compile time; rebuild with feature `gc_prelude/embedded-bootstrap`"
        ))
    }
}

fn resolve_default_artifact_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL)
}

pub fn load_selfhost_coreform_toolchain_v1_with_mode(
    ctx: &mut EvalCtx,
    env: &mut Env,
    mode: SelfhostBootstrapMode,
    artifact_path: Option<&Path>,
) -> anyhow::Result<()> {
    enforce_bootstrap_mode_allowed(mode)?;
    match mode {
        SelfhostBootstrapMode::Embedded => load_selfhost_coreform_toolchain_v1_embedded(ctx, env),
        SelfhostBootstrapMode::ArtifactOnly | SelfhostBootstrapMode::ArtifactPreferred => {
            let from_env = std::env::var(SELFHOST_TOOLCHAIN_ARTIFACT_ENV)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from);
            let resolved = artifact_path
                .map(PathBuf::from)
                .or(from_env)
                .unwrap_or_else(resolve_default_artifact_path);

            match load_selfhost_coreform_toolchain_v1_from_artifact(ctx, env, &resolved) {
                Ok(()) => Ok(()),
                Err(err) => match mode {
                    SelfhostBootstrapMode::ArtifactOnly => Err(anyhow::anyhow!(
                        "selfhost artifact bootstrap required, failed at {}: {err}",
                        resolved.display()
                    )),
                    SelfhostBootstrapMode::ArtifactPreferred => {
                        load_selfhost_coreform_toolchain_v1_embedded(ctx, env).with_context(|| {
                            format!(
                                "artifact bootstrap failed at {}, and embedded fallback failed",
                                resolved.display()
                            )
                        })
                    }
                    SelfhostBootstrapMode::Embedded => Err(anyhow::anyhow!(
                        "internal bootstrap mode drift while handling artifact fallback"
                    )),
                },
            }
        }
    }
}

/// Load the self-hosted CoreForm toolchain v1 into the current environment.
///
/// This is an opt-in cutover mechanism: we bootstrap by parsing the toolchain sources with the Rust
/// CoreForm frontend, but then run the toolchain logic inside the kernel.
pub fn load_selfhost_coreform_toolchain_v1(ctx: &mut EvalCtx, env: &mut Env) -> anyhow::Result<()> {
    load_selfhost_coreform_toolchain_v1_with_mode(
        ctx,
        env,
        SelfhostBootstrapMode::ArtifactOnly,
        None,
    )
}

#[cfg(test)]
#[path = "selfhost_coreform_v1_tests.rs"]
mod tests;
