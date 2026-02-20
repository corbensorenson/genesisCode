use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use gc_coreform::{canonicalize_module, hash_module, parse_module};
use gc_effects::{CapsPolicy, run};
use gc_kernel::{EvalCtx, compile_module, eval_compiled_module};
use gc_prelude::build_prelude;

use crate::config::BenchConfig;
use crate::device_bridge::inrepo_device_bridge_spec;
use crate::measure::best_of;

pub(crate) const GPU_COMPUTE_BACKEND_FALLBACK: &str = "deterministic-fallback";
pub(crate) const GPU_COMPUTE_BACKEND_DEVICE: &str = "device-bridge";
pub(crate) const GPU_COMPUTE_BACKEND_POLICY_DEV_ALLOW_FALLBACK: &str = "dev-allow-fallback";
pub(crate) const GPU_COMPUTE_BACKEND_POLICY_REQUIRE_DEVICE: &str = "require-device";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuComputeBackendPolicy {
    DevAllowFallback,
    RequireDevice,
}

impl GpuComputeBackendPolicy {
    fn from_env(raw: &str) -> Result<Self> {
        match raw.trim() {
            "" | GPU_COMPUTE_BACKEND_POLICY_DEV_ALLOW_FALLBACK => Ok(Self::DevAllowFallback),
            GPU_COMPUTE_BACKEND_POLICY_REQUIRE_DEVICE => Ok(Self::RequireDevice),
            other => bail!(
                "invalid GENESIS_GPU_COMPUTE_BACKEND_POLICY `{other}` (expected {GPU_COMPUTE_BACKEND_POLICY_DEV_ALLOW_FALLBACK}|{GPU_COMPUTE_BACKEND_POLICY_REQUIRE_DEVICE})"
            ),
        }
    }
}

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

fn resolve_device_bridge_cmd_from(raw: Option<&str>) -> Option<PathBuf> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn resolve_device_bridge_cmd() -> Option<PathBuf> {
    resolve_device_bridge_cmd_from(
        std::env::var("GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD")
            .ok()
            .as_deref(),
    )
}

fn encode_bridge_policy(
    base_dir: &Path,
    cmd_name: &str,
    bridge_args: &[String],
) -> Result<CapsPolicy> {
    let allow = "allow = [\"gpu/compute::submit\"]\n";
    let bridge_args_toml = if bridge_args.is_empty() {
        String::new()
    } else {
        format!(
            "bridge_args = [{}]\n",
            bridge_args
                .iter()
                .map(|a| format!("\"{}\"", toml_escape(a)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    CapsPolicy::from_toml_str(&format!(
        "{allow}\n[op.\"gpu/compute::submit\"]\nbase_dir = \"{}\"\nbridge_cmd = \"{}\"\n{bridge_args_toml}max_bytes = 65536\n",
        toml_escape(&base_dir.display().to_string()),
        toml_escape(cmd_name),
    ))
    .context("parse gpu compute bridge policy")
}

fn compute_bridge_policy_with_override(
    tmp_dir: &Path,
    device_cmd_override: Option<PathBuf>,
    backend_policy: GpuComputeBackendPolicy,
) -> Result<(CapsPolicy, String)> {
    if let Some(device_cmd) = device_cmd_override.or_else(resolve_device_bridge_cmd) {
        let cmd_name = device_cmd
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid device bridge command path"))?;
        let base_dir = device_cmd.parent().unwrap_or(tmp_dir);
        let policy = encode_bridge_policy(base_dir, cmd_name, &[])?;
        return Ok((policy, GPU_COMPUTE_BACKEND_DEVICE.to_string()));
    }

    if let Some(spec) = inrepo_device_bridge_spec()? {
        let policy = encode_bridge_policy(&spec.base_dir, &spec.cmd_name, &spec.args)?;
        return Ok((policy, GPU_COMPUTE_BACKEND_DEVICE.to_string()));
    }

    if backend_policy == GpuComputeBackendPolicy::RequireDevice {
        bail!(
            "device-grade gpu compute backend is required by GENESIS_GPU_COMPUTE_BACKEND_POLICY=require-device; configure GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD or build gc_runtime_bench with --features device-bridge"
        );
    }

    let bridge_path = tmp_dir.join("compute_bridge.sh");
    write_compute_fallback_bridge(&bridge_path)?;
    let policy = encode_bridge_policy(tmp_dir, "compute_bridge.sh", &[])?;
    Ok((policy, GPU_COMPUTE_BACKEND_FALLBACK.to_string()))
}

fn compute_bridge_policy(
    tmp_dir: &Path,
    backend_policy: GpuComputeBackendPolicy,
) -> Result<(CapsPolicy, String)> {
    compute_bridge_policy_with_override(tmp_dir, None, backend_policy)
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
    let backend_policy = GpuComputeBackendPolicy::from_env(&cfg.gpu_compute_backend_policy)?;
    let (policy, backend) = compute_bridge_policy(tmp.path(), backend_policy)?;

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

#[cfg(test)]
mod tests {
    use super::{
        GPU_COMPUTE_BACKEND_DEVICE, GPU_COMPUTE_BACKEND_FALLBACK,
        GPU_COMPUTE_BACKEND_POLICY_DEV_ALLOW_FALLBACK, GPU_COMPUTE_BACKEND_POLICY_REQUIRE_DEVICE,
        GpuComputeBackendPolicy, compute_bridge_policy_with_override,
        resolve_device_bridge_cmd_from,
    };

    #[test]
    fn gpu_compute_parses_explicit_device_bridge_env_override() {
        let td = tempfile::tempdir().expect("tempdir");
        let fake_cmd = td.path().join("device_bridge_fake.sh");
        let parsed = resolve_device_bridge_cmd_from(fake_cmd.to_str());
        assert_eq!(parsed.as_deref(), Some(fake_cmd.as_path()));
    }

    #[cfg(not(feature = "device-bridge"))]
    #[test]
    fn gpu_compute_uses_fallback_backend_without_device_bridge_feature_or_env() {
        let td = tempfile::tempdir().expect("tempdir");
        let (_, backend) = compute_bridge_policy_with_override(
            td.path(),
            None,
            GpuComputeBackendPolicy::DevAllowFallback,
        )
        .expect("policy");
        assert_eq!(backend, GPU_COMPUTE_BACKEND_FALLBACK);
    }

    #[test]
    fn gpu_compute_prefers_explicit_device_bridge_override() {
        let td = tempfile::tempdir().expect("tempdir");
        let fake_cmd = td.path().join("device_bridge_fake.sh");
        let (_, backend) = compute_bridge_policy_with_override(
            td.path(),
            Some(fake_cmd),
            GpuComputeBackendPolicy::DevAllowFallback,
        )
        .expect("policy");
        assert_eq!(backend, GPU_COMPUTE_BACKEND_DEVICE);
    }

    #[cfg(feature = "device-bridge")]
    #[test]
    fn gpu_compute_uses_inrepo_device_bridge_when_feature_is_enabled() {
        let td = tempfile::tempdir().expect("tempdir");
        let (_, backend) = compute_bridge_policy_with_override(
            td.path(),
            None,
            GpuComputeBackendPolicy::RequireDevice,
        )
        .expect("policy");
        assert_eq!(backend, GPU_COMPUTE_BACKEND_DEVICE);
    }

    #[cfg(not(feature = "device-bridge"))]
    #[test]
    fn gpu_compute_require_device_policy_rejects_fallback_path() {
        let td = tempfile::tempdir().expect("tempdir");
        let err = compute_bridge_policy_with_override(
            td.path(),
            None,
            GpuComputeBackendPolicy::RequireDevice,
        )
        .expect_err("require-device should fail without device backend");
        assert!(
            err.to_string()
                .contains("device-grade gpu compute backend is required"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn gpu_compute_backend_policy_parser_accepts_supported_values() {
        assert_eq!(
            GpuComputeBackendPolicy::from_env(GPU_COMPUTE_BACKEND_POLICY_DEV_ALLOW_FALLBACK)
                .expect("parse dev policy"),
            GpuComputeBackendPolicy::DevAllowFallback
        );
        assert_eq!(
            GpuComputeBackendPolicy::from_env(GPU_COMPUTE_BACKEND_POLICY_REQUIRE_DEVICE)
                .expect("parse require policy"),
            GpuComputeBackendPolicy::RequireDevice
        );
        assert!(
            GpuComputeBackendPolicy::from_env("unknown-policy").is_err(),
            "unknown policy should fail"
        );
    }
}
