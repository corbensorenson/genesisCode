use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gc_coreform::{canonicalize_module, hash_module, parse_module};
use gc_effects::{CapsPolicy, run};
use gc_kernel::{EvalCtx, compile_module, eval_compiled_module};
use gc_prelude::build_prelude;

use crate::config::BenchConfig;
use crate::measure::best_of;

pub(crate) const GPU_COMPUTE_BACKEND_FALLBACK: &str = "deterministic-fallback";
pub(crate) const GPU_COMPUTE_BACKEND_DEVICE: &str = "device-bridge";

fn toml_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

fn write_compute_fallback_bridge(path: &Path) -> Result<()> {
    let script = r#"#!/usr/bin/env sh
set -eu
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
checksum="$(python3 - "$req_len" <<'PY'
import sys
n = int(sys.argv[1])
acc = 0
for i in range(200000 + n):
    acc = (acc * 1664525 + 1013904223 + i) & 0xFFFFFFFF
print(acc)
PY
)"
resp="{:ok true :kind \"gpu-compute-submit\" :backend \"deterministic-fallback\" :checksum $checksum}"
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

fn resolve_device_bridge_cmd() -> Option<PathBuf> {
    let raw = std::env::var("GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn compute_bridge_policy(tmp_dir: &Path) -> Result<(CapsPolicy, String)> {
    let allow = "allow = [\"gpu/compute::submit\"]\n";
    if let Some(device_cmd) = resolve_device_bridge_cmd() {
        let cmd_name = device_cmd
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid device bridge command path"))?;
        let base_dir = device_cmd.parent().unwrap_or(tmp_dir);
        let policy = CapsPolicy::from_toml_str(&format!(
            "{allow}\n[op.\"gpu/compute::submit\"]\nbase_dir = \"{}\"\nbridge_cmd = \"{}\"\nmax_bytes = 65536\n",
            toml_escape(&base_dir.display().to_string()),
            toml_escape(cmd_name),
        ))
        .context("parse gpu compute device bridge policy")?;
        return Ok((policy, GPU_COMPUTE_BACKEND_DEVICE.to_string()));
    }

    let bridge_path = tmp_dir.join("compute_bridge.sh");
    write_compute_fallback_bridge(&bridge_path)?;
    let policy = CapsPolicy::from_toml_str(&format!(
        "{allow}\n[op.\"gpu/compute::submit\"]\nbase_dir = \"{}\"\nbridge_cmd = \"{}\"\nmax_bytes = 65536\n",
        toml_escape(&tmp_dir.display().to_string()),
        "compute_bridge.sh",
    ))
    .context("parse gpu compute fallback bridge policy")?;
    Ok((policy, GPU_COMPUTE_BACKEND_FALLBACK.to_string()))
}

pub fn run_gpu_compute_submit(cfg: &BenchConfig) -> Result<(u128, String)> {
    let src = r#"
(def bench/prog
  (core/effect::perform
    'gpu/compute::submit
    {:graph {:passes [{:dispatch {:x 128 :y 1 :z 1}
                       :kernel "bench/matmul"
                       :bindings [{:buffer "a"} {:buffer "b"} {:buffer "out"}]}]}}
    (fn (x) (core/effect::pure x))))
bench/prog
"#;
    let forms =
        canonicalize_module(parse_module(src).context("parse gpu compute benchmark module")?)
            .context("canonicalize gpu compute benchmark module")?;
    let compiled = compile_module(&forms).context("compile gpu compute benchmark module")?;
    let program_hash = hash_module(&forms);

    let tmp = tempfile::tempdir().context("create gpu compute benchmark tempdir")?;
    let (policy, backend) = compute_bridge_policy(tmp.path())?;

    let elapsed_ms = best_of(cfg.warmups, cfg.repeats, || {
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let program = eval_compiled_module(&mut ctx, &mut env, &compiled)
            .context("eval gpu compute benchmark module")?;
        let _ = run(
            &mut ctx,
            &policy,
            program,
            program_hash,
            "runtime-bench".to_string(),
        )
        .context("run gpu compute benchmark effect program")?;
        Ok(())
    })?;

    Ok((elapsed_ms, backend))
}
