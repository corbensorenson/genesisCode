use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::session_resources::{SessionResourceLimits, SessionResourceOptions};
use crate::warm_protocol::WARM_PROTOCOL_V02;

pub(super) struct WarmConfig {
    pub(super) prime_selfhost: bool,
    pub(super) max_queue: usize,
    pub(super) max_frame_bytes: usize,
    pub(super) max_workspaces: usize,
    pub(super) workspace_idle: Duration,
    pub(super) max_requests: u64,
    pub(super) workspace_root: PathBuf,
    pub(super) resources: SessionResourceLimits,
}

pub(super) struct WarmOptions<'a> {
    pub(super) prime_selfhost: bool,
    pub(super) max_queue: usize,
    pub(super) max_frame_bytes: usize,
    pub(super) max_workspaces: usize,
    pub(super) workspace_idle_ms: u64,
    pub(super) max_requests: u64,
    pub(super) workspace_root: &'a Path,
    pub(super) resources: SessionResourceOptions,
}

pub(super) fn inherited_global_args(cli: &Cli, resources: &SessionResourceLimits) -> Vec<String> {
    let mut out = vec!["--json".to_string()];
    let step_limit = cli
        .step_limit
        .unwrap_or(gc_kernel::DEFAULT_STEP_LIMIT)
        .min(resources.max_steps);
    out.extend(["--step-limit".to_string(), step_limit.to_string()]);
    for (flag, value) in [
        ("--max-pair-cells", cli.max_pair_cells),
        ("--max-vec-len", cli.max_vec_len),
        ("--max-map-len", cli.max_map_len),
        ("--max-bytes-len", cli.max_bytes_len),
        ("--max-string-len", cli.max_string_len),
    ] {
        if let Some(value) = value {
            out.extend([flag.to_string(), value.to_string()]);
        }
    }
    if let Some(path) = &cli.selfhost_artifact {
        out.extend([
            "--selfhost-artifact".to_string(),
            path.display().to_string(),
        ]);
    }
    out.extend([
        "--selfhost-bootstrap".to_string(),
        cli.selfhost_bootstrap.as_str().to_string(),
    ]);
    if cli.selfhost_only {
        out.push("--selfhost-only".to_string());
    }
    if let Some(frontend) = cli.coreform_frontend {
        out.extend([
            "--coreform-frontend".to_string(),
            frontend.as_str().to_string(),
        ]);
    }
    out.extend([
        "--session-max-effects".to_string(),
        resources.max_effects.to_string(),
    ]);
    out
}

fn flavor_token(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Native => "native",
        Flavor::Wasi => "wasi",
    }
}

pub(super) fn warm_session_cache_key(
    cli: &Cli,
    flavor: Flavor,
    config: &WarmConfig,
    inherited: &[String],
) -> String {
    let payload = json!({
        "kind": "genesis/warm-cache-key-v0.2",
        "protocol": WARM_PROTOCOL_V02,
        "flavor": flavor_token(flavor),
        "prime_selfhost": config.prime_selfhost,
        "selfhost_only": cli.selfhost_only,
        "selfhost_bootstrap": cli.selfhost_bootstrap.as_str(),
        "coreform_frontend": cli.coreform_frontend.map(|value| value.as_str()),
        "inherited": inherited,
        "limits": {
            "max_queue": config.max_queue,
            "max_frame_bytes": config.max_frame_bytes,
            "max_workspaces": config.max_workspaces,
            "workspace_idle_ms": config.workspace_idle.as_millis(),
            "max_requests": config.max_requests,
            "resources": config.resources.as_json(),
        }
    });
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"GCv0.2\0warm-cache-key-v0.2\0");
    hasher.update(json_canonical_string(&payload).as_bytes());
    hasher.finalize().to_hex().to_string()
}

pub(super) fn prime_runtime(cli: &Cli, enabled: bool) -> Result<(), CliError> {
    if enabled {
        let frontend = resolved_coreform_frontend(cli)?;
        if matches!(frontend, gc_obligations::CoreformFrontend::Selfhost(_)) {
            let mut context = mk_ctx(cli);
            let prelude = build_prelude(&mut context);
            let mut environment = prelude.env;
            load_runtime_selfhost_toolchain(cli, &mut context, &mut environment)?;
        }
    }
    Ok(())
}
