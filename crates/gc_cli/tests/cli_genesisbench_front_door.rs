use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::tempdir;

mod support;

fn run_genesis(root: &Path, artifact: &Path, args: &[&str]) -> Output {
    cargo_bin_cmd!("genesis")
        .current_dir(root)
        .arg("--json")
        .arg("--selfhost-artifact")
        .arg(artifact)
        .args(args)
        .output()
        .expect("execute genesis benchmark command")
}

fn data(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("parse genesis envelope");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["kind"], "genesis/bench-v0.1");
    envelope["data"].clone()
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create copied tree");
    for entry in fs::read_dir(source).expect("read source tree") {
        let entry = entry.expect("read source entry");
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &target_path);
        } else {
            fs::copy(source_path, target_path).expect("copy source file");
        }
    }
}

#[test]
fn canonical_front_door_runs_replays_bundles_and_submits_without_adapter_reinvocation() {
    let root = support::repo_root();
    let temp = tempdir().expect("temporary benchmark root");
    let artifact = support::copy_repo_toolchain_artifact(temp.path());

    let inspected = data(&run_genesis(
        &root,
        &artifact,
        &["bench", "inspect", "--case", "generation-small"],
    ));
    assert_eq!(
        inspected["adapterClasses"],
        serde_json::json!([
            "hosted-api",
            "local-openai-compatible",
            "direct-local-runtime",
            "command-plugin",
            "deterministic-mock"
        ])
    );
    assert_eq!(inspected["commands"].as_array().unwrap().len(), 7);

    let external_adapter = temp.path().join("adapter.json");
    let external_executable = temp.path().join("adapter.py");
    fs::copy(
        root.join("benchmarks/genesisbench/v0.1/adapters/command-plugin.json"),
        &external_adapter,
    )
    .expect("copy adapter manifest");
    fs::copy(
        root.join("benchmarks/genesisbench/v0.1/adapters/command_fixture.py"),
        &external_executable,
    )
    .expect("copy adapter executable");
    let mut permissions = fs::metadata(&external_executable)
        .expect("adapter executable metadata")
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&external_executable, permissions).expect("make adapter executable");
    }

    let run_root = temp.path().join("run");
    let executed = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "run",
            "--case",
            "generation-small",
            "--adapter",
            external_adapter.to_str().unwrap(),
            "--adapter-executable",
            external_executable.to_str().unwrap(),
            "--out",
            run_root.to_str().unwrap(),
        ],
    ));
    assert_eq!(executed["outcome"], "verified");
    assert_eq!(executed["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(executed["attempts"][0]["status"], "succeeded");
    assert_eq!(
        executed["attempts"][0]["secretDisclosure"]["valuesRecorded"],
        false
    );
    assert!(
        !fs::read_to_string(run_root.join("run.json"))
            .unwrap()
            .contains(root.to_str().unwrap())
    );

    fs::remove_file(external_adapter).expect("remove external adapter manifest");
    fs::remove_file(external_executable).expect("remove external adapter executable");
    let validated = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "validate-run",
            "--run",
            run_root.join("run.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(validated["valid"], true);
    let replayed = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "replay",
            "--run",
            run_root.join("run.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(replayed["adapterInvoked"], false);
    assert_eq!(replayed["modelAccessed"], false);
    assert_eq!(replayed["allFieldsValidated"], true);
    assert_eq!(replayed["independentRescoreMatched"], true);

    let bundle_a = temp.path().join("a.gcbundle");
    let bundle_b = temp.path().join("b.gcbundle");
    for bundle in [&bundle_a, &bundle_b] {
        data(&run_genesis(
            &root,
            &artifact,
            &[
                "bench",
                "bundle",
                "--run",
                run_root.join("run.json").to_str().unwrap(),
                "--out",
                bundle.to_str().unwrap(),
            ],
        ));
    }
    assert_eq!(fs::read(&bundle_a).unwrap(), fs::read(&bundle_b).unwrap());

    let outbox = temp.path().join("outbox");
    let submitted = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "submit",
            "--bundle",
            bundle_a.to_str().unwrap(),
            "--outbox",
            outbox.to_str().unwrap(),
            "--submitter",
            "integration-test",
        ],
    ));
    assert_eq!(submitted["transport"], "local-immutable-outbox-v0.1");
    assert_eq!(fs::read_dir(&outbox).unwrap().count(), 2);

    let tampered = temp.path().join("tampered");
    copy_tree(&run_root, &tampered);
    fs::write(tampered.join("candidate/main.gc"), "41\n").expect("tamper candidate");
    let rejected = run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "validate-run",
            "--run",
            tampered.join("run.json").to_str().unwrap(),
        ],
    );
    assert!(
        !rejected.status.success(),
        "tampered candidate must fail closed"
    );
    let error: Value = serde_json::from_slice(&rejected.stdout).expect("parse rejection envelope");
    assert_eq!(error["error"]["code"], "bench/front-door-failed");
}

#[test]
fn failed_provider_attempt_is_retained_as_replayable_invalid_evidence() {
    let root = support::repo_root();
    let temp = tempdir().expect("temporary benchmark root");
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let run_root = temp.path().join("failed-run");
    let output = cargo_bin_cmd!("genesis")
        .current_dir(&root)
        .env_remove("GENESISBENCH_FIXTURE_API_KEY")
        .arg("--json")
        .arg("--selfhost-artifact")
        .arg(&artifact)
        .args([
            "bench",
            "run",
            "--case",
            "generation-small",
            "--adapter",
            "benchmarks/genesisbench/v0.1/adapters/hosted-api.json",
            "--out",
        ])
        .arg(&run_root)
        .output()
        .expect("execute failed provider fixture");
    let run = data(&output);
    assert_eq!(run["outcome"], "invalid");
    assert_eq!(run["attempts"][0]["status"], "failed");
    assert!(run["scoreIdentitySha256"].is_null());
    assert!(!run_root.join("score.json").exists());

    data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "validate-run",
            "--run",
            run_root.join("run.json").to_str().unwrap(),
        ],
    ));
    let replayed = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "replay",
            "--run",
            run_root.join("run.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(replayed["adapterInvoked"], false);
    assert_eq!(replayed["independentRescoreMatched"], false);
}

#[test]
fn generated_authorities_and_all_adapter_controls_are_current() {
    let root: PathBuf = support::repo_root();
    let output = Command::new("python3")
        .current_dir(&root)
        .args([
            "scripts/lib/genesisbench_front_door.py",
            "check",
            "--self-test",
        ])
        .output()
        .expect("check GenesisBench front-door authorities");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse authority report");
    assert_eq!(report["adapterClasses"], 5);
    assert_eq!(report["controls"], 26);
}
