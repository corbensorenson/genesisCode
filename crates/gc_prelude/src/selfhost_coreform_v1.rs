use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::Context;
#[cfg(feature = "embedded-bootstrap")]
use once_cell::sync::Lazy;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term};
use gc_kernel::{CompiledModule, Env, EvalCtx, compile_module, eval_compiled_module};

const SELFHOST_TOOLCHAIN_MANIFEST_SRC: &str =
    include_str!("../../../selfhost/toolchain_manifest.gc");
const SELFHOST_TOOLCHAIN_EMBEDDED_ARTIFACT_SRC: &str =
    include_str!("../../../selfhost/toolchain.gc");

const SELFHOST_TOOLCHAIN_ARTIFACT_ENV: &str = "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT";
const SELFHOST_TOOLCHAIN_ARTIFACT_KIND: &str = "genesis/selfhost-toolchain-artifact-v0.2";
const DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = ".genesis/selfhost/toolchain.gc";

#[derive(Debug, Clone)]
struct ToolchainManifest {
    module_paths: Vec<String>,
    required_symbols: Vec<String>,
}

#[cfg(feature = "embedded-bootstrap")]
type SelfhostCompiledModules = Vec<(String, CompiledModule)>;

type CachedCompiledModules = Vec<(String, CompiledModule)>;

// Per-process cache: compiling the selfhost toolchain modules dominates costs for obligations and
// other workflows that create fresh ctx/env pairs. The artifact bytes are content-addressed, so
// caching by a content hash is safe and deterministic.
static ARTIFACT_COMPILED_CACHE: OnceLock<Mutex<BTreeMap<[u8; 32], CachedCompiledModules>>> =
    OnceLock::new();

static TOOLCHAIN_MANIFEST: OnceLock<Result<ToolchainManifest, String>> = OnceLock::new();
static EMBEDDED_MODULE_SOURCES: OnceLock<Result<Vec<(String, String)>, String>> = OnceLock::new();

fn parse_module_paths_vec(t: &Term, field: &str) -> anyhow::Result<Vec<String>> {
    let Term::Vector(v) = t else {
        return Err(anyhow::anyhow!("{field} must be a vector"));
    };
    let mut out = Vec::with_capacity(v.len());
    let mut seen = BTreeSet::new();
    for item in v {
        let Term::Str(s) = item else {
            return Err(anyhow::anyhow!("{field} entries must be strings"));
        };
        if s.trim().is_empty() {
            return Err(anyhow::anyhow!("{field} cannot contain empty paths"));
        }
        if !seen.insert(s.clone()) {
            return Err(anyhow::anyhow!("{field} contains duplicate path: {s}"));
        }
        out.push(s.clone());
    }
    if out.is_empty() {
        return Err(anyhow::anyhow!("{field} must not be empty"));
    }
    Ok(out)
}

fn parse_required_symbols_vec(t: &Term, field: &str) -> anyhow::Result<Vec<String>> {
    let Term::Vector(v) = t else {
        return Err(anyhow::anyhow!("{field} must be a vector"));
    };
    let mut out = Vec::with_capacity(v.len());
    let mut seen = BTreeSet::new();
    for item in v {
        let sym = match item {
            Term::Symbol(s) => s.clone(),
            Term::Str(s) => s.clone(),
            _ => return Err(anyhow::anyhow!("{field} entries must be symbols or strings")),
        };
        if sym.trim().is_empty() {
            return Err(anyhow::anyhow!("{field} cannot contain empty symbol names"));
        }
        if !seen.insert(sym.clone()) {
            return Err(anyhow::anyhow!("{field} contains duplicate symbol: {sym}"));
        }
        out.push(sym);
    }
    Ok(out)
}

fn parse_toolchain_manifest_src(src: &str) -> anyhow::Result<ToolchainManifest> {
    let term = parse_term(src).map_err(|e| anyhow::anyhow!("manifest parse: {e}"))?;
    let root = match term {
        Term::Map(m) => m,
        _ => return Err(anyhow::anyhow!("manifest root must be a map")),
    };
    let kind = match map_get(&root, ":kind") {
        Some(Term::Str(s)) => s.as_str(),
        _ => return Err(anyhow::anyhow!("manifest missing :kind string")),
    };
    if kind != "genesis/selfhost-toolchain-manifest-v0.2" {
        return Err(anyhow::anyhow!(
            "manifest :kind mismatch: expected genesis/selfhost-toolchain-manifest-v0.2, got {kind}"
        ));
    }
    let v = match map_get(&root, ":v") {
        Some(Term::Int(i)) => i,
        _ => return Err(anyhow::anyhow!("manifest missing :v int")),
    };
    if v != &1.into() {
        return Err(anyhow::anyhow!("manifest :v must be 1, got {v}"));
    }
    let module_paths = parse_module_paths_vec(
        map_get(&root, ":module-paths")
            .ok_or_else(|| anyhow::anyhow!("manifest missing :module-paths"))?,
        ":module-paths",
    )?;
    let required_symbols = parse_required_symbols_vec(
        map_get(&root, ":required-symbols")
            .ok_or_else(|| anyhow::anyhow!("manifest missing :required-symbols"))?,
        ":required-symbols",
    )?;
    Ok(ToolchainManifest {
        module_paths,
        required_symbols,
    })
}

fn toolchain_manifest() -> anyhow::Result<&'static ToolchainManifest> {
    let r = TOOLCHAIN_MANIFEST
        .get_or_init(|| parse_toolchain_manifest_src(SELFHOST_TOOLCHAIN_MANIFEST_SRC).map_err(|e| e.to_string()));
    match r {
        Ok(m) => Ok(m),
        Err(e) => Err(anyhow::anyhow!("selfhost toolchain manifest: {e}")),
    }
}

fn parse_embedded_artifact_sources() -> anyhow::Result<BTreeMap<String, String>> {
    let term = parse_term(SELFHOST_TOOLCHAIN_EMBEDDED_ARTIFACT_SRC)
        .map_err(|e| anyhow::anyhow!("embedded toolchain artifact parse: {e}"))?;
    let root = match term {
        Term::Map(m) => m,
        _ => return Err(anyhow::anyhow!("embedded artifact root must be a map")),
    };
    let kind = match map_get(&root, ":kind") {
        Some(Term::Str(s)) => s.as_str(),
        _ => return Err(anyhow::anyhow!("embedded artifact missing :kind string")),
    };
    if kind != SELFHOST_TOOLCHAIN_ARTIFACT_KIND {
        return Err(anyhow::anyhow!(
            "embedded artifact :kind mismatch: expected {SELFHOST_TOOLCHAIN_ARTIFACT_KIND}, got {kind}"
        ));
    }
    let modules = match map_get(&root, ":modules") {
        Some(Term::Vector(v)) => v,
        _ => return Err(anyhow::anyhow!("embedded artifact missing :modules vector")),
    };
    let mut out = BTreeMap::new();
    for m in modules {
        let Term::Map(mm) = m else {
            return Err(anyhow::anyhow!("embedded artifact module entry must be map"));
        };
        let path = match map_get(mm, ":path") {
            Some(Term::Str(s)) => s.clone(),
            _ => return Err(anyhow::anyhow!("embedded artifact module missing :path string")),
        };
        let src = match map_get(mm, ":source") {
            Some(Term::Str(s)) => s.clone(),
            _ => return Err(anyhow::anyhow!("embedded artifact module {path} missing :source string")),
        };
        if out.insert(path.clone(), src).is_some() {
            return Err(anyhow::anyhow!(
                "embedded artifact module has duplicate path: {path}"
            ));
        }
    }
    Ok(out)
}

fn read_manifest_sources_from_workspace(manifest: &ToolchainManifest) -> anyhow::Result<Vec<(String, String)>> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::with_capacity(manifest.module_paths.len());
    for path in &manifest.module_paths {
        let full = repo_root.join(path);
        let src = std::fs::read_to_string(&full)
            .with_context(|| format!("read selfhost module {}", full.display()))?;
        out.push((path.clone(), src));
    }
    Ok(out)
}

#[cfg(feature = "embedded-bootstrap")]
static SELFHOST_COREFORM_V1: Lazy<Result<SelfhostCompiledModules, String>> = Lazy::new(|| {
    let mut out = Vec::new();
    for (name, src) in selfhost_coreform_toolchain_v1_sources() {
        let forms = parse_module(&src).map_err(|e| format!("{name}: parse: {e}"))?;
        let forms = canonicalize_module(forms).map_err(|e| format!("{name}: canon: {e}"))?;
        let compiled = compile_module(&forms).map_err(|e| format!("{name}: compile: {e}"))?;
        out.push((name, compiled));
    }
    Ok(out)
});

pub fn selfhost_coreform_toolchain_v1_sources() -> Vec<(String, String)> {
    let r = EMBEDDED_MODULE_SOURCES.get_or_init(|| {
        let manifest = toolchain_manifest().map_err(|e| e.to_string())?;
        if let Ok(sources) = read_manifest_sources_from_workspace(manifest) {
            return Ok(sources);
        }

        let mut embedded = parse_embedded_artifact_sources().map_err(|e| e.to_string())?;
        let mut ordered = Vec::with_capacity(manifest.module_paths.len());
        for path in &manifest.module_paths {
            let src = embedded.remove(path).ok_or_else(|| {
                format!("embedded artifact missing manifest module source: {path}")
            })?;
            ordered.push((path.clone(), src));
        }
        Ok(ordered)
    });
    match r {
        Ok(v) => v.clone(),
        Err(e) => panic!("selfhost toolchain embedded sources unavailable: {e}"),
    }
}

pub fn embedded_bootstrap_available() -> bool {
    cfg!(feature = "embedded-bootstrap")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfhostBootstrapMode {
    ArtifactOnly,
    ArtifactPreferred,
    Embedded,
}

fn map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

fn with_trusted_bootstrap_limits<T, F>(ctx: &mut EvalCtx, f: F) -> anyhow::Result<T>
where
    F: FnOnce(&mut EvalCtx) -> anyhow::Result<T>,
{
    let saved_step_limit = ctx.step_limit;
    let saved_mem_limits = ctx.mem_limits;
    ctx.step_limit = None;
    ctx.mem_limits = gc_kernel::MemLimits::default();
    let out = f(ctx);
    ctx.step_limit = saved_step_limit;
    ctx.mem_limits = saved_mem_limits;
    ctx.reset_counters();
    out
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
///       :forms [<TopForm> ...]          ; optional (preferred): canonical module forms
///       :module-h b"...32 bytes..."
///       :stage1-ok true
///       :stage2-supported bool
///       :stage2-ok bool
///     }
///   ]
/// }
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
    h.update(b"GCv0.2\0selfhost-artifact\0");
    h.update(src.as_bytes());
    let artifact_h: [u8; 32] = *h.finalize().as_bytes();

    if let Some(cache) = ARTIFACT_COMPILED_CACHE.get() {
        if let Some(compiled) = cache.lock().expect("artifact cache lock").get(&artifact_h) {
            return with_trusted_bootstrap_limits(ctx, |ctx| {
                for (name, m) in compiled {
                    eval_compiled_module(ctx, env, m).with_context(|| format!("eval {name}"))?;
                }
                Ok(())
            });
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
    if v != &1.into() {
        return Err(anyhow::anyhow!("artifact :v must be 1, got {v}"));
    }

    let modules = match map_get(&root, ":modules") {
        Some(Term::Vector(v)) => v,
        _ => return Err(anyhow::anyhow!("artifact missing :modules vector")),
    };

    let manifest = toolchain_manifest()?;
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
    let mut compiled_in_order: CachedCompiledModules = Vec::with_capacity(manifest.module_paths.len());
    for path in &manifest.module_paths {
        let module = compiled_by_path
            .remove(path)
            .ok_or_else(|| anyhow::anyhow!("artifact missing compiled module: {path}"))?;
        compiled_in_order.push((path.clone(), module));
    }

    let out = with_trusted_bootstrap_limits(ctx, |ctx| {
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
        cache
            .lock()
            .expect("artifact cache lock")
            .insert(artifact_h, compiled_in_order);
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
        return with_trusted_bootstrap_limits(ctx, |ctx| {
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
                    SelfhostBootstrapMode::Embedded => unreachable!(),
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
