use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const PROTOCOL: &str = "2025-11-25";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn initialize(id: u64, roots: bool) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL,
            "capabilities": if roots { json!({"roots": {"listChanged": true}}) } else { json!({}) },
            "clientInfo": {"name": "gc-cli-mcp-test", "version": "0.1.0"}
        }
    })
}

struct McpChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl McpChild {
    fn spawn(cwd: &Path, extra: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_genesis"));
        command
            .current_dir(cwd)
            .env(
                "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
                repo_root().join("selfhost/toolchain.gc"),
            )
            .arg("mcp")
            .args(["--prime-selfhost", "false", "--workspace-root", "."])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn MCP server");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin: Some(stdin),
            stdout,
        }
    }

    fn send(&mut self, value: &Value) {
        let stdin = self.stdin.as_mut().expect("open stdin");
        writeln!(stdin, "{value}").expect("write MCP frame");
        stdin.flush().expect("flush MCP frame");
    }

    fn send_raw(&mut self, value: &str) {
        let stdin = self.stdin.as_mut().expect("open stdin");
        writeln!(stdin, "{value}").expect("write raw MCP frame");
        stdin.flush().expect("flush raw MCP frame");
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read MCP frame");
        assert!(!line.is_empty(), "MCP server closed before a response");
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("invalid MCP frame `{line}`: {error}"))
    }

    fn finish(mut self) -> String {
        drop(self.stdin.take());
        let status = self.child.wait().expect("wait for MCP server");
        assert!(status.success(), "MCP server exited with {status}");
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("read stderr");
        stderr
    }

    fn finish_frames(mut self) -> (Vec<Value>, String) {
        drop(self.stdin.take());
        let mut frames = Vec::new();
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read MCP frame");
            if read == 0 {
                break;
            }
            frames.push(serde_json::from_str(&line).expect("valid MCP frame"));
        }
        let status = self.child.wait().expect("wait for MCP server");
        assert!(status.success(), "MCP server exited with {status}");
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("read stderr");
        (frames, stderr)
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        if self.stdin.is_some() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn complete_handshake(server: &mut McpChild, roots: bool) {
    server.send(&initialize(1, roots));
    let initialized = server.read();
    assert_eq!(initialized["id"], 1);
    assert_eq!(initialized["result"]["protocolVersion"], PROTOCOL);
    assert!(initialized["result"]["capabilities"].get("tasks").is_none());
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    }));
}

#[test]
fn mcp_lists_generated_tools_and_resources_without_stdout_pollution() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut server = McpChild::spawn(td.path(), &[]);
    complete_handshake(&mut server, false);
    server.send(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
    let tools = server.read();
    assert_eq!(tools["id"], 2);
    let tools = tools["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 20);
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("name"))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "apply-patch",
            "build",
            "check",
            "diff",
            "explain",
            "format",
            "get-card",
            "package",
            "parse",
            "replay",
            "run",
            "search-symbol",
            "session-abort",
            "session-apply",
            "session-begin",
            "session-stage",
            "session-status",
            "session-test",
            "test",
            "verify"
        ]
    );
    assert!(
        tools
            .iter()
            .all(|tool| tool["execution"]["taskSupport"] == "forbidden")
    );
    server.send(&json!({
        "jsonrpc":"2.0","id":3,"method":"resources/read",
        "params":{"uri":"genesis://mcp/profile"}
    }));
    let resource = server.read();
    assert_eq!(resource["id"], 3);
    let profile_text = resource["result"]["contents"][0]["text"]
        .as_str()
        .expect("profile text");
    let profile: Value = serde_json::from_str(profile_text).expect("profile JSON");
    assert_eq!(profile["protocolVersion"], PROTOCOL);
    assert_eq!(profile["tools"].as_array().map(Vec::len), Some(20));
    assert_eq!(server.finish(), "");
}

#[test]
fn mcp_negotiates_roots_and_executes_parse_with_strict_progress() {
    let td = tempfile::tempdir().expect("tempdir");
    fs::write(td.path().join("program.gc"), "(prim int/add 1 2)\n").expect("program");
    let root_uri = format!(
        "file://{}",
        td.path().canonicalize().expect("root").display()
    );
    let mut server = McpChild::spawn(td.path(), &[]);
    complete_handshake(&mut server, true);
    let roots_request = server.read();
    assert_eq!(roots_request["method"], "roots/list");
    server.send(&json!({
        "jsonrpc":"2.0",
        "id": roots_request["id"],
        "result":{"roots":[{"uri":root_uri,"name":"test"}]}
    }));
    server.send(&json!({
        "jsonrpc":"2.0","id":"parse-1","method":"tools/call",
        "params":{
            "name":"parse",
            "arguments":{"root":root_uri,"file":"program.gc"},
            "_meta":{"progressToken":"progress-1"}
        }
    }));
    let started = server.read();
    let completed = server.read();
    let response = server.read();
    assert_eq!(started["method"], "notifications/progress");
    assert_eq!(started["params"]["progress"], 0);
    assert_eq!(completed["method"], "notifications/progress");
    assert_eq!(completed["params"]["progress"], 1);
    assert_eq!(response["id"], "parse-1");
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["structuredContent"]["kind"],
        "genesis/parse-v0.1"
    );
    assert_eq!(server.finish(), "");
}

#[test]
fn mcp_executes_transactional_session_through_generated_cli_routes() {
    let td = tempfile::tempdir().expect("tempdir");
    fs::write(
        td.path().join("package.toml"),
        "name = \"mcp-session\"\nversion = \"0.0.1\"\nmodules = []\ndependencies = []\nobligations = []\n",
    )
    .expect("package");
    let root_uri = format!(
        "file://{}",
        td.path().canonicalize().expect("root").display()
    );
    let mut server = McpChild::spawn(td.path(), &[]);
    complete_handshake(&mut server, true);
    let roots_request = server.read();
    server.send(&json!({
        "jsonrpc":"2.0","id":roots_request["id"],
        "result":{"roots":[{"uri":root_uri,"name":"transaction-root"}]}
    }));
    server.send(&json!({
        "jsonrpc":"2.0","id":"begin-1","method":"tools/call",
        "params":{"name":"session-begin","arguments":{
            "root":root_uri,"pkg":"package.toml","session":"candidate"
        }}
    }));
    let begin = server.read();
    assert_eq!(begin["id"], "begin-1");
    assert_eq!(begin["result"]["isError"], false);
    assert_eq!(
        begin["result"]["structuredContent"]["kind"],
        "genesis/agent-session-begin-v0.1"
    );
    let snapshot = begin["result"]["structuredContent"]["data"]["base_snapshot"]
        .as_str()
        .expect("snapshot");
    assert_eq!(snapshot.len(), 64);

    server.send(&json!({
        "jsonrpc":"2.0","id":"status-1","method":"tools/call",
        "params":{"name":"session-status","arguments":{
            "root":root_uri,"pkg":"package.toml","session":"candidate"
        }}
    }));
    let status = server.read();
    assert_eq!(status["id"], "status-1");
    assert_eq!(status["result"]["isError"], false);
    assert_eq!(
        status["result"]["structuredContent"]["data"]["current_snapshot"],
        snapshot
    );
    assert_eq!(server.finish(), "");
}

#[test]
fn mcp_rejects_batches_tasks_and_escaped_roots_without_panicking() {
    let td = tempfile::tempdir().expect("tempdir");
    let parent_uri = format!(
        "file://{}",
        td.path()
            .parent()
            .expect("parent")
            .canonicalize()
            .expect("parent root")
            .display()
    );
    let mut server = McpChild::spawn(
        td.path(),
        &["--max-frame-bytes", "512", "--max-output-bytes", "1024"],
    );
    server.send_raw("[]");
    assert_eq!(server.read()["error"]["code"], -32600);
    server.send_raw(&format!("{{\"padding\":\"{}\"}}", "x".repeat(700)));
    assert_eq!(server.read()["error"]["code"], -32003);
    complete_handshake(&mut server, true);
    let roots_request = server.read();
    server.send(&json!({
        "jsonrpc":"2.0","id":roots_request["id"],
        "result":{"roots":[{"uri":parent_uri}]}
    }));
    server.send(&json!({
        "jsonrpc":"2.0","id":7,"method":"tools/call",
        "params":{"name":"get-card","arguments":{"card":"core"},"task":{"ttl":1000}}
    }));
    let task_error = server.read();
    assert_eq!(task_error["error"]["code"], -32602);
    assert_eq!(
        task_error["error"]["message"],
        "MCP Tasks were not negotiated"
    );
    server.send(&json!({
        "jsonrpc":"2.0","id":8,"method":"tasks/list","params":{}
    }));
    assert_eq!(server.read()["error"]["code"], -32601);
    server.send(&json!({
        "jsonrpc":"2.0","id":10,"method":"resources/read",
        "params":{"uri":"genesis://mcp/profile"}
    }));
    assert_eq!(server.read()["error"]["code"], -32003);
    server.send(&json!({
        "jsonrpc":"2.0","id":9,"method":"tools/call",
        "params":{"name":"get-card","arguments":{"card":"core"}}
    }));
    let root_error = server.read();
    assert_eq!(root_error["error"]["code"], -32602);
    assert_eq!(
        root_error["error"]["message"],
        "client exposed no usable workspace roots"
    );
    assert_eq!(server.finish(), "");
}

#[test]
fn mcp_eof_terminalizes_every_accepted_call_with_audited_provenance() {
    let td = tempfile::tempdir().expect("tempdir");
    let mut server = McpChild::spawn(
        td.path(),
        &["--max-drain-requests", "0", "--drain-timeout-ms", "1000"],
    );
    complete_handshake(&mut server, false);
    for id in ["one", "two", "three"] {
        server.send(&json!({
            "jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":"get-card","arguments":{"card":"core"}}
        }));
    }
    let (frames, stderr) = server.finish_frames();
    assert_eq!(stderr, "");
    let mut disconnect_cancellations = 0;
    for id in ["one", "two", "three"] {
        let response = frames
            .iter()
            .find(|frame| frame["id"] == id)
            .unwrap_or_else(|| panic!("missing terminal response for {id}: {frames:?}"));
        let audit = if let Some(code) = response["error"]["code"].as_i64() {
            assert!(
                matches!(code, -32005 | -32006),
                "unexpected EOF terminal response for {id}: {response}"
            );
            disconnect_cancellations += 1;
            &response["error"]["data"]["audit"]
        } else {
            assert!(
                response["result"].is_object(),
                "accepted call must complete or receive a disconnect error: {response}"
            );
            &response["result"]["_meta"]["genesis/sessionAudit"]
        };
        assert_eq!(audit["kind"], "genesis/agent-session-audit-v0.1");
        assert!(
            audit["limits_identity"]
                .as_str()
                .is_some_and(|value| value.len() == 64)
        );
    }
    assert!(
        disconnect_cancellations > 0,
        "EOF with a zero-request drain budget must cancel outstanding work: {frames:?}"
    );
}
