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
    assert_eq!(
        terminal(&output, "deadline")["error"]["details"]["hard_termination"],
        false
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
