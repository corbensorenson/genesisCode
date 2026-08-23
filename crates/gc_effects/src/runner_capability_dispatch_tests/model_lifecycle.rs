use super::*;

#[cfg(all(not(target_os = "wasi"), unix))]
use crate::runner_host_bridge::HostBridgeRuntime;
#[cfg(all(not(target_os = "wasi"), unix))]
use sha2::{Digest as _, Sha256};

#[cfg(all(not(target_os = "wasi"), unix))]
const MODEL_PLUGIN: &str = "genesis.agent-model-runner.v0.1";

#[cfg(all(not(target_os = "wasi"), unix))]
fn write_model_provider_bridge(path: &std::path::Path) {
    let source = r#"#!/usr/bin/env sh
set -eu
pid_file="$1"
mode="$2"
op="$3"
echo "$mode:$$" >> "$pid_file"
while IFS= read -r req_len; do
  if [ -z "${req_len:-}" ]; then
    exit 0
  fi
  dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
  if [ "$mode" = "error" ]; then
    printf 'not-a-frame\n'
    sleep 30
  elif [ "$mode" = "timeout" ]; then
    sleep 30 &
    descendant="$!"
    echo "timeout-child:$descendant" >> "$pid_file"
    wait "$descendant"
  else
    resp="{:ok true :provider-session :model-runner :op \"$op\"}"
    resp_len="$(printf '%s' "$resp" | wc -c | tr -d '[:space:]')"
    printf '%s\n%s' "$resp_len" "$resp"
  fi
done
"#;
    std::fs::write(path, source).expect("write model provider bridge");
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = std::fs::metadata(path)
        .expect("model provider bridge metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("model provider bridge chmod");
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn sha256_file(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).expect("read model provider bridge");
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn model_policy(
    base_dir: &std::path::Path,
    bridge_digest: &str,
    pid_file: &std::path::Path,
    mode: &str,
    timeout_ms: u64,
) -> CapsPolicy {
    CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["{MODEL_PLUGIN}"]
allow_commands = ["infer"]
base_dir = "{}"
bridge_cmd = "model_provider_bridge.sh"
bridge_cmd_sha256 = "sha256:{bridge_digest}"
bridge_transport = "persistent-stdio"
bridge_args = ["{}", "{mode}"]
timeout_ms = {timeout_ms}
max_bytes = 4096
"#,
        base_dir.display(),
        pid_file.display(),
    ))
    .expect("model provider policy")
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn model_payload() -> Term {
    term_map([
        (Term::symbol(":plugin"), Term::Str(MODEL_PLUGIN.to_string())),
        (Term::symbol(":command"), Term::Str("infer".to_string())),
        (
            Term::symbol(":payload"),
            term_map([
                (
                    Term::symbol(":model-id"),
                    Term::Str("fixture/model".to_string()),
                ),
                (Term::symbol(":request-sha256"), Term::Str("0".repeat(64))),
            ]),
        ),
    ])
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn call_model_provider(runtime: &mut HostBridgeRuntime, policy: &CapsPolicy) -> Value {
    let mut budget = ArtifactBudgetState::default();
    super::super::call_capability_with_runtime(
        "host/plugin::command",
        &model_payload(),
        policy.op_policy("host/plugin::command"),
        policy,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &mut budget,
        None,
        runtime,
        SealId(107),
    )
    .expect("model provider capability dispatch")
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn pid_is_alive(pid: i32) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    if result != 0 {
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    }
    let state = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    !state.is_some_and(|state| state.starts_with('Z'))
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn wait_for_pid_exit(pid: i32) {
    for _ in 0..200 {
        if !pid_is_alive(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        !pid_is_alive(pid),
        "model provider pid {pid} survived cleanup"
    );
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn read_mode_pids(path: &std::path::Path, mode: &str) -> Vec<i32> {
    std::fs::read_to_string(path)
        .expect("model provider pid log")
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(observed_mode, _)| *observed_mode == mode)
        .map(|(_, pid)| pid.parse::<i32>().expect("model provider pid"))
        .collect()
}

#[cfg(all(not(target_os = "wasi"), unix))]
#[test]
fn model_runner_plugin_session_is_owned_reaped_and_restart_isolated() {
    let temp = tempfile::tempdir().expect("model provider tempdir");
    let bridge = temp.path().join("model_provider_bridge.sh");
    let pid_file = temp.path().join("model_provider_pids.txt");
    write_model_provider_bridge(&bridge);
    let digest = sha256_file(&bridge);

    let success_policy = model_policy(temp.path(), &digest, &pid_file, "success", 1000);
    let mut first_runtime = HostBridgeRuntime::default();
    for _ in 0..2 {
        let response = call_model_provider(&mut first_runtime, &success_policy);
        let Some(Term::Map(response)) = response.as_data() else {
            panic!("expected model provider response map");
        };
        assert_eq!(
            response.get(&TermOrdKey(Term::symbol(":provider-session"))),
            Some(&Term::symbol(":model-runner"))
        );
    }
    let first_generation = read_mode_pids(&pid_file, "success");
    assert_eq!(
        first_generation.len(),
        1,
        "one runner must reuse exactly one provider process"
    );
    assert!(pid_is_alive(first_generation[0]));
    drop(first_runtime);
    wait_for_pid_exit(first_generation[0]);

    let mut restarted_runtime = HostBridgeRuntime::default();
    let _ = call_model_provider(&mut restarted_runtime, &success_policy);
    let generations = read_mode_pids(&pid_file, "success");
    assert_eq!(generations.len(), 2);
    assert_ne!(generations[0], generations[1]);
    drop(restarted_runtime);
    wait_for_pid_exit(generations[1]);

    let error_policy = model_policy(temp.path(), &digest, &pid_file, "error", 1000);
    let mut error_runtime = HostBridgeRuntime::default();
    let error = call_model_provider(&mut error_runtime, &error_policy);
    assert_eq!(code_from_error(error), "host/plugin/bridge-parse");
    let error_pid = read_mode_pids(&pid_file, "error")
        .into_iter()
        .last()
        .expect("error provider pid");
    wait_for_pid_exit(error_pid);
    drop(error_runtime);

    let timeout_policy = model_policy(temp.path(), &digest, &pid_file, "timeout", 100);
    let mut timeout_runtime = HostBridgeRuntime::default();
    let timeout = call_model_provider(&mut timeout_runtime, &timeout_policy);
    assert_eq!(code_from_error(timeout), "host/plugin/bridge-timeout");
    drop(timeout_runtime);
    for pid in read_mode_pids(&pid_file, "timeout")
        .into_iter()
        .chain(read_mode_pids(&pid_file, "timeout-child"))
    {
        wait_for_pid_exit(pid);
    }
}
