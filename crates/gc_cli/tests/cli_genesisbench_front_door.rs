use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use sha2::{Digest, Sha256};
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
        "command failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
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

fn seal(mut value: Value) -> Value {
    value["contentIdentitySha256"] = Value::String(String::new());
    let mut bytes = serde_json::to_vec(&value).expect("canonical JSON");
    bytes.push(b'\n');
    value["contentIdentitySha256"] = Value::String(format!("{:x}", Sha256::digest(bytes)));
    value
}

fn write_submission_claim(root: &Path, path: &Path) {
    let reference: Value = serde_json::from_slice(
        &fs::read(root.join("docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json"))
            .expect("read reference agent"),
    )
    .expect("parse reference agent");
    let lineages = serde_json::json!(["lineage-generation-001"]);
    let mut lineage_bytes = serde_json::to_vec(&lineages).expect("lineage JSON");
    lineage_bytes.push(b'\n');
    let claim = seal(serde_json::json!({
        "kind": "genesis/genesisbench-submission-claim-v0.1",
        "version": "0.1.0",
        "registryPolicyIdentitySha256": "1111111111111111111111111111111111111111111111111111111111111111",
        "evaluation": {
            "id": "front-door-integration",
            "taskEpochId": "public-v0.1",
            "contextMode": "compact-small",
            "interactionMode": "artifact-response-v0.1",
            "expectedLineageIds": lineages,
            "expectedLineagesIdentitySha256": format!("{:x}", Sha256::digest(lineage_bytes)),
        },
        "track": {
            "id": "open-agent",
            "scaffoldClass": "fixed-reference",
            "scaffoldIdentitySha256": reference["contentIdentitySha256"],
            "genesisSpecificTraining": "unknown",
            "adaptationIdentitySha256": null,
            "inferenceMode": "local-offline",
            "networkMode": "deny",
        },
        "model": {
            "familyId": "fixture-family",
            "providerId": "fixture-provider",
            "id": "genesisbench-fixture-model",
            "revision": "v0.1",
            "runtimeId": "command-fixture",
            "runtimeVersion": "v0.1",
            "runtimeArtifactSha256": null,
        },
        "contamination": {
            "label": "declared-contaminated",
            "evidenceCodes": ["known-exposure/public-reference"],
            "evidenceIdentitySha256": null,
        },
        "hardware": {
            "classId": null,
            "combinedResidentBytes": null,
            "evidenceIdentitySha256": null,
            "measurementMethod": "not-claimed",
        },
        "economics": {
            "currency": null,
            "costMicrounits": null,
            "latencyMs": 0,
            "energyMillijoules": null,
        },
        "contentIdentitySha256": "",
    }));
    fs::write(path, serde_json::to_vec_pretty(&claim).unwrap()).expect("write claim");
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
    assert_eq!(inspected["commands"].as_array().unwrap().len(), 15);
    assert!(
        inspected["openAgentHarnessIdentitySha256"]
            .as_str()
            .is_some_and(|value| value.len() == 64)
    );

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
    let signing_key = temp.path().join("submitter.toml");
    let generated_key = run_genesis(
        &root,
        &artifact,
        &["keygen", "--out", signing_key.to_str().unwrap()],
    );
    assert!(
        generated_key.status.success(),
        "{}",
        String::from_utf8_lossy(&generated_key.stderr)
    );
    let claim = temp.path().join("claim.json");
    write_submission_claim(&root, &claim);
    let submitted = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "submit",
            "--bundle",
            bundle_a.to_str().unwrap(),
            "--claim",
            claim.to_str().unwrap(),
            "--outbox",
            outbox.to_str().unwrap(),
            "--submitter",
            "integration-test",
            "--key",
            signing_key.to_str().unwrap(),
        ],
    ));
    assert_eq!(submitted["transport"], "local-signed-immutable-outbox-v0.1");
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
fn open_agent_campaign_is_predeclared_isolated_validated_and_replayed_without_model_access() {
    let root = support::repo_root();
    let temp = tempdir().expect("temporary Open Agent root");
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let fixture = temp.path().join("codex-fixture.py");
    fs::write(
        &fixture,
        concat!(
            "#!/usr/bin/python3\n",
            "import json,pathlib,sys\n",
            "if '--version' in sys.argv:\n",
            " print('codex-cli 0.0.0-fixture')\n",
            " raise SystemExit(0)\n",
            "if pathlib.Path('package.toml').exists():\n",
            " pathlib.Path('deployment.json').write_text('{}\\n', encoding='ascii')\n",
            " with pathlib.Path('package.toml').open('a', encoding='ascii') as stream:\n",
            "  stream.write('# noneditable drift\\n')\n",
            "else:\n",
            " pathlib.Path('main.gc').write_text('42\\n', encoding='ascii')\n",
            "print(json.dumps({'type':'turn.completed','fixture':True}, sort_keys=True))\n",
        ),
    )
    .expect("write Open Agent fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fixture, fs::Permissions::from_mode(0o755))
            .expect("make Open Agent fixture executable");
    }

    let campaign = temp.path().join("campaign.json");
    let campaign_planned = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-campaign-plan",
            "--campaign",
            "codex-luna-xhigh-conformance",
            "--phase",
            "reality-gate",
            "--case",
            "completion-small",
            "--case",
            "deployment-small",
            "--case",
            "generation-small",
            "--case",
            "package-migration-small",
            "--case",
            "performance-repair-small",
            "--case",
            "policy-minimization-small",
            "--case",
            "refactor-small",
            "--case",
            "repair-small",
            "--case",
            "replay-investigation-small",
            "--runner",
            "codex-cli-hosted",
            "--agent-executable",
            fixture.to_str().unwrap(),
            "--model",
            "luna",
            "--model-revision",
            "provider-alias:luna@2026-07-17",
            "--reasoning-effort",
            "xhigh",
            "--timeout-ms",
            "30000",
            "--hardware-class",
            "fixture-host",
            "--out",
            campaign.to_str().unwrap(),
        ],
    ));
    assert_eq!(campaign_planned["phase"], "reality-gate");
    assert_eq!(campaign_planned["cases"].as_array().unwrap().len(), 9);
    assert_eq!(campaign_planned["publication"]["expectedAttemptCount"], 9);

    let predeclaration = temp.path().join("predeclaration.json");
    let planned = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-plan",
            "--case",
            "completion-small",
            "--campaign-predeclaration",
            campaign.to_str().unwrap(),
            "--out",
            predeclaration.to_str().unwrap(),
        ],
    ));
    assert_eq!(planned["track"]["id"], "open-agent");
    assert_eq!(planned["track"]["rankEligible"], false);
    assert_eq!(planned["attemptPolicy"]["attempts"], 1);
    assert_eq!(planned["model"]["requestedId"], "luna");
    assert_eq!(planned["model"]["reasoningEffort"], "xhigh");

    let run_root = temp.path().join("open-agent-run");
    let executed = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-run",
            "--campaign-predeclaration",
            campaign.to_str().unwrap(),
            "--predeclaration",
            predeclaration.to_str().unwrap(),
            "--agent-executable",
            fixture.to_str().unwrap(),
            "--out",
            run_root.to_str().unwrap(),
        ],
    ));
    assert_eq!(executed["outcome"], "verified");
    assert_eq!(executed["workspace"]["violations"], serde_json::json!([]));
    assert_eq!(executed["attempt"]["index"], 0);
    assert_eq!(executed["attempt"]["environmentValuesRecorded"], false);
    assert_eq!(
        fs::read(run_root.join("candidate/main.gc")).unwrap(),
        b"42\n"
    );

    let validated = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-validate",
            "--run",
            run_root.join("run.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(validated["valid"], true);

    let invalid_predeclaration = temp.path().join("invalid-predeclaration.json");
    let invalid_planned = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-plan",
            "--case",
            "deployment-small",
            "--campaign-predeclaration",
            campaign.to_str().unwrap(),
            "--out",
            invalid_predeclaration.to_str().unwrap(),
        ],
    ));
    assert_eq!(invalid_planned["case"]["id"], "deployment-small");

    let invalid_root = temp.path().join("invalid-open-agent-run");
    let invalid = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-run",
            "--campaign-predeclaration",
            campaign.to_str().unwrap(),
            "--predeclaration",
            invalid_predeclaration.to_str().unwrap(),
            "--agent-executable",
            fixture.to_str().unwrap(),
            "--out",
            invalid_root.to_str().unwrap(),
        ],
    ));
    assert_eq!(invalid["outcome"], "invalid");
    assert_eq!(
        invalid["workspace"]["violations"],
        serde_json::json!(["noneditable-input-drift"])
    );
    assert!(!invalid_root.join("candidate").exists());
    assert_eq!(
        fs::read(invalid_root.join("observed-workspace/deployment.json")).unwrap(),
        b"{}\n"
    );
    assert!(
        fs::read_to_string(invalid_root.join("observed-workspace/package.toml"))
            .unwrap()
            .ends_with("# noneditable drift\n")
    );
    let invalid_validated = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-validate",
            "--run",
            invalid_root.join("run.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(invalid_validated["valid"], true);

    fs::remove_file(&fixture).expect("remove external agent fixture before replay");
    let replayed = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-replay",
            "--run",
            run_root.join("run.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(replayed["agentAccessed"], false);
    assert_eq!(replayed["modelAccessed"], false);
    assert_eq!(replayed["allFieldsValidated"], true);
    assert_eq!(replayed["independentRescoreMatched"], true);

    let invalid_replayed = data(&run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-replay",
            "--run",
            invalid_root.join("run.json").to_str().unwrap(),
        ],
    ));
    assert_eq!(invalid_replayed["agentAccessed"], false);
    assert_eq!(invalid_replayed["modelAccessed"], false);
    assert_eq!(invalid_replayed["allFieldsValidated"], true);
    assert!(invalid_replayed["independentRescoreMatched"].is_null());

    let tampered_invalid = temp.path().join("tampered-invalid-open-agent-run");
    copy_tree(&invalid_root, &tampered_invalid);
    fs::write(
        tampered_invalid.join("observed-workspace/package.toml"),
        "tampered\n",
    )
    .expect("tamper observed invalid workspace");
    let rejected = run_genesis(
        &root,
        &artifact,
        &[
            "bench",
            "agent-validate",
            "--run",
            tampered_invalid.join("run.json").to_str().unwrap(),
        ],
    );
    assert!(
        !rejected.status.success(),
        "tampered invalid workspace payload must fail closed"
    );
}

#[test]
fn retained_luna_campaign_replays_after_current_authorities_advance() {
    let root = support::repo_root();
    let temp = tempdir().expect("temporary historical replay root");
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let campaign = root
        .join("benchmarks/genesisbench/v0.1/campaigns/codex-luna-xhigh-2026-07-17/reality-gate");
    let cases = [
        "completion-small",
        "deployment-small",
        "generation-small",
        "package-migration-small",
        "performance-repair-small",
        "policy-minimization-small",
        "refactor-small",
        "repair-small",
        "replay-investigation-small",
    ];
    for case in cases {
        let run = campaign.join("runs").join(case).join("run.json");
        let validated = data(&run_genesis(
            &root,
            &artifact,
            &["bench", "agent-validate", "--run", run.to_str().unwrap()],
        ));
        assert_eq!(validated["valid"], true);
        assert_eq!(validated["outcome"], "invalid");

        let replayed = data(&run_genesis(
            &root,
            &artifact,
            &["bench", "agent-replay", "--run", run.to_str().unwrap()],
        ));
        assert_eq!(replayed["agentAccessed"], false);
        assert_eq!(replayed["modelAccessed"], false);
        assert_eq!(replayed["allFieldsValidated"], true);
        assert!(replayed["independentRescoreMatched"].is_null());
    }

    let report: Value = serde_json::from_slice(
        &fs::read(campaign.join("report.json")).expect("read retained campaign report"),
    )
    .expect("parse retained campaign report");
    assert_eq!(report["matrix"]["complete"], true);
    assert_eq!(report["summary"]["invalid"], 9);
    assert_eq!(report["summary"]["providerUnavailableForAccount"], true);
    assert_eq!(report["summary"]["modelExecutionObserved"], false);
    assert_eq!(report["expansion"]["allowed"], false);
}

#[test]
fn retained_available_luna_campaign_replays_with_typed_harness_defects() {
    let root = support::repo_root();
    let temp = tempdir().expect("temporary historical Luna replay root");
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let campaign = root.join(
        "benchmarks/genesisbench/v0.1/campaigns/\
         codex-gpt-5-6-luna-xhigh-2026-07-17/reality-gate",
    );
    let cases = [
        "completion-small",
        "deployment-small",
        "generation-small",
        "package-migration-small",
        "performance-repair-small",
        "policy-minimization-small",
        "refactor-small",
        "repair-small",
        "replay-investigation-small",
    ];
    for case in cases {
        let run = campaign.join("runs").join(case).join("run.json");
        let validated = data(&run_genesis(
            &root,
            &artifact,
            &["bench", "agent-validate", "--run", run.to_str().unwrap()],
        ));
        let expected = if case == "package-migration-small" {
            "verified"
        } else {
            "invalid"
        };
        assert_eq!(validated["outcome"], expected);

        let replay = run_genesis(
            &root,
            &artifact,
            &["bench", "agent-replay", "--run", run.to_str().unwrap()],
        );
        let archive_platform_matches = cfg!(all(target_os = "macos", target_arch = "aarch64"));
        if case == "package-migration-small" && !archive_platform_matches {
            assert!(!replay.status.success());
            assert!(
                String::from_utf8_lossy(&replay.stdout)
                    .contains("archived replay tool is incompatible with this host"),
                "unexpected replay refusal: {}",
                String::from_utf8_lossy(&replay.stdout),
            );
        } else {
            let replayed = data(&replay);
            assert_eq!(replayed["agentAccessed"], false);
            assert_eq!(replayed["modelAccessed"], false);
            assert_eq!(replayed["allFieldsValidated"], true);
            if case == "package-migration-small" {
                assert_eq!(replayed["independentRescoreMatched"], true);
            } else {
                assert!(replayed["independentRescoreMatched"].is_null());
            }
        }
    }

    let report: Value = serde_json::from_slice(
        &fs::read(campaign.join("report.json")).expect("read available Luna report"),
    )
    .expect("parse available Luna report");
    assert_eq!(report["summary"]["verified"], 1);
    assert_eq!(report["summary"]["invalid"], 8);
    assert_eq!(report["summary"]["modelExecutionObserved"], true);
    assert_eq!(report["summary"]["toolCacheContaminationObserved"], true);
    assert_eq!(
        report["summary"]["declaredEditableOutputRejectionObserved"],
        true
    );
    assert_eq!(report["summary"]["eventLineLimitObserved"], true);
    assert_eq!(report["expansion"]["allowed"], false);
    assert_eq!(
        report["expansion"]["reasonCodes"],
        serde_json::json!(["campaign/harness-defect", "campaign/invalid-attempts"])
    );
}

#[test]
fn retained_v3_luna_reality_gate_is_closed_and_harness_clean() {
    let root = support::repo_root();
    let temp = tempdir().expect("temporary v0.3 Luna validation root");
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let campaign = root.join(
        "benchmarks/genesisbench/v0.1/campaigns/\
         codex-gpt-5-6-luna-xhigh-harness-v0-3-2026-07-17/reality-gate",
    );
    let cases = [
        "completion-small",
        "deployment-small",
        "generation-small",
        "package-migration-small",
        "performance-repair-small",
        "policy-minimization-small",
        "refactor-small",
        "repair-small",
        "replay-investigation-small",
    ];
    for case in cases {
        let run = campaign.join("runs").join(case).join("run.json");
        let validated = data(&run_genesis(
            &root,
            &artifact,
            &["bench", "agent-validate", "--run", run.to_str().unwrap()],
        ));
        let expected = if case == "deployment-small" {
            "invalid"
        } else {
            "verified"
        };
        assert_eq!(validated["outcome"], expected);
    }

    let report: Value = serde_json::from_slice(
        &fs::read(campaign.join("report.json")).expect("read v0.3 Luna report"),
    )
    .expect("parse v0.3 Luna report");
    assert_eq!(report["summary"]["verified"], 8);
    assert_eq!(report["summary"]["invalid"], 1);
    assert_eq!(report["summary"]["ambientSkillDiscoveryObserved"], false);
    assert_eq!(report["summary"]["toolCacheContaminationObserved"], false);
    assert_eq!(
        report["summary"]["declaredEditableOutputRejectionObserved"],
        false
    );
    assert_eq!(report["summary"]["eventLineLimitObserved"], false);
    assert_eq!(report["expansion"]["allowed"], false);
    let deployment = report["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["caseId"] == "deployment-small")
        .expect("deployment attempt");
    assert_eq!(
        deployment["failureCodes"],
        serde_json::json!(["model/noneditable-input-drift"])
    );
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
    assert_eq!(report["controls"], 65);
}
