use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin_cmd;

fn canonical_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(entries) => {
            let mut keys = entries.keys().collect::<Vec<_>>();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical_json(&entries[key]));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonical_json).collect())
        }
        _ => value.clone(),
    }
}

fn forged_snapshot_identity(snapshot: &serde_json::Value) -> String {
    let payload = serde_json::json!({
        "schema": snapshot["schema"],
        "files": snapshot["files"],
    });
    let encoded =
        serde_json::to_string(&canonical_json(&payload)).expect("encode snapshot payload");
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"GCv0.2\0workspace-snapshot-v0.1\0");
    hasher.update(encoded.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn write_package(root: &Path) {
    fs::write(
        root.join("package.toml"),
        r#"
name = "agent-session-test"
version = "0.0.1"
dependencies = []
obligations = ["core/obligation::unit-tests"]
modules = [
  { path = "a.gc", hash = "" },
  { path = "b.gc", hash = "" }
]
tests = ["agent/session::tests"]
"#,
    )
    .expect("write package manifest");
    fs::write(
        root.join("a.gc"),
        r#"
(def agent/session::value 41)
(def agent/session::tests
  {"value" {:body (fn (_) agent/session::value) :expect 41}})
"#,
    )
    .expect("write a.gc");
    fs::write(
        root.join("b.gc"),
        "(def agent/session::read (fn (_) agent/session::value))\n",
    )
    .expect("write b.gc");
}

fn json_command(root: &Path, args: &[&str], success: bool) -> serde_json::Value {
    let mut command = cargo_bin_cmd!("genesis_parity");
    command
        .current_dir(root)
        .args(["--json", "--coreform-frontend", "rust"]);
    command.args(args);
    let assertion = command.assert();
    let output = if success {
        assertion.success().get_output().stdout.clone()
    } else {
        assertion.failure().get_output().stdout.clone()
    };
    serde_json::from_slice(&output).expect("parse JSON command output")
}

fn refactor_patch(root: &Path, destination: &str) -> String {
    let plan = json_command(
        root,
        &[
            "semantic-edit",
            "refactor-plan",
            "--pkg",
            "package.toml",
            "--kind",
            "rename",
            "--from",
            "agent/session::value",
            "--to",
            destination,
        ],
        true,
    );
    plan.pointer("/data/patch_coreform")
        .and_then(serde_json::Value::as_str)
        .expect("plan patch")
        .to_string()
}

#[test]
fn agent_session_stages_tests_and_explicitly_applies_a_verified_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_package(root);
    let original_a = fs::read(root.join("a.gc")).expect("read original module");
    let patch = refactor_patch(root, "agent/session::value_v2");
    fs::write(root.join("rename.gcpatch"), patch).expect("write patch");

    let begin = json_command(
        root,
        &[
            "session",
            "begin",
            "--pkg",
            "package.toml",
            "--session",
            "candidate-a",
        ],
        true,
    );
    assert_eq!(
        begin.get("kind").and_then(serde_json::Value::as_str),
        Some("genesis/agent-session-begin-v0.1")
    );
    let base = begin
        .pointer("/data/base_snapshot")
        .and_then(serde_json::Value::as_str)
        .expect("base snapshot");
    assert_eq!(base.len(), 64);

    let stage = json_command(
        root,
        &[
            "session",
            "stage",
            "--pkg",
            "package.toml",
            "--session",
            "candidate-a",
            "--patch",
            "rename.gcpatch",
        ],
        true,
    );
    assert_eq!(
        stage
            .pointer("/data/obligations_ok")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        fs::read(root.join("a.gc")).expect("read unchanged live module"),
        original_a,
        "staging must not mutate the live package"
    );
    let isolated = fs::read_to_string(
        root.join(".genesis/agent-sessions/transactions/candidate-a/workspace/a.gc"),
    )
    .expect("read isolated module");
    assert!(isolated.contains("agent/session::value_v2"));

    let test = json_command(
        root,
        &[
            "session",
            "test",
            "--pkg",
            "package.toml",
            "--session",
            "candidate-a",
        ],
        true,
    );
    assert_eq!(
        test.pointer("/data/obligations_ok")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let apply = json_command(
        root,
        &[
            "session",
            "apply",
            "--pkg",
            "package.toml",
            "--session",
            "candidate-a",
        ],
        true,
    );
    assert_eq!(
        apply
            .pointer("/data/status")
            .and_then(serde_json::Value::as_str),
        Some("applied")
    );
    let live = fs::read_to_string(root.join("a.gc")).expect("read applied module");
    assert!(live.contains("agent/session::value_v2"));

    let status = json_command(
        root,
        &[
            "session",
            "status",
            "--pkg",
            "package.toml",
            "--session",
            "candidate-a",
        ],
        true,
    );
    assert_eq!(
        status
            .pointer("/data/status")
            .and_then(serde_json::Value::as_str),
        Some("applied")
    );
    assert_eq!(
        status
            .pointer("/data/patch_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
}

#[test]
fn agent_session_rejects_stale_live_state_without_overwriting_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_package(root);
    let patch = refactor_patch(root, "agent/session::value_v2");
    fs::write(root.join("rename.gcpatch"), patch).expect("write patch");
    json_command(
        root,
        &[
            "session",
            "begin",
            "--pkg",
            "package.toml",
            "--session",
            "stale",
        ],
        true,
    );
    json_command(
        root,
        &[
            "session",
            "stage",
            "--pkg",
            "package.toml",
            "--session",
            "stale",
            "--patch",
            "rename.gcpatch",
        ],
        true,
    );
    fs::write(root.join("b.gc"), "(def user/concurrent-change 99)\n")
        .expect("write concurrent change");

    let rejected = json_command(
        root,
        &[
            "session",
            "apply",
            "--pkg",
            "package.toml",
            "--session",
            "stale",
        ],
        false,
    );
    assert_eq!(
        rejected
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str),
        Some("session/stale-base")
    );
    assert_eq!(
        fs::read_to_string(root.join("b.gc")).expect("read concurrent change"),
        "(def user/concurrent-change 99)\n"
    );
    assert!(
        !root.join(".genesis/agent-sessions/apply.lock").exists(),
        "apply lock must be reaped after rejection"
    );
}

#[test]
fn agent_session_rejects_unverified_and_tampered_transactions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_package(root);
    let patch = refactor_patch(root, "agent/session::value_v2");
    fs::write(root.join("rename.gcpatch"), patch).expect("write patch");
    json_command(
        root,
        &[
            "session",
            "begin",
            "--pkg",
            "package.toml",
            "--session",
            "guarded",
        ],
        true,
    );
    let unverified = json_command(
        root,
        &[
            "session",
            "apply",
            "--pkg",
            "package.toml",
            "--session",
            "guarded",
        ],
        false,
    );
    assert_eq!(
        unverified
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str),
        Some("session/unverified")
    );

    json_command(
        root,
        &[
            "session",
            "stage",
            "--pkg",
            "package.toml",
            "--session",
            "guarded",
            "--patch",
            "rename.gcpatch",
        ],
        true,
    );
    fs::write(
        root.join(".genesis/agent-sessions/transactions/guarded/workspace/a.gc"),
        "(def injected/value 7)\n",
    )
    .expect("tamper isolated workspace");
    let tampered = json_command(
        root,
        &[
            "session",
            "apply",
            "--pkg",
            "package.toml",
            "--session",
            "guarded",
        ],
        false,
    );
    assert_eq!(
        tampered
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str),
        Some("session/workspace-tampered")
    );
}

#[test]
fn agent_session_rejects_a_tampered_state_chain() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_package(root);
    json_command(
        root,
        &[
            "session",
            "begin",
            "--pkg",
            "package.toml",
            "--session",
            "state-tamper",
        ],
        true,
    );

    let state_path = root.join(".genesis/agent-sessions/transactions/state-tamper/session.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read transaction state"))
            .expect("parse transaction state");
    state["current_snapshot"] = serde_json::Value::String("0".repeat(64));
    fs::write(
        &state_path,
        serde_json::to_vec(&state).expect("encode tampered state"),
    )
    .expect("write tampered transaction state");

    let rejected = json_command(
        root,
        &[
            "session",
            "status",
            "--pkg",
            "package.toml",
            "--session",
            "state-tamper",
        ],
        false,
    );
    assert_eq!(
        rejected
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str),
        Some("session/state-invalid")
    );
}

#[test]
fn agent_session_rejects_a_content_addressed_snapshot_path_escape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_package(root);
    let begin = json_command(
        root,
        &[
            "session",
            "begin",
            "--pkg",
            "package.toml",
            "--session",
            "path-forgery",
        ],
        true,
    );
    let base = begin
        .pointer("/data/base_snapshot")
        .and_then(serde_json::Value::as_str)
        .expect("base snapshot");
    let store = root.join(".genesis/agent-sessions");
    let original_snapshot_path = store.join("snapshots").join(format!("{base}.json"));
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&fs::read(&original_snapshot_path).expect("read snapshot manifest"))
            .expect("parse snapshot manifest");
    snapshot["files"][0]["path"] = serde_json::Value::String("../outside.gc".to_string());
    let forged_identity = forged_snapshot_identity(&snapshot);
    snapshot["identity"] = serde_json::Value::String(forged_identity.clone());
    fs::write(
        store
            .join("snapshots")
            .join(format!("{forged_identity}.json")),
        serde_json::to_vec(&canonical_json(&snapshot)).expect("encode forged snapshot"),
    )
    .expect("write forged snapshot");

    let state_path = store.join("transactions/path-forgery/session.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read transaction state"))
            .expect("parse transaction state");
    state["base_snapshot"] = serde_json::Value::String(forged_identity.clone());
    state["current_snapshot"] = serde_json::Value::String(forged_identity);
    fs::write(
        &state_path,
        serde_json::to_vec(&canonical_json(&state)).expect("encode forged state"),
    )
    .expect("write forged state");

    let rejected = json_command(
        root,
        &[
            "session",
            "status",
            "--pkg",
            "package.toml",
            "--session",
            "path-forgery",
        ],
        false,
    );
    assert_eq!(
        rejected
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str),
        Some("session/snapshot-mismatch")
    );
}

#[test]
fn agent_session_errors_do_not_expose_absolute_package_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let manifest = root.join("malformed-package.toml");
    fs::write(&manifest, "this is not valid toml = [").expect("write malformed manifest");
    let manifest_arg = manifest.to_string_lossy().into_owned();
    let rejected = json_command(
        root,
        &[
            "session",
            "begin",
            "--pkg",
            &manifest_arg,
            "--session",
            "path-redaction",
        ],
        false,
    );
    assert_eq!(
        rejected
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str),
        Some("session/package-invalid")
    );
    let encoded = serde_json::to_string(&rejected).expect("encode rejection");
    assert!(
        !encoded.contains(&root.to_string_lossy().into_owned()),
        "transaction errors must not expose absolute workspace roots"
    );
}
