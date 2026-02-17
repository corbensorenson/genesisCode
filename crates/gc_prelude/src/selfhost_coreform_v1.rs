use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
#[cfg(feature = "embedded-bootstrap")]
use once_cell::sync::Lazy;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, parse_module, parse_term, print_term,
};
use gc_kernel::{CompiledModule, Env, EvalCtx, compile_module, eval_compiled_module};

const PARSE_SRC: &str = include_str!("../../../selfhost/parse.gc");
const CANON_SRC: &str = include_str!("../../../selfhost/canon.gc");
const PRINTER_SRC: &str = include_str!("../../../selfhost/printer.gc");
const HASH_SRC: &str = include_str!("../../../selfhost/hash.gc");
const TOOL_SRC: &str = include_str!("../../../selfhost/tool_coreform_v1.gc");
const CLI_TOOL_SRC: &str = include_str!("../../../selfhost/cli_coreform_v1.gc");

const SELFHOST_TOOLCHAIN_ARTIFACT_ENV: &str = "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT";
const SELFHOST_TOOLCHAIN_ARTIFACT_KIND: &str = "genesis/selfhost-toolchain-artifact-v0.2";
const DEFAULT_SELFHOST_TOOLCHAIN_ARTIFACT_REL: &str = ".genesis/selfhost/toolchain.gc";

const MODULE_SOURCES: [(&str, &str); 6] = [
    ("selfhost/parse.gc", PARSE_SRC),
    ("selfhost/canon.gc", CANON_SRC),
    ("selfhost/printer.gc", PRINTER_SRC),
    ("selfhost/hash.gc", HASH_SRC),
    ("selfhost/tool_coreform_v1.gc", TOOL_SRC),
    ("selfhost/cli_coreform_v1.gc", CLI_TOOL_SRC),
];

#[cfg(feature = "embedded-bootstrap")]
type SelfhostCompiledModules = Vec<(&'static str, CompiledModule)>;

#[cfg(feature = "embedded-bootstrap")]
static SELFHOST_COREFORM_V1: Lazy<Result<SelfhostCompiledModules, String>> = Lazy::new(|| {
    let mut out = Vec::new();
    for (name, src) in MODULE_SOURCES {
        let forms = parse_module(src).map_err(|e| format!("{name}: parse: {e}"))?;
        let forms = canonicalize_module(forms).map_err(|e| format!("{name}: canon: {e}"))?;
        let compiled = compile_module(&forms).map_err(|e| format!("{name}: compile: {e}"))?;
        out.push((name, compiled));
    }
    Ok(out)
});

pub fn selfhost_coreform_toolchain_v1_sources() -> &'static [(&'static str, &'static str)] {
    &MODULE_SOURCES
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

    let expected_paths: BTreeSet<&str> = MODULE_SOURCES.iter().map(|(p, _)| *p).collect();
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

        let forms = if let Some(v) = forms_from_artifact {
            v
        } else {
            let src = src.ok_or_else(|| {
                anyhow::anyhow!("artifact module {path} missing :source string or :forms vector")
            })?;
            let forms = parse_module(src).map_err(|e| anyhow::anyhow!("{path}: parse: {e}"))?;
            let forms =
                canonicalize_module(forms).map_err(|e| anyhow::anyhow!("{path}: canon: {e}"))?;
            let got_h = hash_module(&forms);
            if got_h != module_h {
                return Err(anyhow::anyhow!(
                    "artifact module hash mismatch for {path}: expected {:x?}, computed {:x?}",
                    module_h,
                    got_h
                ));
            }
            forms
        };
        let compiled =
            compile_module(&forms).map_err(|e| anyhow::anyhow!("{path}: compile: {e}"))?;
        compiled_by_path.insert(path, compiled);
    }

    for expected in expected_paths {
        if !seen.contains(expected) {
            return Err(anyhow::anyhow!(
                "artifact missing required module: {expected}"
            ));
        }
    }

    with_trusted_bootstrap_limits(ctx, |ctx| {
        for (path, _) in MODULE_SOURCES {
            let module = compiled_by_path
                .remove(path)
                .ok_or_else(|| anyhow::anyhow!("artifact missing compiled module: {path}"))?;
            eval_compiled_module(ctx, env, &module).with_context(|| format!("eval {path}"))?;
        }
        Ok(())
    })
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
