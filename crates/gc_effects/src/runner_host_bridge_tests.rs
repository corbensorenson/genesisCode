use super::{BridgeError, HostBridgeRuntime, decode_bridge_stdout, runner_host_bridge_policy};
use crate::policy::{CapsPolicy, OpPolicy};
use gc_coreform::{Term, TermOrdKey};
#[cfg(not(target_os = "wasi"))]
use std::time::Instant;

#[cfg(not(target_os = "wasi"))]
thread_local! {
    static TEST_BRIDGE_RUNTIME: std::cell::RefCell<HostBridgeRuntime> =
        std::cell::RefCell::new(HostBridgeRuntime::default());
}

#[cfg(not(target_os = "wasi"))]
fn call_host_bridge(
    family: &str,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
) -> Result<Term, BridgeError> {
    TEST_BRIDGE_RUNTIME
        .with_borrow_mut(|runtime| super::call_host_bridge(runtime, family, op, payload, pol))
}

#[cfg(not(target_os = "wasi"))]
fn reset_test_bridge_runtime() {
    TEST_BRIDGE_RUNTIME.with_borrow_mut(|runtime| {
        *runtime = HostBridgeRuntime::default();
    });
}

#[test]
fn framed_response_decodes() {
    let body = "{:ok true :id \"x\"}";
    let out = format!("{}\n{}", body.len(), body);
    let term = decode_bridge_stdout("test", out.as_bytes(), None).expect("decode");
    let Term::Map(m) = term else {
        panic!("expected map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":id"))),
        Some(&Term::Str("x".to_string()))
    );
}

#[test]
fn response_limit_is_enforced() {
    let body = "{:ok true :id \"x\"}";
    let out = format!("{}\n{}", body.len(), body);
    let err = decode_bridge_stdout("test", out.as_bytes(), Some(4)).expect_err("must fail");
    assert_eq!(err.code, "test/bridge-response-too-large");
}

#[test]
fn forced_wasi_profile_supports_inline_response() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["gpu/compute::limits"]

[op."gpu/compute::limits"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :transport :wasi}"
"#,
    )
    .expect("caps");
    let resp = call_host_bridge(
        "gpu",
        "gpu/compute::limits",
        &Term::Map(
            [(
                TermOrdKey(Term::symbol(":payload")),
                Term::Str("x".to_string()),
            )]
            .into_iter()
            .collect(),
        ),
        policy.op_policy("gpu/compute::limits"),
    )
    .expect("wasi bridge");
    let Term::Map(mm) = resp else {
        panic!("map response expected");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":transport"))),
        Some(&Term::symbol(":wasi"))
    );
}

#[test]
fn forced_wasi_profile_reports_missing_profile_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["gpu/compute::limits"]

[op."gpu/compute::limits"]
wasi_bridge_profile = true
"#,
    )
    .expect("caps");
    let err = call_host_bridge(
        "gpu",
        "gpu/compute::limits",
        &Term::Nil,
        policy.op_policy("gpu/compute::limits"),
    )
    .expect_err("missing wasi profile data should fail");
    assert_eq!(err.code, "gpu/bridge-wasi-profile-required");
}

#[test]
fn normalize_sha256_hex_accepts_prefixed_and_plain_hex() {
    let raw = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd";
    assert_eq!(
        runner_host_bridge_policy::normalize_sha256_hex(raw),
        Some(raw.to_string())
    );
    assert_eq!(
        runner_host_bridge_policy::normalize_sha256_hex(&format!("sha256:{raw}")),
        Some(raw.to_string())
    );
    assert!(runner_host_bridge_policy::normalize_sha256_hex("sha256:not-a-hex").is_none());
    assert!(runner_host_bridge_policy::normalize_sha256_hex("abc").is_none());
}

#[cfg(all(not(target_os = "wasi"), unix))]
#[test]
fn successful_spawn_bridge_can_close_unused_stdin_without_a_transport_error() {
    use std::os::unix::fs::PermissionsExt;

    let td = tempfile::tempdir().expect("bridge tempdir");
    let bridge = td.path().join("close_stdin_bridge.sh");
    std::fs::write(
        &bridge,
        r#"#!/bin/sh
exec 0<&-
resp='{:ok true :stdin :closed}'
printf '%s\n%s' "${#resp}" "$resp"
"#,
    )
    .expect("write bridge");
    let mut permissions = std::fs::metadata(&bridge)
        .expect("bridge metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bridge, permissions).expect("bridge chmod");
    let payload = Term::Str("x".repeat(256 * 1024));

    for timeout in [None, Some(5_000_u64)] {
        let timeout_line = timeout
            .map(|value| format!("timeout_ms = {value}"))
            .unwrap_or_default();
        let policy = CapsPolicy::from_toml_str(&format!(
            r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{}"
bridge_cmd = "close_stdin_bridge.sh"
max_bytes = 524288
{}
"#,
            td.path().display(),
            timeout_line
        ))
        .expect("bridge policy");
        let response = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &payload,
            policy.op_policy("gpu/compute::limits"),
        )
        .expect("successful bridge response must win over an unused-stdin broken pipe");
        let Term::Map(response) = response else {
            panic!("bridge response must be a map");
        };
        assert_eq!(
            response.get(&TermOrdKey(Term::symbol(":stdin"))),
            Some(&Term::symbol(":closed"))
        );
    }
}

#[cfg(all(not(target_os = "wasi"), unix))]
#[test]
fn spawn_bridge_reaps_residual_descendants_after_success_and_error() {
    use std::os::unix::fs::PermissionsExt;

    let td = tempfile::tempdir().expect("bridge tempdir");
    let bridge = td.path().join("forking_bridge.sh");
    let pid_log = td.path().join("forking_bridge_pids.txt");
    std::fs::write(
        &bridge,
        r#"#!/usr/bin/env sh
set -eu
pid_file="$1"
mode="$2"
sleep 30 </dev/null >/dev/null 2>&1 &
descendant="$!"
echo "$$:$descendant" >> "$pid_file"
if [ "$mode" = "error" ]; then
  echo "injected bridge failure" >&2
  exit 42
fi
resp='{:ok true}'
printf '%s\n%s' "${#resp}" "$resp"
"#,
    )
    .expect("write forking bridge");
    let mut permissions = std::fs::metadata(&bridge)
        .expect("forking bridge metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bridge, permissions).expect("forking bridge chmod");

    let base_dir = td.path().display().to_string();
    let pid_log_s = pid_log.display().to_string();
    let mut runtime = HostBridgeRuntime::default();
    for mode in ["success", "error"] {
        let policy = CapsPolicy::from_toml_str(&format!(
            r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "forking_bridge.sh"
bridge_transport = "spawn-per-op"
bridge_args = ["{pid_log_s}", "{mode}"]
max_bytes = 4096
"#
        ))
        .expect("forking bridge policy");
        let result = super::call_host_bridge(
            &mut runtime,
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            policy.op_policy("gpu/compute::limits"),
        );
        if mode == "success" {
            result.expect("successful bridge response");
        } else {
            assert_eq!(
                result.expect_err("bridge error exit").code,
                "gpu/bridge-exit"
            );
        }
    }

    let pids = std::fs::read_to_string(&pid_log)
        .expect("forking bridge pid log")
        .lines()
        .flat_map(|line| line.split(':'))
        .map(|pid| pid.parse::<i32>().expect("forking bridge pid"))
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 4);
    for pid in pids {
        wait_for_pid_exit(pid);
    }
}

#[cfg(not(target_os = "wasi"))]
fn write_persistent_bridge_script(path: &std::path::Path) {
    let src = r#"#!/usr/bin/env sh
set -eu
op="$1"
startup_delay_ms=35
python3 - "$startup_delay_ms" <<'PY'
import sys, time
time.sleep(int(sys.argv[1]) / 1000.0)
PY
if [ "${GENESIS_HOST_BRIDGE_TRANSPORT:-}" = "persistent-stdio" ]; then
  persistent=1
else
  persistent=0
fi
while IFS= read -r req_len; do
  if [ -z "${req_len:-}" ]; then
    exit 0
  fi
  dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
  resp="{:ok true :kind :bridge-ok :op \"$op\"}"
  resp_len="$(printf '%s' "$resp" | wc -c | tr -d '[:space:]')"
  printf '%s\n%s' "$resp_len" "$resp"
  if [ "$persistent" != "1" ]; then
    exit 0
  fi
done
"#;
    std::fs::write(path, src).expect("write persistent bridge script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("bridge metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("bridge chmod");
    }
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn write_timeout_bridge_script(path: &std::path::Path) {
    let src = r#"#!/usr/bin/env sh
set -eu
pid_file="${1:-}"
mode="${2:-fast}"
op="${3:-unknown}"
if [ -n "$pid_file" ]; then
  :
fi
if [ "$mode" = "hang" ]; then
  sleep 30 &
  descendant="$!"
  echo "$$:$descendant" >> "$pid_file"
  wait "$descendant"
elif [ -n "$pid_file" ]; then
  echo "$$:0" >> "$pid_file"
fi
resp="{:ok true :mode \"$mode\" :op \"$op\"}"
resp_len="$(printf '%s' "$resp" | wc -c | tr -d '[:space:]')"
printf '%s\n%s' "$resp_len" "$resp"
"#;
    std::fs::write(path, src).expect("write timeout bridge script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("bridge metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("bridge chmod");
    }
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn write_persistent_timeout_bridge_script(path: &std::path::Path) {
    let src = r#"#!/usr/bin/env sh
set -eu
pid_file="$1"
op="$2"
while IFS= read -r req_len; do
  dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
  sleep 30 &
  descendant="$!"
  echo "$$:$descendant" >> "$pid_file"
  wait "$descendant"
  resp="{:ok true :op \"$op\"}"
  resp_len="$(printf '%s' "$resp" | wc -c | tr -d '[:space:]')"
  printf '%s\n%s' "$resp_len" "$resp"
done
"#;
    std::fs::write(path, src).expect("write persistent timeout bridge script");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("persistent timeout bridge metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("persistent timeout bridge chmod");
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn write_owned_persistent_bridge_script(path: &std::path::Path) {
    let src = r#"#!/usr/bin/env sh
set -eu
pid_file="$1"
mode="$2"
op="$3"
echo "${GENESIS_HOST_BRIDGE_FAMILY}:$$" >> "$pid_file"
while IFS= read -r req_len; do
  dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
  if [ "$mode" = "error" ]; then
    printf 'not-a-frame\n'
    sleep 30
  fi
  resp="{:ok true :op \"$op\"}"
  resp_len="$(printf '%s' "$resp" | wc -c | tr -d '[:space:]')"
  printf '%s\n%s' "$resp_len" "$resp"
done
"#;
    std::fs::write(path, src).expect("write owned persistent bridge script");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("owned persistent bridge metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("owned persistent bridge chmod");
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn pid_is_alive(pid: i32) -> bool {
    let rc = unsafe { libc::kill(pid, 0) };
    if rc != 0 {
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
        "bridge pid {pid} survived bounded cleanup"
    );
}

#[cfg(not(target_os = "wasi"))]
fn p95_ms(samples: &[u128]) -> u128 {
    let mut s = samples.to_vec();
    s.sort_unstable();
    let n = s.len();
    assert!(n > 0, "samples must be non-empty");
    let rank = (95 * n).div_ceil(100);
    s[rank.saturating_sub(1)]
}

#[cfg(not(target_os = "wasi"))]
#[test]
fn persistent_stdio_transport_reduces_bridge_p95_latency_vs_spawn_per_op() {
    reset_test_bridge_runtime();
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("persistent_bridge.sh");
    write_persistent_bridge_script(&bridge);
    let base_dir = td.path().display().to_string();

    let spawn_policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_bridge.sh"
bridge_transport = "spawn-per-op"
max_bytes = 4096
timeout_ms = 5000
"#
    ))
    .expect("spawn policy");

    let persistent_policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_bridge.sh"
bridge_transport = "persistent-stdio"
max_bytes = 4096
timeout_ms = 5000
"#
    ))
    .expect("persistent policy");

    let mut spawn_samples = Vec::new();
    for _ in 0..12 {
        let t0 = Instant::now();
        let _ = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            spawn_policy.op_policy("gpu/compute::limits"),
        )
        .expect("spawn transport call");
        spawn_samples.push(t0.elapsed().as_millis());
    }

    // Exclude connection establishment: this assertion measures the steady-state
    // transport benefit while cold-start latency is covered by separate budgets.
    let _ = call_host_bridge(
        "gpu",
        "gpu/compute::limits",
        &Term::Nil,
        persistent_policy.op_policy("gpu/compute::limits"),
    )
    .expect("persistent transport warmup");
    let mut persistent_samples = Vec::new();
    for _ in 0..12 {
        let t0 = Instant::now();
        let _ = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            persistent_policy.op_policy("gpu/compute::limits"),
        )
        .expect("persistent transport call");
        persistent_samples.push(t0.elapsed().as_millis());
    }

    let spawn_p95 = p95_ms(&spawn_samples);
    let persistent_p95 = p95_ms(&persistent_samples);
    assert!(
        persistent_p95 + 10 < spawn_p95,
        "expected persistent p95 ({persistent_p95}ms) to beat spawn-per-op p95 ({spawn_p95}ms)"
    );
    reset_test_bridge_runtime();
}

#[cfg(not(target_os = "wasi"))]
#[test]
fn rejects_invalid_bridge_transport_policy_value() {
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("persistent_bridge.sh");
    write_persistent_bridge_script(&bridge);
    let base_dir = td.path().display().to_string();
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_bridge.sh"
bridge_transport = "udp-magic"
max_bytes = 4096
"#
    ))
    .expect("parse caps policy");
    let err = call_host_bridge(
        "gpu",
        "gpu/compute::limits",
        &Term::Nil,
        policy.op_policy("gpu/compute::limits"),
    )
    .expect_err("invalid bridge_transport must fail");
    assert_eq!(err.code, "gpu/bridge-policy");
    assert!(err.message.contains("bridge_transport must be one of"));
}

#[cfg(all(not(target_os = "wasi"), unix))]
#[test]
#[ignore = "stress-gate"]
fn spawn_per_op_timeout_kills_bridge_processes_and_recovers() {
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("timeout_bridge.sh");
    write_timeout_bridge_script(&bridge);
    let base_dir = td.path().display().to_string();
    let pid_log = td.path().join("bridge_pids.txt");
    let pid_log_s = pid_log.display().to_string();

    let readiness_policy = format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "timeout_bridge.sh"
bridge_transport = "spawn-per-op"
bridge_args = ["{pid_log_s}", "hang"]
timeout_ms = 5000
max_bytes = 4096
"#
    );
    let readiness = std::thread::spawn(move || {
        let policy = CapsPolicy::from_toml_str(&readiness_policy).expect("readiness policy");
        let mut runtime = HostBridgeRuntime::default();
        super::call_host_bridge(
            &mut runtime,
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            policy.op_policy("gpu/compute::limits"),
        )
    });
    for _ in 0..800 {
        if std::fs::metadata(&pid_log).is_ok_and(|metadata| metadata.len() > 0) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        std::fs::metadata(&pid_log).is_ok_and(|metadata| metadata.len() > 0),
        "timeout bridge fixture did not reach descendant-ready state"
    );
    let readiness_error = readiness
        .join()
        .expect("readiness bridge call panicked")
        .expect_err("readiness bridge must timeout");
    assert_eq!(readiness_error.code, "gpu/bridge-timeout");

    let timeout_policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "timeout_bridge.sh"
bridge_transport = "spawn-per-op"
bridge_args = ["{pid_log_s}", "hang"]
timeout_ms = 25
max_bytes = 4096
"#
    ))
    .expect("timeout policy");

    let started = Instant::now();
    for _ in 0..32 {
        let err = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            timeout_policy.op_policy("gpu/compute::limits"),
        )
        .expect_err("hung bridge call must timeout");
        assert_eq!(err.code, "gpu/bridge-timeout", "{}", err.message);
        assert_eq!(
            super::active_bridge_io_pumps_for_tests(),
            0,
            "timeout returned before bridge I/O pumps quiesced"
        );
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "repeated hard cancellation exceeded the stress bound: {:?}",
        started.elapsed()
    );

    let pid_src = std::fs::read_to_string(&pid_log).expect("read bridge pid log");
    let pids: Vec<i32> = pid_src
        .lines()
        .flat_map(|line| line.trim().split(':'))
        .filter_map(|field| field.parse::<i32>().ok())
        .filter(|pid| *pid > 0)
        .collect();
    assert!(
        !pids.is_empty(),
        "expected timeout bridge script to log pids"
    );
    for pid in pids {
        for _ in 0..100 {
            if !pid_is_alive(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !pid_is_alive(pid),
            "timed-out bridge tree pid {pid} remained alive after timeout"
        );
    }

    let fast_policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "timeout_bridge.sh"
bridge_transport = "spawn-per-op"
bridge_args = ["{pid_log_s}", "fast"]
timeout_ms = 200
max_bytes = 4096
"#
    ))
    .expect("fast policy");
    let resp = call_host_bridge(
        "gpu",
        "gpu/compute::limits",
        &Term::Nil,
        fast_policy.op_policy("gpu/compute::limits"),
    )
    .expect("fast bridge call");
    let Term::Map(mm) = resp else {
        panic!("fast response should be map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":mode"))),
        Some(&Term::Str("fast".to_string()))
    );
    assert_eq!(super::active_bridge_io_pumps_for_tests(), 0);
}

#[cfg(all(not(target_os = "wasi"), unix))]
#[test]
fn persistent_bridge_owner_closes_all_families_on_error_drop_and_restart() {
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("owned_persistent_bridge.sh");
    write_owned_persistent_bridge_script(&bridge);
    let pid_log = td.path().join("owned_bridge_pids.txt");
    let base_dir = td.path().display().to_string();
    let pid_log_s = pid_log.display().to_string();
    let workers_before =
        super::runner_host_bridge_persistent::joined_persistent_bridge_workers_for_tests();

    let policy_for = |family: &str, mode: &str| {
        CapsPolicy::from_toml_str(&format!(
            r#"
allow = ["test/bridge::call"]
[op."test/bridge::call"]
base_dir = "{base_dir}"
bridge_cmd = "owned_persistent_bridge.sh"
bridge_transport = "persistent-stdio"
bridge_args = ["{pid_log_s}", "{mode}"]
timeout_ms = 1000
max_bytes = 4096
family_marker = "{family}"
"#
        ))
        .expect("owned bridge policy")
    };

    let mut runtime = HostBridgeRuntime::default();
    for family in ["net", "process", "gfx", "gpu", "model"] {
        let policy = policy_for(family, "success");
        super::call_host_bridge(
            &mut runtime,
            family,
            "test/bridge::call",
            &Term::Nil,
            policy.op_policy("test/bridge::call"),
        )
        .expect("persistent family call");
    }
    let error_policy = policy_for("plugin", "error");
    let error = super::call_host_bridge(
        &mut runtime,
        "plugin",
        "test/bridge::call",
        &Term::Nil,
        error_policy.op_policy("test/bridge::call"),
    )
    .expect_err("invalid frame must fail");
    assert_eq!(error.code, "plugin/bridge-parse");

    let before_drop = std::fs::read_to_string(&pid_log).expect("owned bridge pid log");
    let first_generation = before_drop
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(_, pid)| pid.parse::<i32>().expect("bridge pid"))
        .collect::<Vec<_>>();
    assert_eq!(first_generation.len(), 6);
    assert!(
        first_generation
            .iter()
            .take(5)
            .all(|pid| pid_is_alive(*pid))
    );
    wait_for_pid_exit(*first_generation.last().expect("error bridge pid"));

    drop(runtime);
    for pid in &first_generation {
        wait_for_pid_exit(*pid);
    }
    assert!(
        super::runner_host_bridge_persistent::joined_persistent_bridge_workers_for_tests()
            >= workers_before + first_generation.len(),
        "runtime drop returned before all persistent workers joined"
    );

    let mut restarted = HostBridgeRuntime::default();
    let restart_policy = policy_for("model", "success");
    super::call_host_bridge(
        &mut restarted,
        "model",
        "test/bridge::call",
        &Term::Nil,
        restart_policy.op_policy("test/bridge::call"),
    )
    .expect("bridge call after runtime restart");
    drop(restarted);
    let all_pids = std::fs::read_to_string(&pid_log).expect("restart pid log");
    let restarted_pid = all_pids
        .lines()
        .last()
        .and_then(|line| line.split_once(':'))
        .and_then(|(_, pid)| pid.parse::<i32>().ok())
        .expect("restarted bridge pid");
    assert!(!first_generation.contains(&restarted_pid));
    wait_for_pid_exit(restarted_pid);
}

#[cfg(all(not(target_os = "wasi"), unix))]
#[test]
#[ignore = "stress-gate"]
fn persistent_stdio_timeout_kills_process_trees_and_workers() {
    reset_test_bridge_runtime();
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("persistent_timeout_bridge.sh");
    write_persistent_timeout_bridge_script(&bridge);
    let base_dir = td.path().display().to_string();
    let pid_log = td.path().join("persistent_bridge_pids.txt");
    let pid_log_s = pid_log.display().to_string();
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_timeout_bridge.sh"
bridge_transport = "persistent-stdio"
bridge_args = ["{pid_log_s}"]
timeout_ms = 200
max_bytes = 4096
"#
    ))
    .expect("policy");
    let started = Instant::now();
    let joined_before =
        super::runner_host_bridge_persistent::joined_persistent_bridge_workers_for_tests();
    for iteration in 0..16 {
        let error = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            policy.op_policy("gpu/compute::limits"),
        )
        .expect_err("hung persistent request must timeout");
        assert_eq!(error.code, "gpu/bridge-timeout");
        assert!(
            super::runner_host_bridge_persistent::joined_persistent_bridge_workers_for_tests()
                > joined_before + iteration,
            "persistent timeout returned before joining worker {iteration}"
        );
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5));

    let pids = std::fs::read_to_string(&pid_log)
        .expect("persistent pid log")
        .lines()
        .flat_map(|line| line.split(':'))
        .filter_map(|field| field.parse::<i32>().ok())
        .collect::<Vec<_>>();
    assert!(
        pids.len() >= 24 && pids.len() % 2 == 0,
        "stress must observe leader/descendant pairs for most sessions; got {} pids",
        pids.len()
    );
    for pid in pids {
        for _ in 0..100 {
            if !pid_is_alive(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            !pid_is_alive(pid),
            "persistent bridge tree pid {pid} survived"
        );
    }

    let healthy = td.path().join("persistent_bridge.sh");
    write_persistent_bridge_script(&healthy);
    let healthy_policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "persistent_bridge.sh"
bridge_transport = "persistent-stdio"
timeout_ms = 1000
max_bytes = 4096
"#
    ))
    .expect("healthy policy");
    call_host_bridge(
        "gpu",
        "gpu/compute::limits",
        &Term::Nil,
        healthy_policy.op_policy("gpu/compute::limits"),
    )
    .expect("healthy persistent request after timeout stress");
    reset_test_bridge_runtime();
    assert!(
        super::runner_host_bridge_persistent::joined_persistent_bridge_workers_for_tests()
            >= joined_before + 17
    );
}
