use anyhow::Context;
use once_cell::sync::Lazy;

use gc_coreform::{canonicalize_module, parse_module};
use gc_kernel::{CompiledModule, Env, EvalCtx, compile_module, eval_compiled_module};

const PARSE_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../selfhost/parse.gc"
));
const CANON_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../selfhost/canon.gc"
));
const PRINTER_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../selfhost/printer.gc"
));
const HASH_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../selfhost/hash.gc"
));
const TOOL_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../selfhost/tool_coreform_v1.gc"
));

type SelfhostCompiledModules = Vec<(&'static str, CompiledModule)>;

static SELFHOST_COREFORM_V1: Lazy<Result<SelfhostCompiledModules, String>> = Lazy::new(|| {
    let mut out = Vec::new();
    for (name, src) in [
        ("selfhost/parse.gc", PARSE_SRC),
        ("selfhost/canon.gc", CANON_SRC),
        ("selfhost/printer.gc", PRINTER_SRC),
        ("selfhost/hash.gc", HASH_SRC),
        ("selfhost/tool_coreform_v1.gc", TOOL_SRC),
    ] {
        let forms = parse_module(src).map_err(|e| format!("{name}: parse: {e}"))?;
        let forms = canonicalize_module(forms).map_err(|e| format!("{name}: canon: {e}"))?;
        let compiled = compile_module(&forms).map_err(|e| format!("{name}: compile: {e}"))?;
        out.push((name, compiled));
    }
    Ok(out)
});

/// Load the self-hosted CoreForm toolchain v1 into the current environment.
///
/// This is an opt-in cutover mechanism: we bootstrap by parsing the toolchain sources with the Rust
/// CoreForm frontend, but then run the toolchain logic inside the kernel.
pub fn load_selfhost_coreform_toolchain_v1(ctx: &mut EvalCtx, env: &mut Env) -> anyhow::Result<()> {
    let mods = SELFHOST_COREFORM_V1
        .as_ref()
        .map_err(|s| anyhow::anyhow!("selfhost toolchain init failed: {s}"))?;

    // Toolchain bootstrap must not consume user step/memory budgets.
    let saved_step_limit = ctx.step_limit;
    let saved_mem_limits = ctx.mem_limits;
    ctx.step_limit = None;
    ctx.mem_limits = gc_kernel::MemLimits::default();

    for (name, module) in mods {
        eval_compiled_module(ctx, env, module).with_context(|| format!("eval {name}"))?;
    }

    ctx.step_limit = saved_step_limit;
    ctx.mem_limits = saved_mem_limits;
    ctx.reset_counters();
    Ok(())
}
