use super::{call_host_bridge, decode_bridge_stdout, runner_host_bridge_policy};
use crate::policy::CapsPolicy;
use gc_coreform::{Term, TermOrdKey};
#[cfg(not(target_os = "wasi"))]
use std::time::Instant;

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
  echo "$$" >> "$pid_file"
fi
if [ "$mode" = "hang" ]; then
  sleep 5
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
fn pid_is_alive(pid: i32) -> bool {
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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
    super::reset_persistent_bridge_sessions_for_tests();
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
    super::reset_persistent_bridge_sessions_for_tests();
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
fn spawn_per_op_timeout_kills_bridge_processes_and_recovers() {
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("timeout_bridge.sh");
    write_timeout_bridge_script(&bridge);
    let base_dir = td.path().display().to_string();
    let pid_log = td.path().join("bridge_pids.txt");
    let pid_log_s = pid_log.display().to_string();

    let timeout_policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gpu/compute::limits"]
[op."gpu/compute::limits"]
base_dir = "{base_dir}"
bridge_cmd = "timeout_bridge.sh"
bridge_transport = "spawn-per-op"
bridge_args = ["{pid_log_s}", "hang"]
timeout_ms = 100
max_bytes = 4096
"#
    ))
    .expect("timeout policy");

    for _ in 0..8 {
        let err = call_host_bridge(
            "gpu",
            "gpu/compute::limits",
            &Term::Nil,
            timeout_policy.op_policy("gpu/compute::limits"),
        )
        .expect_err("hung bridge call must timeout");
        assert_eq!(err.code, "gpu/bridge-timeout");
    }

    let pid_src = std::fs::read_to_string(&pid_log).expect("read bridge pid log");
    let pids: Vec<i32> = pid_src
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
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
            "timed-out bridge pid {pid} remained alive after timeout"
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
}

#[cfg(not(target_os = "wasi"))]
#[test]
fn persistent_stdio_transport_rejects_timeout_policy() {
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
bridge_transport = "persistent-stdio"
timeout_ms = 25
max_bytes = 4096
"#
    ))
    .expect("policy");
    let err = call_host_bridge(
        "gpu",
        "gpu/compute::limits",
        &Term::Nil,
        policy.op_policy("gpu/compute::limits"),
    )
    .expect_err("persistent timeout policy must fail closed");
    assert_eq!(err.code, "gpu/bridge-policy");
    assert!(err.message.contains("timeout_ms is not supported"));
}
