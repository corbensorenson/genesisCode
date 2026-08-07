use super::*;

#[test]
fn release_evidence_v02_partition_is_closed_and_adversarially_checked() {
    let root = repo_root();
    let policy = fs::read_to_string(root.join("policies/release_evidence_dag_v0.2.json"))
        .expect("read release evidence DAG policy");
    let schema = fs::read_to_string(root.join("docs/spec/RELEASE_EVIDENCE_DAG_v0.2.schema.json"))
        .expect("read release evidence DAG schema");
    let health = fs::read_to_string(root.join("scripts/render_upgrade_plan_health_report.sh"))
        .expect("read release health runner");
    let execution = fs::read_to_string(root.join("scripts/lib/release_evidence_execution.py"))
        .expect("read release evidence execution runner");
    let fanout = fs::read_to_string(root.join("scripts/lib/release_evidence_fanout.py"))
        .expect("read release evidence fanout runner");
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read release evidence workflow");
    for marker in [
        "genesis/release-evidence-dag-v0.2",
        "independent-matched-cohorts",
        "independent-odd-cohort",
        "producer-authored-verdict",
        "crossRunReuseAllowed",
    ] {
        assert!(
            policy.contains(marker),
            "missing DAG policy marker: {marker}"
        );
    }
    for marker in [
        "release-evidence-dag-v0.2.schema.json",
        "measuredWorkerWallMs",
        "oddSamplesRequired",
        "agent-gpu-strict",
    ] {
        assert!(
            schema.contains(marker),
            "missing DAG schema marker: {marker}"
        );
    }
    for marker in [
        "GENESIS_RELEASE_EVIDENCE_NODE_CLASS",
        "GENESIS_RELEASE_EVIDENCE_INPUT_ROOT",
        "GENESIS_RELEASE_EVIDENCE_EXPORT_ROOT",
        "GENESIS_RELEASE_EVIDENCE_FANOUT_TOKEN",
        "release_evidence_partial",
        "release_evidence_command_ids_sha256",
        "GENESIS_RELEASE_EVIDENCE_PHASE",
        "release_evidence_fanout.py",
    ] {
        assert!(
            health.contains(marker),
            "missing DAG runner marker: {marker}"
        );
    }
    for marker in [
        "genesis/release-evidence-worker-observation-v0.2",
        "genesis/release-evidence-aggregate-v0.2",
        "commandCoverageExact",
        "dependency_mirror.prove_network_denial(prefix, require_loopback=True)",
        "genesis/release-evidence-worker-start-v0.2",
        "require_initialized_output",
        "fanout consumer does not bind the cold-1 producer",
        "release aggregate has a missing or duplicate execution node",
        "precondition.update(",
        "artifactInventoryAtMeasuredStartSha256\": measured_inventory",
    ] {
        assert!(
            execution.contains(marker),
            "missing v0.2 execution control: {marker}"
        );
    }
    for marker in [
        "genesis/release-evidence-fanout-auth-v0.2",
        "same-run cold-1 fanout artifact",
        "fanout archive path is unsafe or duplicated",
        "another workflow run, attempt, or revision",
        "TRANSIENT_HTTP_STATUSES",
        "sleep_for_retry(deadline",
        "archive_path.unlink(missing_ok=True)",
    ] {
        assert!(
            fanout.contains(marker),
            "missing v0.2 fanout control: {marker}"
        );
    }
    for marker in [
        "release_evidence_cold_worker:",
        "release_evidence_warm_worker:",
        "release_evidence_invariant_worker:",
        "release_evidence_stress_worker:",
        "index: [1, 2, 3]",
        "Publish Same-Run Cold-1 Fanout",
        "Aggregate Release Evidence DAG",
        "initialize-worker",
        "orchestration/fanout.stderr.log",
        "Install Aggregate Rust",
        "Install Aggregate Node",
    ] {
        assert!(
            workflow.contains(marker),
            "missing v0.2 workflow topology: {marker}"
        );
    }
    let measured_boundary = execution
        .find("artifactInventoryAtMeasuredStartSha256\": measured_inventory")
        .expect("warm measured-boundary inventory capture");
    let setup_start = execution[measured_boundary..]
        .find("setup = run_phase(")
        .expect("measured setup after warm-boundary capture");
    assert!(
        setup_start > 0,
        "warm inventory must be resampled before measured setup"
    );

    for action in ["check", "self-test"] {
        let output = Command::new("python3")
            .arg(root.join("scripts/lib/release_evidence_dag.py"))
            .arg("--root")
            .arg(&root)
            .arg(action)
            .current_dir(&root)
            .output()
            .expect("run release evidence DAG validation");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let execution_self_test = Command::new("python3")
        .arg(root.join("scripts/lib/release_evidence_execution.py"))
        .arg("--root")
        .arg(&root)
        .arg("self-test")
        .current_dir(&root)
        .output()
        .expect("run release evidence execution self-test");
    assert!(
        execution_self_test.status.success(),
        "{}",
        String::from_utf8_lossy(&execution_self_test.stderr)
    );
}
