use std::path::Path;

use anyhow::{Context, Result};
use gc_coreform::{canonicalize_module, hash_module, parse_module};
use gc_effects::{CapsPolicy, run};
use gc_kernel::{EvalCtx, compile_module, eval_compiled_module};
use gc_prelude::build_prelude;

use crate::config::BenchConfig;
use crate::measure::best_of;

fn toml_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

fn write_bridge_script(path: &Path) -> Result<()> {
    let script = r#"#!/usr/bin/env sh
set -eu
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
resp='{:ok true :kind "gpu-compute-bridge"}'
resp_len="$(printf '%s' "$resp" | wc -c | tr -d '[:space:]')"
printf '%s\n%s' "$resp_len" "$resp"
"#;
    std::fs::write(path, script).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .with_context(|| format!("metadata {}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod +x {}", path.display()))?;
    }
    Ok(())
}

pub fn run_bridge_runner(cfg: &BenchConfig) -> Result<u128> {
    let src = "(def bench/prog (core/gpu/compute::limits nil))\nbench/prog\n";
    let forms = canonicalize_module(parse_module(src).context("parse bridge benchmark module")?)
        .context("canonicalize bridge benchmark module")?;
    let compiled = compile_module(&forms).context("compile bridge benchmark module")?;
    let program_hash = hash_module(&forms);

    let tmp = tempfile::tempdir().context("create bridge benchmark tempdir")?;
    let bridge_path = tmp.path().join("bridge.sh");
    write_bridge_script(&bridge_path)?;

    let policy = CapsPolicy::from_toml_str(&format!(
        "allow = [\"gpu/compute::limits\"]\n\n[op.\"gpu/compute::limits\"]\nbase_dir = \"{}\"\nbridge_cmd = \"bridge.sh\"\nmax_bytes = 4096\n",
        toml_escape(&tmp.path().display().to_string())
    ))
    .context("parse bridge benchmark policy")?;

    best_of(cfg.warmups, cfg.repeats, || {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let program = eval_compiled_module(&mut ctx, &mut env, &compiled)
            .context("eval bridge benchmark module")?;
        let _ = run(
            &mut ctx,
            &policy,
            program,
            program_hash,
            "runtime-bench".to_string(),
        )
        .context("run bridge benchmark effect program")?;
        Ok(())
    })
}

pub fn run_task_runner(cfg: &BenchConfig) -> Result<u128> {
    let src = r#"
(def bench/prog
  ((core/effect::bind
     (((core/task::spawn "bench-scope") "bench-task")
       {
         :task/program
         (core/task::program
           [
             (core/task::step/set 1)
             (core/task::step/int-add 2)
             (core/task::step/return 3)
           ])
       }))
    (fn (spawn-resp)
      (core/task::await ((core/map::get spawn-resp) (quote :task-id))))))
bench/prog
"#;

    let forms = canonicalize_module(parse_module(src).context("parse task benchmark module")?)
        .context("canonicalize task benchmark module")?;
    let compiled = compile_module(&forms).context("compile task benchmark module")?;
    let program_hash = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str("allow = [\"core/task::spawn\", \"core/task::await\"]")
        .context("parse task benchmark policy")?;

    best_of(cfg.warmups, cfg.repeats, || {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let program = eval_compiled_module(&mut ctx, &mut env, &compiled)
            .context("eval task benchmark module")?;
        let _ = run(
            &mut ctx,
            &policy,
            program,
            program_hash,
            "runtime-bench".to_string(),
        )
        .context("run task benchmark effect program")?;
        Ok(())
    })
}
