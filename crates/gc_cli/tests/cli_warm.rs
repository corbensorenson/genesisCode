use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value as JsonValue, json};
use std::fs;
use std::path::{Path, PathBuf};

const PROTOCOL: &str = "genesis/warm-protocol-v0.2";
const RESPONSE: &str = "genesis/warm-response-v0.2";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn repo_toolchain_artifact() -> PathBuf {
    repo_root().join("selfhost").join("toolchain.gc")
}

fn cmd() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("genesis");
    cmd.env(
        "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
        repo_toolchain_artifact(),
    );
    cmd
}

fn initialize(id: &str) -> JsonValue {
    json!({
        "protocol": PROTOCOL,
        "id": id,
        "method": "initialize",
        "client": {"name": "gc-cli-integration", "version": "0.1.0"}
    })
}

fn execute(id: &str, workspace_id: &str, root: &str, argv: &[&str]) -> JsonValue {
    json!({
        "protocol": PROTOCOL,
        "id": id,
        "method": "execute",
        "workspace": {"id": workspace_id, "root": root},
        "argv": argv
    })
}

fn control(id: &str, method: &str) -> JsonValue {
    json!({"protocol": PROTOCOL, "id": id, "method": method})
}

fn run_warm(base: &Path, frames: &[JsonValue], options: &[&str]) -> Vec<JsonValue> {
    let input = frames
        .iter()
        .map(JsonValue::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let output = cmd()
        .current_dir(base)
        .arg("warm")
        .args(options)
        .arg("--workspace-root")
        .arg(".")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("warm output must be UTF-8");
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("invalid warm response `{line}`: {error}"))
        })
        .collect()
}

fn responses<'a>(output: &'a [JsonValue], id: &str) -> Vec<&'a JsonValue> {
    output
        .iter()
        .filter(|response| response["id"] == id)
        .collect()
}

fn terminal<'a>(output: &'a [JsonValue], id: &str) -> &'a JsonValue {
    responses(output, id)
        .into_iter()
        .find(|response| response["status"] != "accepted")
        .unwrap_or_else(|| panic!("missing terminal response for {id}: {output:?}"))
}

fn terminal_audit(response: &JsonValue) -> &JsonValue {
    response
        .pointer("/data/audit")
        .or_else(|| response.pointer("/error/details/audit"))
        .unwrap_or_else(|| panic!("terminal response has no session audit: {response}"))
}

fn assert_native_audit(response: &JsonValue, workspace: &Path) {
    let audit = terminal_audit(response);
    assert_eq!(audit["kind"], "genesis/agent-session-audit-v0.1");
    assert_eq!(audit["worker_profile"], "native-isolated-v0.1");
    let identity = audit["limits_identity"].as_str().expect("limits identity");
    assert_eq!(identity.len(), 64);
    assert!(identity.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(audit["observed"]["peak_processes"].as_u64().is_some());
    assert!(audit["observed"]["peak_heap_bytes"].as_u64().is_some());
    assert!(
        !response
            .to_string()
            .contains(&workspace.display().to_string()),
        "session provenance must not leak the absolute workspace path"
    );
}

fn assert_closed_response_shape(response: &JsonValue) {
    let mut fields = response
        .as_object()
        .expect("response must be an object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "data", "error", "id", "kind", "meta", "ok", "protocol", "status"
        ]
    );
    assert_eq!(response["kind"], RESPONSE);
    assert_eq!(response["protocol"], PROTOCOL);
}

#[test]
fn warm_v02_matches_cold_json_and_isolates_workspaces() {
    let td = tempfile::tempdir().unwrap();
    let ws_a = td.path().join("ws-a");
    let ws_b = td.path().join("ws-b");
    fs::create_dir_all(&ws_a).unwrap();
    fs::create_dir_all(&ws_b).unwrap();
    fs::write(ws_a.join("prog.gc"), "(prim int/add 2 3)\n").unwrap();
    fs::write(ws_b.join("prog.gc"), "(prim int/add 9 4)\n").unwrap();

    let cold_a: JsonValue = serde_json::from_slice(
        &cmd()
            .current_dir(&ws_a)
            .args(["--json", "eval", "prog.gc"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    let cold_b: JsonValue = serde_json::from_slice(
        &cmd()
            .current_dir(&ws_b)
            .args(["--json", "eval", "prog.gc"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();

    let output = run_warm(
        td.path(),
        &[
            initialize("init"),
            execute("eval-a", "a", "ws-a", &["--json", "eval", "prog.gc"]),
            execute("eval-b", "b", "ws-b", &["--json", "eval", "prog.gc"]),
            control("stop", "shutdown"),
        ],
        &[],
    );

    for response in &output {
        assert_closed_response_shape(response);
        let key = response["meta"]["session_cache_key"]
            .as_str()
            .expect("cache key");
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert_eq!(responses(&output, "eval-a").len(), 2);
    assert_eq!(responses(&output, "eval-b").len(), 2);
    assert_eq!(terminal(&output, "eval-a")["data"]["result"], cold_a);
    assert_eq!(terminal(&output, "eval-b")["data"]["result"], cold_b);
    assert_eq!(terminal(&output, "stop")["status"], "draining");
}

#[test]
fn warm_v02_rejects_uninitialized_duplicate_unknown_and_nested_frames() {
    let td = tempfile::tempdir().unwrap();
    fs::create_dir(td.path().join("ws")).unwrap();
    fs::create_dir(td.path().join("ws-2")).unwrap();
    fs::write(td.path().join("ws/quick.gc"), "(prim int/add 1 1)\n").unwrap();
    let absolute = td.path().join("outside.gc").display().to_string();
    let unknown = json!({
        "protocol": PROTOCOL, "id": "unknown", "method": "ping", "extra": true
    });
    let absolute_path = execute(
        "absolute",
        "absolute-ws",
        "ws",
        &["--json", "eval", &absolute],
    );
    let output = run_warm(
        td.path(),
        &[
            control("early", "ping"),
            initialize("init"),
            control("early", "ping"),
            unknown,
            execute("nested", "ws", "ws", &["warm"]),
            execute("bind", "shared", "ws", &["--json", "eval", "quick.gc"]),
            execute("rebind", "shared", "ws-2", &["--json", "eval", "quick.gc"]),
            execute("parent", "parent", "../", &["--json", "eval", "quick.gc"]),
            absolute_path,
            control("stop", "shutdown"),
        ],
        &[],
    );

    assert_eq!(
        terminal(&output, "early")["error"]["code"],
        "warm/not-initialized"
    );
    assert_eq!(
        responses(&output, "early")[1]["error"]["code"],
        "warm/duplicate-id"
    );
    assert_eq!(
        terminal(&output, "unknown")["error"]["code"],
        "warm/frame-fields"
    );
    assert_eq!(terminal(&output, "nested")["error"]["code"], "warm/nested");
    assert_eq!(terminal(&output, "bind")["status"], "completed");
    assert_eq!(
        terminal(&output, "rebind")["error"]["code"],
        "warm/workspace-rebind"
    );
    assert_eq!(
        terminal(&output, "parent")["error"]["code"],
        "warm/workspace-root"
    );
    assert_eq!(
        terminal(&output, "absolute")["error"]["code"],
        "warm/workspace-path"
    );
}

#[test]
fn warm_v02_recovers_after_an_oversized_frame() {
    let td = tempfile::tempdir().unwrap();
    let oversized = format!("{{\"padding\":\"{}\"}}", "x".repeat(300));
    let input = format!(
        "{oversized}\n{}\n{}\n",
        initialize("init"),
        control("stop", "shutdown")
    );
    let output = cmd()
        .current_dir(td.path())
        .args(["warm", "--max-frame-bytes", "256", "--workspace-root", "."])
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let responses = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<JsonValue>(line).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(responses[0]["error"]["code"], "warm/frame-too-large");
    assert_eq!(terminal(&responses, "init")["status"], "initialized");
    assert_eq!(terminal(&responses, "stop")["status"], "draining");
}

#[test]
fn warm_v02_counts_rejected_transport_frames_toward_the_session_limit() {
    let td = tempfile::tempdir().unwrap();
    let oversized = format!("{{\"padding\":\"{}\"}}", "x".repeat(300));
    let input = format!("{oversized}\n{}\n", initialize("too-late"));
    let output = cmd()
        .current_dir(td.path())
        .args([
            "warm",
            "--max-frame-bytes",
            "256",
            "--max-requests",
            "1",
            "--workspace-root",
            ".",
        ])
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let responses = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<JsonValue>(line).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"]["code"], "warm/frame-too-large");
    assert_eq!(responses[1]["error"]["code"], "warm/session-limit");
    assert!(responses[1]["id"].is_null());
}

fn write_slow_program(root: &Path) {
    let source = fs::read_to_string(repo_root().join("benchmarks/roadmap/v0.1/pb1_fib_25.gc"))
        .unwrap()
        .replace("(bench/fib 25)", "(bench/fib 20)");
    fs::write(root.join("slow.gc"), source).unwrap();
    fs::write(root.join("quick.gc"), "(prim int/add 1 1)\n").unwrap();
}

#[test]
fn warm_v02_bounds_the_queue_and_cancels_queued_work() {
    let td = tempfile::tempdir().unwrap();
    let ws = td.path().join("ws");
    fs::create_dir(&ws).unwrap();
    write_slow_program(&ws);
    let cancel = json!({
        "protocol": PROTOCOL, "id": "cancel-q", "method": "cancel", "target_id": "queued"
    });
    let output = run_warm(
        td.path(),
        &[
            initialize("init"),
            execute("slow", "ws", "ws", &["--json", "eval", "slow.gc"]),
            execute("queued", "ws", "ws", &["--json", "eval", "quick.gc"]),
            execute("overflow", "ws", "ws", &["--json", "eval", "quick.gc"]),
            cancel,
            control("stop", "shutdown"),
        ],
        &["--max-queue", "1"],
    );

    assert_eq!(
        terminal(&output, "overflow")["error"]["code"],
        "warm/queue-full"
    );
    assert_eq!(
        terminal(&output, "queued")["error"]["code"],
        "warm/cancelled"
    );
    assert_eq!(terminal(&output, "cancel-q")["status"], "cancelled");
    assert_eq!(terminal(&output, "slow")["status"], "completed");
}

#[test]
fn warm_v02_suppresses_running_results_after_cancel_or_deadline() {
    let td = tempfile::tempdir().unwrap();
    let ws = td.path().join("ws");
    fs::create_dir(&ws).unwrap();
    write_slow_program(&ws);
    let cancel = json!({
        "protocol": PROTOCOL, "id": "cancel-run", "method": "cancel", "target_id": "cancelled"
    });
    let deadline = json!({
        "protocol": PROTOCOL,
        "id": "deadline",
        "method": "execute",
        "workspace": {"id": "ws", "root": "ws"},
        "argv": ["--json", "eval", "slow.gc"],
        "deadline_ms": 1
    });
    let output = run_warm(
        td.path(),
        &[
            initialize("init"),
            execute("cancelled", "ws", "ws", &["--json", "eval", "slow.gc"]),
            cancel,
            deadline,
            control("stop", "shutdown"),
        ],
        &[],
    );

    assert_eq!(
        terminal(&output, "cancel-run")["status"],
        "cancellation-requested"
    );
    assert_eq!(
        terminal(&output, "cancelled")["error"]["code"],
        "warm/cancelled"
    );
    assert_eq!(
        terminal(&output, "deadline")["error"]["code"],
        "warm/deadline-exceeded"
    );
    let deadline = terminal(&output, "deadline");
    let phase = deadline["error"]["details"]["phase"]
        .as_str()
        .expect("deadline phase");
    assert_eq!(
        deadline["error"]["details"]["hard_termination"],
        phase == "running"
    );
    assert_eq!(
        terminal_audit(deadline)["worker_profile"],
        if phase == "running" {
            "native-isolated-v0.1"
        } else {
            "not-started-v0.1"
        }
    );
}

#[test]
fn warm_v02_restart_requires_renegotiation() {
    let td = tempfile::tempdir().unwrap();
    let output = run_warm(
        td.path(),
        &[
            initialize("init-0"),
            control("restart", "restart"),
            control("early", "ping"),
            initialize("init-1"),
            control("ready", "ping"),
            control("stop", "shutdown"),
        ],
        &[],
    );

    assert_eq!(terminal(&output, "restart")["status"], "restarted");
    assert_eq!(terminal(&output, "restart")["meta"]["generation"], 1);
    assert_eq!(
        terminal(&output, "early")["error"]["code"],
        "warm/not-initialized"
    );
    assert_eq!(terminal(&output, "init-1")["status"], "initialized");
    assert_eq!(terminal(&output, "ready")["status"], "ready");
}

#[test]
fn warm_v02_rejects_request_resource_overrides_before_admission() {
    let td = tempfile::tempdir().unwrap();
    fs::write(td.path().join("quick.gc"), "(prim int/add 1 1)\n").unwrap();
    let output = run_warm(
        td.path(),
        &[
            initialize("init"),
            execute(
                "override",
                "ws",
                ".",
                &["--step-limit", "1", "eval", "quick.gc"],
            ),
            control("stop", "shutdown"),
        ],
        &[],
    );

    assert_eq!(
        terminal(&output, "override")["error"]["code"],
        "warm/resource-override"
    );
    assert_eq!(responses(&output, "override").len(), 1);
}

#[cfg(unix)]
#[test]
fn warm_v02_hard_limits_kill_and_reap_native_workers_with_audits() {
    use std::os::unix::fs::PermissionsExt;

    let td = tempfile::tempdir().unwrap();
    write_slow_program(td.path());

    let wall = run_warm(
        td.path(),
        &[
            initialize("init-wall"),
            execute("wall", "ws", ".", &["eval", "slow.gc"]),
        ],
        &["--max-wall-ms", "1"],
    );
    let wall = terminal(&wall, "wall");
    assert_eq!(wall["error"]["code"], "warm/resource-exceeded");
    assert_eq!(wall["error"]["details"]["resource"], "wall");
    assert_eq!(
        terminal_audit(wall)["termination"],
        "resource-killed-and-reaped"
    );
    assert_native_audit(wall, td.path());

    let output = run_warm(
        td.path(),
        &[
            initialize("init-output"),
            execute("output", "ws", ".", &["cli-schema"]),
        ],
        &["--max-output-bytes", "1024"],
    );
    let output = terminal(&output, "output");
    assert_eq!(output["error"]["code"], "warm/resource-exceeded");
    assert_eq!(output["error"]["details"]["resource"], "output");
    assert_native_audit(output, td.path());

    let cpu = run_warm(
        td.path(),
        &[
            initialize("init-cpu"),
            execute("cpu", "ws", ".", &["eval", "slow.gc"]),
        ],
        &["--max-cpu-ms", "1", "--max-wall-ms", "10000"],
    );
    let cpu = terminal(&cpu, "cpu");
    assert_eq!(cpu["error"]["code"], "warm/resource-exceeded");
    assert_eq!(cpu["error"]["details"]["resource"], "cpu");
    assert_native_audit(cpu, td.path());

    fs::write(
        td.path().join("heap.gc"),
        "(def prog (((core/process::spawn \"echo\") []) {}))\nprog\n",
    )
    .unwrap();
    fs::write(
        td.path().join("heap.toml"),
        r#"
allow = ["sys/process::spawn"]
[op."sys/process::spawn"]
allow_programs = ["echo"]
base_dir = "."
bridge_cmd = "heap_bridge.sh"
max_bytes = 4096
"#,
    )
    .unwrap();
    fs::write(
        td.path().join("heap_bridge.sh"),
        r#"#!/bin/sh
exec python3 -c 'import time
resident = bytearray(96 * 1024 * 1024)
for offset in range(0, len(resident), 4096):
    resident[offset] = 1
time.sleep(5)'
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(td.path().join("heap_bridge.sh"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(td.path().join("heap_bridge.sh"), permissions).unwrap();
    let heap = run_warm(
        td.path(),
        &[
            initialize("init-heap"),
            execute(
                "heap",
                "ws",
                ".",
                &["run", "heap.gc", "--caps", "heap.toml"],
            ),
        ],
        &[
            "--max-heap-bytes",
            "67108864",
            "--max-wall-ms",
            "30000",
            "--max-processes",
            "4",
        ],
    );
    let heap = terminal(&heap, "heap");
    assert_eq!(heap["error"]["details"]["resource"], "heap");
    assert!(
        terminal_audit(heap)["observed"]["peak_heap_bytes"]
            .as_u64()
            .is_some_and(|value| value > 67_108_864)
    );
    assert_native_audit(heap, td.path());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn warm_v02_contains_aborted_worker_and_runs_the_next_request() {
    use std::os::unix::fs::PermissionsExt;

    let td = tempfile::tempdir().unwrap();
    fs::write(td.path().join("quick.gc"), "(prim int/add 20 22)\n").unwrap();
    fs::write(
        td.path().join("abort.gc"),
        "(def prog (((core/process::spawn \"echo\") []) {}))\nprog\n",
    )
    .unwrap();
    fs::write(
        td.path().join("abort.toml"),
        r#"
allow = ["sys/process::spawn"]
[op."sys/process::spawn"]
allow_programs = ["echo"]
base_dir = "."
bridge_cmd = "abort_bridge.sh"
max_bytes = 4096
"#,
    )
    .unwrap();
    fs::write(
        td.path().join("abort_bridge.sh"),
        "#!/bin/sh\nkill -KILL \"$PPID\"\nsleep 5\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(td.path().join("abort_bridge.sh"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(td.path().join("abort_bridge.sh"), permissions).unwrap();

    let output = run_warm(
        td.path(),
        &[
            initialize("init"),
            execute(
                "abort",
                "ws",
                ".",
                &["run", "abort.gc", "--caps", "abort.toml"],
            ),
            execute("healthy", "ws", ".", &["eval", "quick.gc"]),
            control("stop", "shutdown"),
        ],
        &["--max-wall-ms", "30000", "--max-processes", "4"],
    );

    let aborted = terminal(&output, "abort");
    assert_eq!(aborted["error"]["code"], "warm/worker-abort");
    assert_eq!(aborted["error"]["retryable"], true);
    assert_eq!(aborted["error"]["details"]["daemon_available"], true);
    assert_eq!(aborted["error"]["details"]["signal"], 9);
    assert_eq!(
        terminal_audit(aborted)["termination"],
        "worker-signal-contained"
    );
    assert_native_audit(aborted, td.path());

    let healthy = terminal(&output, "healthy");
    assert_eq!(healthy["ok"], true, "daemon did not recover: {healthy}");
    assert_eq!(healthy["status"], "completed");
    assert_eq!(healthy["meta"]["generation"], 0);
    assert_eq!(healthy["meta"]["crash_count"], 1);
}

#[test]
fn warm_v02_eof_terminalizes_every_accepted_request_with_a_bounded_drain() {
    let td = tempfile::tempdir().unwrap();
    write_slow_program(td.path());
    let output = run_warm(
        td.path(),
        &[
            initialize("init"),
            execute("one", "ws", ".", &["eval", "slow.gc"]),
            execute("two", "ws", ".", &["eval", "quick.gc"]),
            execute("three", "ws", ".", &["eval", "quick.gc"]),
        ],
        &["--max-drain-requests", "1", "--drain-timeout-ms", "10000"],
    );

    for id in ["one", "two", "three"] {
        assert_eq!(
            responses(&output, id).len(),
            2,
            "accepted plus terminal for {id}"
        );
        let terminal = terminal(&output, id);
        let audit = terminal_audit(terminal);
        assert_eq!(audit["kind"], "genesis/agent-session-audit-v0.1");
        assert!(
            audit["limits_identity"]
                .as_str()
                .is_some_and(|value| value.len() == 64)
        );
    }
    let bounded = ["one", "two", "three"]
        .iter()
        .filter(|id| terminal(&output, id)["error"]["code"] == "warm/drain-bounded")
        .count();
    assert_eq!(bounded, 2);
}

#[cfg(unix)]
#[test]
fn warm_v02_enforces_steps_effects_processes_and_disk_with_exact_evidence() {
    use std::os::unix::fs::PermissionsExt;

    let td = tempfile::tempdir().unwrap();
    write_slow_program(td.path());

    let steps = run_warm(
        td.path(),
        &[
            initialize("init-steps"),
            execute("steps", "ws", ".", &["eval", "slow.gc"]),
        ],
        &["--max-steps", "100"],
    );
    let steps = terminal(&steps, "steps");
    assert_eq!(steps["error"]["details"]["resource"], "steps");
    assert_eq!(
        steps["error"]["details"]["command_envelope"]["error"]["context"]["kind"],
        "step-limit"
    );
    assert_native_audit(steps, td.path());

    fs::write(
        td.path().join("effects.gc"),
        r#"
(def prog
  ((core/effect::bind
     (core/effect::perform 'sys/time::now nil (fn (first) (core/effect::pure first))))
    (fn (_)
      (core/effect::perform 'sys/time::now nil (fn (second) (core/effect::pure second))))))
prog
"#,
    )
    .unwrap();
    fs::write(
        td.path().join("effects.toml"),
        "allow = [\"sys/time::now\"]\n",
    )
    .unwrap();
    let effects = run_warm(
        td.path(),
        &[
            initialize("init-effects"),
            execute(
                "effects",
                "ws",
                ".",
                &["run", "effects.gc", "--caps", "effects.toml"],
            ),
        ],
        &["--max-effects", "1"],
    );
    let effects = terminal(&effects, "effects");
    assert_eq!(effects["error"]["details"]["resource"], "effects");
    assert_eq!(terminal_audit(effects)["observed"]["effect_ops"], 2);
    assert_native_audit(effects, td.path());

    fs::write(
        td.path().join("process.gc"),
        "(def prog (((core/process::spawn \"echo\") []) {}))\nprog\n",
    )
    .unwrap();
    fs::write(
        td.path().join("process.toml"),
        r#"
allow = ["sys/process::spawn"]
[op."sys/process::spawn"]
allow_programs = ["echo"]
base_dir = "."
bridge_cmd = "slow_bridge.sh"
max_bytes = 4096
"#,
    )
    .unwrap();
    fs::write(
        td.path().join("slow_bridge.sh"),
        r#"#!/bin/sh
sleep 5
response='{:ok true :process-id "never"}'
printf '%s\n%s' "${#response}" "$response"
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(td.path().join("slow_bridge.sh"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(td.path().join("slow_bridge.sh"), permissions).unwrap();
    let processes = run_warm(
        td.path(),
        &[
            initialize("init-processes"),
            execute(
                "processes",
                "ws",
                ".",
                &["run", "process.gc", "--caps", "process.toml"],
            ),
        ],
        &["--max-processes", "1"],
    );
    let processes = terminal(&processes, "processes");
    assert_eq!(processes["error"]["details"]["resource"], "processes");
    assert!(
        terminal_audit(processes)["observed"]["peak_processes"]
            .as_u64()
            .is_some_and(|value| value > 1)
    );
    assert_native_audit(processes, td.path());

    fs::write(
        td.path().join("disk_bridge.sh"),
        r#"#!/bin/sh
dd if=/dev/zero of=disk-a.bin bs=700000 count=1 status=none
dd if=/dev/zero of=disk-b.bin bs=700000 count=1 status=none
response='{:ok true :process-id "disk"}'
printf '%s\n%s' "${#response}" "$response"
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(td.path().join("disk_bridge.sh"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(td.path().join("disk_bridge.sh"), permissions).unwrap();
    fs::write(
        td.path().join("disk.toml"),
        r#"
allow = ["sys/process::spawn"]
[op."sys/process::spawn"]
allow_programs = ["echo"]
base_dir = "."
bridge_cmd = "disk_bridge.sh"
max_bytes = 4096
"#,
    )
    .unwrap();
    let disk = run_warm(
        td.path(),
        &[
            initialize("init-disk"),
            execute(
                "disk",
                "ws",
                ".",
                &["run", "process.gc", "--caps", "disk.toml"],
            ),
        ],
        &["--max-disk-bytes", "1048576", "--max-processes", "4"],
    );
    let disk = terminal(&disk, "disk");
    assert_eq!(
        disk["error"]["details"]["resource"], "disk",
        "unexpected disk terminal response: {disk}"
    );
    assert!(
        terminal_audit(disk)["observed"]["disk_delta_bytes"]
            .as_i64()
            .is_some_and(|value| value > 1_048_576)
    );
    assert_native_audit(disk, td.path());
}
