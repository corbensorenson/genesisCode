use assert_cmd::cargo::cargo_bin_cmd;
use base64ct::{Base64, Encoding};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::tempdir;

mod support;

fn run(root: &Path, artifact: &Path, args: &[&str]) -> Output {
    cargo_bin_cmd!("genesis")
        .current_dir(root)
        .arg("--json")
        .arg("--selfhost-artifact")
        .arg(artifact)
        .args(args)
        .output()
        .expect("execute GenesisBench registry command")
}

fn data(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "command failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("parse CLI envelope");
    assert_eq!(envelope["ok"], true);
    envelope["data"].clone()
}

fn rejected(output: &Output, expected_code: &str) {
    assert!(
        !output.status.success(),
        "negative control unexpectedly passed"
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("parse rejection envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], expected_code);
}

fn seal(mut value: Value) -> Value {
    value["contentIdentitySha256"] = Value::String(String::new());
    let mut bytes = serde_json::to_vec(&value).expect("canonical JSON");
    bytes.push(b'\n');
    value["contentIdentitySha256"] = Value::String(format!("{:x}", Sha256::digest(bytes)));
    value
}

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON")).expect("parse JSON")
}

fn write(path: impl AsRef<Path>, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("pretty JSON");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("write JSON");
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create copied directory");
    for entry in fs::read_dir(source).expect("read copied directory") {
        let entry = entry.expect("read copied entry");
        let destination = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy registry artifact");
        }
    }
}

fn key_row(keygen: &Value, id: &str) -> Value {
    let public_b64 = keygen["pk_b64"].as_str().expect("public key");
    let public = Base64::decode_vec(public_b64).expect("decode public key");
    serde_json::json!({
        "id": id,
        "keyId": format!("sha256:{:x}", Sha256::digest(public)),
        "publicKeyBase64": public_b64,
        "provenance": format!("integration-test/{id}"),
    })
}

fn create_policy_and_claim(
    root: &Path,
    directory: &Path,
    operator: &Value,
    submitter: &Value,
) -> (PathBuf, PathBuf) {
    let profile = read(root.join("docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"));
    let reference = read(root.join("docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json"));
    let suite = read(root.join("benchmarks/agent_tasks/v0.1/suite.json"));
    let policy = seal(serde_json::json!({
        "kind": "genesis/genesisbench-registry-policy-v0.1",
        "version": "0.1.0",
        "registryId": "registry-integration",
        "protocol": {
            "id": profile["protocolId"],
            "version": profile["version"],
            "identitySha256": profile["contentIdentitySha256"],
        },
        "operator": operator,
        "submitters": [submitter],
        "admission": {
            "allowedTracks": ["open-agent"],
            "maxBundleBytes": 67108864,
            "rankPublicReferences": false,
        },
        "ranking": {
            "lexicographicKeys": [
                "verified-solve-rate-desc",
                "conditional-quality-desc",
                "capability-excess-asc",
                "context-bytes-asc",
                "tool-calls-asc",
                "repair-calls-asc"
            ],
            "costLatencyAffectRank": false,
            "completeEvaluationSetRequired": true,
            "tiesRemainTies": true,
        },
        "contentIdentitySha256": "",
    }));
    let mut lineages = suite["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["contextTier"] == "small")
        .map(|case| case["lineageId"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    lineages.sort();
    lineages.dedup();
    let lineage_value = serde_json::to_value(&lineages).unwrap();
    let mut lineage_bytes = serde_json::to_vec(&lineage_value).unwrap();
    lineage_bytes.push(b'\n');
    let claim = seal(serde_json::json!({
        "kind": "genesis/genesisbench-submission-claim-v0.1",
        "version": "0.1.0",
        "registryPolicyIdentitySha256": policy["contentIdentitySha256"],
        "evaluation": {
            "id": "evaluation-integration",
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
            "runtimeId": "deterministic-mock",
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
    let policy_path = directory.join("policy.json");
    let claim_path = directory.join("claim.json");
    write(&policy_path, &policy);
    write(&claim_path, &claim);
    (policy_path, claim_path)
}

#[test]
fn signed_registry_admits_rescores_rebuilds_and_rejects_history_tampering() {
    let root = support::repo_root();
    let temporary = tempdir().expect("temporary registry test");
    let artifact = support::copy_repo_toolchain_artifact(temporary.path());
    let operator_key = temporary.path().join("operator.toml");
    let submitter_key = temporary.path().join("submitter.toml");
    let wrong_operator_key = temporary.path().join("wrong-operator.toml");
    let operator = data(&run(
        &root,
        &artifact,
        &["keygen", "--out", operator_key.to_str().unwrap()],
    ));
    let submitter = data(&run(
        &root,
        &artifact,
        &["keygen", "--out", submitter_key.to_str().unwrap()],
    ));
    data(&run(
        &root,
        &artifact,
        &["keygen", "--out", wrong_operator_key.to_str().unwrap()],
    ));
    let operator_row = key_row(&operator, "operator");
    let submitter_row = key_row(&submitter, "submitter");
    let (policy, claim) =
        create_policy_and_claim(&root, temporary.path(), &operator_row, &submitter_row);

    let run_root = temporary.path().join("run");
    data(&run(
        &root,
        &artifact,
        &[
            "bench",
            "run",
            "--case",
            "generation-small",
            "--adapter",
            "benchmarks/genesisbench/v0.1/adapters/deterministic-mock.json",
            "--out",
            run_root.to_str().unwrap(),
        ],
    ));
    let bundle = temporary.path().join("result.gcbundle");
    data(&run(
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
    let outbox = temporary.path().join("outbox");
    let duplicate_payload = temporary.path().join("duplicate-payload.json");
    fs::write(&duplicate_payload, b"{\"a\":1,\"a\":2}\n").unwrap();
    rejected(
        &run(
            &root,
            &artifact,
            &[
                "bench",
                "__crypto-sign",
                "--payload",
                duplicate_payload.to_str().unwrap(),
                "--key",
                submitter_key.to_str().unwrap(),
                "--payload-type",
                "application/vnd.genesiscode.genesisbench-submission.v0.1+json",
            ],
        ),
        "bench/crypto-payload",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissive_key = temporary.path().join("permissive-submitter.toml");
        fs::copy(&submitter_key, &permissive_key).unwrap();
        fs::set_permissions(&permissive_key, fs::Permissions::from_mode(0o644)).unwrap();
        rejected(
            &run(
                &root,
                &artifact,
                &[
                    "bench",
                    "submit",
                    "--bundle",
                    bundle.to_str().unwrap(),
                    "--claim",
                    claim.to_str().unwrap(),
                    "--outbox",
                    temporary.path().join("permissive-outbox").to_str().unwrap(),
                    "--submitter",
                    "submitter",
                    "--key",
                    permissive_key.to_str().unwrap(),
                ],
            ),
            "bench/front-door-failed",
        );
    }
    let submitted = data(&run(
        &root,
        &artifact,
        &[
            "bench",
            "submit",
            "--bundle",
            bundle.to_str().unwrap(),
            "--claim",
            claim.to_str().unwrap(),
            "--outbox",
            outbox.to_str().unwrap(),
            "--submitter",
            "submitter",
            "--key",
            submitter_key.to_str().unwrap(),
        ],
    ));
    assert_eq!(submitted["transport"], "local-signed-immutable-outbox-v0.1");
    let submission = outbox.join(format!(
        "{}.submission.json",
        submitted["submissionIdentitySha256"].as_str().unwrap()
    ));

    let wrong_registry = temporary.path().join("wrong-registry");
    rejected(
        &run(
            &root,
            &artifact,
            &[
                "bench",
                "registry-init",
                "--registry",
                wrong_registry.to_str().unwrap(),
                "--policy",
                policy.to_str().unwrap(),
                "--operator-key",
                wrong_operator_key.to_str().unwrap(),
            ],
        ),
        "bench/front-door-failed",
    );
    assert!(!wrong_registry.exists());

    let registry = temporary.path().join("registry");
    let initialized = data(&run(
        &root,
        &artifact,
        &[
            "bench",
            "registry-init",
            "--registry",
            registry.to_str().unwrap(),
            "--policy",
            policy.to_str().unwrap(),
            "--operator-key",
            operator_key.to_str().unwrap(),
        ],
    ));
    assert_eq!(initialized["sequence"], 0);

    let mut forged = read(&submission);
    let signature = forged["envelope"]["signatures"][0]["sig"]
        .as_str()
        .unwrap()
        .to_owned();
    forged["envelope"]["signatures"][0]["sig"] = Value::String(format!(
        "{}{}",
        if &signature[..1] == "A" { "B" } else { "A" },
        &signature[1..]
    ));
    forged = seal(forged);
    let forged_path = temporary.path().join("forged-submission.json");
    write(&forged_path, &forged);
    rejected(
        &run(
            &root,
            &artifact,
            &[
                "bench",
                "registry-admit",
                "--registry",
                registry.to_str().unwrap(),
                "--submission",
                forged_path.to_str().unwrap(),
                "--bundle",
                bundle.to_str().unwrap(),
                "--operator-key",
                operator_key.to_str().unwrap(),
            ],
        ),
        "bench/front-door-failed",
    );

    let admitted = data(&run(
        &root,
        &artifact,
        &[
            "bench",
            "registry-admit",
            "--registry",
            registry.to_str().unwrap(),
            "--submission",
            submission.to_str().unwrap(),
            "--bundle",
            bundle.to_str().unwrap(),
            "--operator-key",
            operator_key.to_str().unwrap(),
        ],
    ));
    assert_eq!(admitted["decision"], "unranked");
    assert!(
        admitted["reasonCodes"]
            .as_array()
            .unwrap()
            .contains(&Value::String("task/public-reference".to_owned()))
    );

    let verified = data(&run(
        &root,
        &artifact,
        &[
            "bench",
            "registry-verify",
            "--registry",
            registry.to_str().unwrap(),
        ],
    ));
    assert_eq!(verified["events"], 1);
    assert_eq!(verified["checkpoints"], 2);
    assert_eq!(verified["historyComplete"], true);

    let site_a = temporary.path().join("site-a");
    let site_b = temporary.path().join("site-b");
    for site in [&site_a, &site_b] {
        data(&run(
            &root,
            &artifact,
            &[
                "bench",
                "registry-build",
                "--registry",
                registry.to_str().unwrap(),
                "--out",
                site.to_str().unwrap(),
            ],
        ));
    }
    assert_eq!(
        read(site_a.join("leaderboard.json")),
        read(site_b.join("leaderboard.json"))
    );
    let board = read(site_a.join("leaderboard.json"));
    assert!(
        board["cohorts"][0]["rankedSystems"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        board["cohorts"][0]["unrankedSystems"][0]["missingLineageIds"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(board["results"][0]["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(board["results"][0]["replay"]["modelAccessed"], false);

    let idempotent = data(&run(
        &root,
        &artifact,
        &[
            "bench",
            "registry-admit",
            "--registry",
            registry.to_str().unwrap(),
            "--submission",
            submission.to_str().unwrap(),
            "--bundle",
            bundle.to_str().unwrap(),
            "--operator-key",
            operator_key.to_str().unwrap(),
        ],
    ));
    assert_eq!(idempotent["idempotent"], true);
    assert_eq!(idempotent["sequence"], 1);

    let hidden_history = temporary.path().join("registry-hidden-history");
    copy_tree(&registry, &hidden_history);
    fs::write(hidden_history.join("events/hidden.event"), b"hidden\n").unwrap();
    rejected(
        &run(
            &root,
            &artifact,
            &[
                "bench",
                "registry-verify",
                "--registry",
                hidden_history.to_str().unwrap(),
            ],
        ),
        "bench/front-door-failed",
    );

    let rewritten_result = temporary.path().join("registry-rewritten-result");
    copy_tree(&registry, &rewritten_result);
    let result_path = fs::read_dir(rewritten_result.join("objects/results"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::OpenOptions::new()
        .append(true)
        .open(result_path)
        .unwrap()
        .write_all(b" \n")
        .unwrap();
    rejected(
        &run(
            &root,
            &artifact,
            &[
                "bench",
                "registry-verify",
                "--registry",
                rewritten_result.to_str().unwrap(),
            ],
        ),
        "bench/front-door-failed",
    );

    let deleted_event = temporary.path().join("registry-deleted-event");
    copy_tree(&registry, &deleted_event);
    let event = fs::read_dir(deleted_event.join("events"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::remove_file(event).unwrap();
    rejected(
        &run(
            &root,
            &artifact,
            &[
                "bench",
                "registry-verify",
                "--registry",
                deleted_event.to_str().unwrap(),
            ],
        ),
        "bench/front-door-failed",
    );
}

#[test]
fn registry_authorities_and_lexicographic_controls_are_current() {
    let root = support::repo_root();
    let output = std::process::Command::new("python3")
        .current_dir(&root)
        .args([
            "scripts/lib/genesisbench_registry.py",
            "check",
            "--self-test",
        ])
        .output()
        .expect("check registry authorities");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schemas"], 7);
    assert_eq!(report["controls"], 8);
}
