use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("gc_cli must live under <repository>/crates")
        .to_path_buf()
}

fn suite(root: &Path) -> Value {
    serde_json::from_slice(
        &fs::read(root.join("benchmarks/agent_tasks/v0.1/suite.json"))
            .expect("read task benchmark suite"),
    )
    .expect("parse task benchmark suite")
}

fn small_case<'a>(suite: &'a Value, task: &str) -> &'a Value {
    suite["cases"]
        .as_array()
        .expect("benchmark cases")
        .iter()
        .find(|case| case["taskClass"] == task && case["contextTier"] == "small")
        .unwrap_or_else(|| panic!("missing small benchmark case for {task}"))
}

fn copy_reference(root: &Path, case: &Value, destination: &Path) {
    let reference_root = root.join(case["referenceRoot"].as_str().expect("reference root"));
    for file in case["referenceFiles"].as_array().expect("reference files") {
        let relative = file["path"].as_str().expect("reference path");
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("create candidate parent");
        }
        fs::copy(reference_root.join(relative), target).expect("copy reference candidate");
    }
}

fn score_output(root: &Path, case_id: &str, candidate: &Path) -> Output {
    Command::new("python3")
        .current_dir(root)
        .arg("scripts/lib/gc_agent_scoring.py")
        .arg("--score")
        .arg("--case")
        .arg(case_id)
        .arg("--candidate")
        .arg(candidate)
        .arg("--genesis-bin")
        .arg(env!("CARGO_BIN_EXE_genesis"))
        .arg("--selfhost-artifact")
        .arg(root.join("selfhost/toolchain.gc"))
        .output()
        .expect("run model-agnostic scorer")
}

fn score(root: &Path, case: &Value, candidate: &Path) -> Value {
    let case_id = case["id"].as_str().expect("case id");
    let output = score_output(root, case_id, candidate);
    assert!(
        output.status.success(),
        "scorer failed for {case_id}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "scorer returned invalid JSON for {case_id}: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn dimension<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["dimensions"]
        .as_array()
        .expect("score dimensions")
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("missing score dimension {id}"))
}

#[test]
fn public_references_score_perfectly_and_deterministically_with_shipped_binary() {
    let root = repository_root();
    let suite = suite(&root);
    let scoring: Value = serde_json::from_slice(
        &fs::read(root.join("docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"))
            .expect("read scoring authority"),
    )
    .expect("parse scoring authority");
    let tasks = [
        "completion",
        "deployment",
        "generation",
        "package-migration",
        "performance-repair",
        "policy-minimization",
        "refactor",
        "repair",
        "replay-investigation",
    ];

    let mut repeated_completion = None;
    for task in tasks {
        let case = small_case(&suite, task);
        let workspace = tempfile::tempdir().expect("isolated scoring candidate");
        copy_reference(&root, case, workspace.path());
        let report = score(&root, case, workspace.path());
        assert_eq!(report["qualityScoreBasisPoints"], 10_000, "{task}");
        assert_eq!(report["validity"]["passed"], true, "{task}");
        assert_eq!(
            report["bindings"]["scorerRuntimeSha256"], scoring["implementation"]["runtimeSha256"],
            "{task} runtime binding"
        );
        assert_eq!(
            report["bindings"]["scorerContractSha256"], scoring["implementation"]["contractSha256"],
            "{task} contract binding"
        );
        assert_eq!(
            report["modelSpecificMetrics"],
            serde_json::json!({
                "includedInQualityScore": false,
                "recordedBy": "genesis/agent-benchmark-run-v0.1",
                "present": false
            })
        );
        for row in report["dimensions"].as_array().expect("score dimensions") {
            if row["applicable"] == true {
                assert_eq!(row["scoreBasisPoints"], 10_000, "{task}/{}", row["id"]);
            }
        }
        let serialized = serde_json::to_string(&report).expect("serialize score report");
        assert!(
            !serialized.contains(root.to_str().expect("UTF-8 repository path")),
            "score report leaked the repository path"
        );
        if task == "completion" {
            repeated_completion = Some((case.clone(), workspace, report));
        }
    }

    let (case, workspace, first) = repeated_completion.expect("completion report");
    let second = score(&root, &case, workspace.path());
    assert_eq!(
        first, second,
        "identical candidates must produce identical scores"
    );
}

#[test]
fn scoring_fails_closed_or_penalizes_independent_adversarial_candidates() {
    let root = repository_root();
    let suite = suite(&root);

    let completion = small_case(&suite, "completion");
    let wrong = tempfile::tempdir().expect("wrong-semantics candidate");
    copy_reference(&root, completion, wrong.path());
    fs::write(wrong.path().join("main.gc"), "(prim int/add 40 1)\n").expect("corrupt semantics");
    let wrong_report = score(&root, completion, wrong.path());
    assert_eq!(dimension(&wrong_report, "semantics")["scoreBasisPoints"], 0);
    assert_eq!(wrong_report["qualityScoreBasisPoints"], 0);
    assert_eq!(wrong_report["validity"]["passed"], false);

    let policy_case = small_case(&suite, "policy-minimization");
    let broad = tempfile::tempdir().expect("broad-policy candidate");
    copy_reference(&root, policy_case, broad.path());
    let caps = broad.path().join("caps.toml");
    let source = fs::read_to_string(&caps).expect("read candidate policy");
    fs::write(
        &caps,
        source.replace(
            "allow = [\"io/fs::read\"]",
            "allow = [\"io/fs::read\", \"io/fs::write\"]",
        ),
    )
    .expect("broaden candidate policy");
    let broad_report = score(&root, policy_case, broad.path());
    assert_eq!(
        dimension(&broad_report, "semantics")["scoreBasisPoints"],
        10_000
    );
    assert_eq!(
        dimension(&broad_report, "policy-scope")["scoreBasisPoints"],
        0
    );
    assert_eq!(broad_report["policy"]["scopeOk"], false);
    assert_eq!(broad_report["qualityScoreBasisPoints"], 0);

    let escaped = tempfile::tempdir().expect("scope-escape candidate");
    copy_reference(&root, completion, escaped.path());
    fs::write(escaped.path().join("notes.txt"), "undeclared output\n").expect("add extra file");
    let escaped_report = score(&root, completion, escaped.path());
    assert_eq!(escaped_report["patch"]["editableScopeOk"], false);
    assert_eq!(escaped_report["qualityScoreBasisPoints"], 0);
    assert!(
        escaped_report["validity"]["failedDimensions"]
            .as_array()
            .expect("failed dimensions")
            .contains(&Value::String("editable-scope".to_owned()))
    );

    let wasteful = tempfile::tempdir().expect("resource-heavy candidate");
    copy_reference(&root, completion, wasteful.path());
    let mut source = fs::read_to_string(wasteful.path().join("main.gc")).expect("read program");
    source.push_str("\n;");
    source.push_str(&"x".repeat(64 * 1024));
    source.push('\n');
    fs::write(wasteful.path().join("main.gc"), source).expect("inflate candidate source");
    let wasteful_report = score(&root, completion, wasteful.path());
    assert_eq!(
        dimension(&wasteful_report, "semantics")["scoreBasisPoints"],
        10_000
    );
    assert!(
        dimension(&wasteful_report, "resource-use")["scoreBasisPoints"]
            .as_u64()
            .expect("resource score")
            < 10_000
    );
    assert!(
        dimension(&wasteful_report, "patch-minimality")["scoreBasisPoints"]
            .as_u64()
            .expect("patch score")
            < 10_000
    );
    assert_eq!(wasteful_report["validity"]["passed"], true);
    assert!(
        wasteful_report["qualityScoreBasisPoints"]
            .as_u64()
            .expect("quality score")
            < 10_000
    );
}

#[cfg(unix)]
#[test]
fn scoring_rejects_symlink_candidates_before_execution() {
    use std::os::unix::fs::symlink;

    let root = repository_root();
    let suite = suite(&root);
    let completion = small_case(&suite, "completion");
    let candidate = tempfile::tempdir().expect("symlink candidate");
    let target = candidate.path().join("target.gc");
    fs::write(&target, "(prim int/add 40 2)\n").expect("write symlink target");
    symlink(&target, candidate.path().join("main.gc")).expect("create candidate symlink");

    let output = score_output(
        &root,
        completion["id"].as_str().expect("case id"),
        candidate.path(),
    );
    assert!(
        !output.status.success(),
        "symlink candidate must fail closed"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("symlink"),
        "symlink rejection must be explicit"
    );

    let root_target = tempfile::tempdir().expect("root symlink target");
    copy_reference(&root, completion, root_target.path());
    let root_parent = tempfile::tempdir().expect("root symlink parent");
    let root_link = root_parent.path().join("candidate");
    symlink(root_target.path(), &root_link).expect("create candidate-root symlink");
    let root_output = score_output(
        &root,
        completion["id"].as_str().expect("case id"),
        &root_link,
    );
    assert!(
        !root_output.status.success(),
        "symlink candidate root must fail closed"
    );
    assert!(
        String::from_utf8_lossy(&root_output.stderr).contains("symlink"),
        "candidate-root symlink rejection must be explicit"
    );
}
