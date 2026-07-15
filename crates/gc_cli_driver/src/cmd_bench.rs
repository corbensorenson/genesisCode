use super::*;
use std::process::Command;

const KIND_BENCH: &str = "genesis/bench-v0.1";
const DRIVER_REL: &str = "scripts/lib/genesisbench_front_door.py";

fn resolve_repo_root() -> Result<PathBuf, CliError> {
    let cwd = std::env::current_dir().map_err(|error| {
        cli_err(
            EX_IO,
            "bench/current-directory",
            format!("failed to resolve current directory: {error}"),
        )
    })?;
    for start in [cwd, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")] {
        for candidate in start.ancestors() {
            if candidate.join(DRIVER_REL).is_file()
                && candidate
                    .join("benchmarks/agent_tasks/v0.1/suite.json")
                    .is_file()
            {
                return Ok(candidate.to_path_buf());
            }
        }
    }
    Err(cli_err(
        EX_IO,
        "bench/authority-root-missing",
        "GenesisBench authorities are unavailable; run this command from a GenesisCode source tree",
    ))
}

fn push_path(args: &mut Vec<String>, flag: &str, path: &Path) {
    args.push(flag.to_string());
    args.push(path.as_os_str().to_string_lossy().into_owned());
}

fn runtime_paths(cli: &Cli, context: &str) -> Result<(PathBuf, PathBuf), CliError> {
    let genesis_bin = std::env::current_exe().map_err(|error| {
        cli_err(
            EX_IO,
            "bench/executable-unavailable",
            format!("{context}: failed to resolve genesis executable: {error}"),
        )
    })?;
    let artifact = resolved_selfhost_artifact_for_frontend(cli).ok_or_else(|| {
        cli_err(
            EX_IO,
            "bench/selfhost-artifact-required",
            format!(
                "{context}: pass --selfhost-artifact <file> or provide .genesis/selfhost/toolchain.gc"
            ),
        )
    })?;
    Ok((genesis_bin, artifact))
}

fn driver_args(cli: &Cli, cmd: &BenchCmd) -> Result<Vec<String>, CliError> {
    let mut args = Vec::new();
    match cmd {
        BenchCmd::Inspect { case, adapter } => {
            args.push("inspect".to_string());
            if let Some(case) = case {
                args.extend(["--case".to_string(), case.clone()]);
            }
            if let Some(adapter) = adapter {
                push_path(&mut args, "--adapter", adapter);
            }
        }
        BenchCmd::Run {
            case,
            adapter,
            out,
            adapter_executable,
            model_artifact,
            ablation,
        } => {
            args.extend(["run".to_string(), "--case".to_string(), case.clone()]);
            push_path(&mut args, "--adapter", adapter);
            push_path(&mut args, "--out", out);
            args.extend(["--ablation".to_string(), ablation.clone()]);
            if let Some(executable) = adapter_executable {
                push_path(&mut args, "--adapter-executable", executable);
            }
            if let Some(model_artifact) = model_artifact {
                push_path(&mut args, "--model-artifact", model_artifact);
            }
            let (genesis_bin, artifact) = runtime_paths(cli, "bench run")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::ValidateRun { run } => {
            args.push("validate-run".to_string());
            push_path(&mut args, "--run", run);
        }
        BenchCmd::Score {
            case,
            candidate,
            out,
        } => {
            args.extend(["score".to_string(), "--case".to_string(), case.clone()]);
            push_path(&mut args, "--candidate", candidate);
            if let Some(out) = out {
                push_path(&mut args, "--out", out);
            }
            let (genesis_bin, artifact) = runtime_paths(cli, "bench score")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::Replay { run } => {
            args.push("replay".to_string());
            push_path(&mut args, "--run", run);
            let (genesis_bin, artifact) = runtime_paths(cli, "bench replay")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::Bundle { run, out } => {
            args.push("bundle".to_string());
            push_path(&mut args, "--run", run);
            push_path(&mut args, "--out", out);
        }
        BenchCmd::Submit {
            bundle,
            outbox,
            submitter,
        } => {
            args.push("submit".to_string());
            push_path(&mut args, "--bundle", bundle);
            push_path(&mut args, "--outbox", outbox);
            args.extend(["--submitter".to_string(), submitter.clone()]);
        }
    }
    Ok(args)
}

pub(super) fn cmd_bench(cli: &Cli, cmd: &BenchCmd) -> Result<CmdOut, CliError> {
    let root = resolve_repo_root()?;
    let driver = root.join(DRIVER_REL);
    if driver.is_symlink() || !driver.is_file() {
        return Err(cli_err(
            EX_IO,
            "bench/driver-invalid",
            format!(
                "benchmark front-door driver is not a regular file: {}",
                driver.display()
            ),
        ));
    }
    let args = driver_args(cli, cmd)?;
    let output = Command::new("python3")
        .arg(&driver)
        .args(&args)
        .current_dir(&root)
        .output()
        .map_err(|error| {
            cli_err(
                EX_IO,
                "bench/driver-spawn",
                format!("failed to execute canonical benchmark front door: {error}"),
            )
        })?;
    if output.stdout.len() > 16 * 1024 * 1024 || output.stderr.len() > 16 * 1024 * 1024 {
        return Err(cli_err(
            EX_VERIFY,
            "bench/output-limit",
            "benchmark front door exceeded the 16 MiB command output ceiling",
        ));
    }
    if !output.status.success() {
        let message = serde_json::from_slice::<serde_json::Value>(&output.stderr)
            .ok()
            .and_then(|value| {
                value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_string());
        return Err(cli_err(
            EX_VERIFY,
            "bench/front-door-failed",
            if message.is_empty() {
                format!("benchmark front door exited with {}", output.status)
            } else {
                message
            },
        ));
    }
    let data: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        cli_err(
            EX_VERIFY,
            "bench/output-invalid",
            format!("benchmark front door emitted invalid JSON: {error}"),
        )
    })?;
    let json = json_envelope_value(JsonEnvelope {
        ok: true,
        kind: KIND_BENCH,
        data: Some(data),
        error: None,
    })?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}
