use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn ci_has_pr_strict_equivalence_gate_job() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(
        ci.contains("pr_strict_equivalence_gate:"),
        "ci workflow must define a strict PR equivalence gate job"
    );
    assert!(
        ci.contains("if: ${{ github.event_name == 'pull_request' }}"),
        "strict equivalence gate must run on pull_request"
    );
    assert!(
        ci.contains("bash scripts/selfhost_strict_golden.sh"),
        "strict equivalence gate must enforce selfhost strict golden"
    );
    assert!(
        ci.contains("node scripts/wasm_cross_host_determinism.mjs"),
        "strict equivalence gate must enforce wasm cross-host determinism"
    );
}

#[test]
fn workspace_test_step_holds_a_live_cargo_cache_lease() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    let test_step = ci.find("- name: Test\n").expect("workspace Test step");
    let changed_offset = ci[test_step..]
        .find("- name: Changed-File Fast Loop Budget")
        .expect("step after workspace Test");
    let body = &ci[test_step..test_step + changed_offset];
    assert!(body.contains("source scripts/lib/cargo_target_dir.sh"));
    assert!(
        body.contains("genesis_configure_cargo_target_dir \"$PWD\" \"workspace-test\" root-host")
    );
    assert!(
        body.contains(
            "trap 'genesis_clear_resolved_cargo_target_dir \"workspace-test-exit\"' EXIT"
        )
    );
}

#[test]
fn local_workspace_contract_has_an_isolated_hosted_job() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    let job_start = ci
        .find("  local_workspace_test_contract:")
        .expect("isolated local workspace contract job");
    let job_end = ci[job_start..]
        .find("  test:\n")
        .map(|offset| job_start + offset)
        .expect("job following local workspace contract");
    let body = &ci[job_start..job_end];

    assert_eq!(
        ci.matches("- name: Local Workspace Test Contract (CI unset)")
            .count(),
        1,
        "CI-unset workspace contract must have one authoritative lane"
    );
    assert!(body.contains("runs-on: ubuntu-latest"));
    assert!(body.contains("fetch-depth: 0"));
    assert!(body.contains("Local Workspace Disk Headroom"));
    assert!(body.contains("--min-kb 10485760 --strict 1"));
    assert!(body.contains("cargo fetch --locked"));
    assert!(
        body.contains(
            "env -u CI cargo test --workspace --profile selfhost-strict --locked --offline"
        )
    );
    assert!(
        !body.contains("Playwright") && !body.contains("test_perf_gates.sh"),
        "isolated contract job must not accumulate browser or performance artifacts"
    );

    let aggregate_end = ci[job_end..]
        .find("  pr_strict_equivalence_gate:")
        .map(|offset| job_end + offset)
        .expect("job following required aggregate");
    let aggregate = &ci[job_end..aggregate_end];
    assert!(aggregate.contains("if: ${{ always() }}"));
    assert!(aggregate.contains("- test_suite"));
    assert!(aggregate.contains("- local_workspace_test_contract"));
    assert!(aggregate.contains("TEST_SUITE_RESULT: ${{ needs.test_suite.result }}"));
    assert!(
        aggregate
            .contains("LOCAL_WORKSPACE_RESULT: ${{ needs.local_workspace_test_contract.result }}")
    );
    assert!(aggregate.contains("Required CI Aggregate"));
}

#[test]
fn ci_fetches_locked_evidence_dependencies_before_offline_baseline_guard() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    let fetch = ci
        .find("Fetch Offline Gate Dependencies")
        .expect("ci must declare the network-enabled evidence dependency fetch boundary");
    let baseline = ci
        .find("Signed Roadmap Baseline Guard")
        .expect("ci must run the signed roadmap baseline guard");
    assert!(
        fetch < baseline,
        "evidence dependencies must be fetched before the locked offline baseline guard"
    );
    for manifest in [
        "tools/genesis-evidence-producer/Cargo.toml",
        "tools/genesis-evidence-verifier/Cargo.toml",
    ] {
        assert!(
            ci.contains(&format!("cargo fetch --manifest-path {manifest} --locked")),
            "ci must fetch the locked standalone workspace: {manifest}"
        );
    }
}

#[test]
fn ci_has_gpu_device_microbench_lane() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(
        ci.contains("gpu_device_microbench:"),
        "ci workflow must define a gpu device microbench lane"
    );
    assert!(
        ci.contains("runs-on: [self-hosted, linux, x64, gpu]"),
        "gpu lane must target dedicated self-hosted gpu runners"
    );
    assert!(
        ci.contains("GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND: \"device-runtime\""),
        "gpu lane must require device-runtime backend"
    );
    assert!(
        ci.contains("GENESIS_RUNTIME_MICROBENCH_FEATURES: \"device-bridge\""),
        "gpu lane must compile runtime microbench with first-party device bridge feature"
    );
    assert!(
        ci.contains("bash scripts/update_gpu_compute_device_conformance_report.sh"),
        "gpu lane must run the explicit gpu device conformance producer"
    );
    assert!(
        ci.contains("Device Bridge Replay Determinism (Feature Gate)"),
        "gpu lane must verify replay determinism for device bridge mode"
    );
    assert!(
        ci.contains("gpu-device-conformance-artifacts-selfhosted-linux"),
        "gpu lane must upload adapter-aware device conformance artifacts"
    );
}

#[test]
fn ci_has_secondary_gpu_deterministic_lane_and_release_parity_gate() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(
        ci.contains("gpu_device_microbench_deterministic:"),
        "ci workflow must define a deterministic independent gpu conformance lane"
    );
    assert!(
        ci.contains("GENESIS_GPU_COMPUTE_DEVICE_RUNTIME_CMD: \"${{ github.workspace }}/scripts/gpu_device_runtime_deterministic.sh\""),
        "deterministic gpu lane must use first-party deterministic runtime bridge command"
    );
    assert!(
        ci.contains("gpu-device-conformance-artifacts-deterministic"),
        "deterministic gpu lane must upload independent conformance artifacts"
    );
    assert!(
        ci.contains("gpu_device_conformance_release_gate:"),
        "ci workflow must define a release gpu conformance parity gate"
    );
    assert!(
        ci.contains(
            "needs:\n      - gpu_device_microbench\n      - gpu_device_microbench_deterministic"
        ),
        "release parity gate must depend on both conformance lanes"
    );
    assert!(
        ci.contains("bash scripts/update_gpu_device_conformance_lane_parity_report.sh"),
        "release parity gate must retain validated conformance parity evidence"
    );
    assert!(
        ci.contains("gpu-device-conformance-lane-parity"),
        "release parity gate must upload parity report artifact"
    );
}

#[test]
fn ci_deduplicates_pr_strict_equivalence_and_enforces_gc_source_budget() {
    let root = repo_root();
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read .github/workflows/ci.yml");
    assert!(
        ci.contains("GC Source Size Budget Guard"),
        "ci workflow must enforce dedicated .gc source size budget guard"
    );
    assert!(
        ci.contains("bash scripts/check_gc_source_size_budget.sh"),
        "ci workflow must run .gc source size budget guard script"
    );
    assert!(
        ci.contains("Selfhost Refactor Guard"),
        "ci workflow must enforce selfhost refactor guard"
    );
    assert!(
        ci.contains("bash scripts/check_selfhost_refactor_guard.sh"),
        "ci workflow must run selfhost refactor guard script"
    );
    assert!(
        ci.contains("Test Execution Profile Matrix Guard"),
        "ci workflow must enforce explicit test execution profile matrix guard"
    );
    assert!(
        ci.contains("bash scripts/check_test_execution_profile_matrix.sh"),
        "ci workflow must run test execution profile matrix guard script"
    );
    assert!(
        ci.contains("GPU Conformance Lane Matrix Guard"),
        "ci workflow must enforce gpu conformance lane matrix guard"
    );
    assert!(
        ci.contains("bash scripts/check_gpu_conformance_lane_matrix.sh"),
        "ci workflow must run gpu conformance lane matrix guard script"
    );
    assert!(
        ci.contains("Fuzz + Differential Hardening Guard"),
        "ci workflow must enforce fuzz + differential hardening guard"
    );
    assert!(
        ci.contains("bash scripts/check_fuzz_differential_hardening.sh"),
        "ci workflow must run fuzz + differential hardening guard script"
    );
    assert!(
        ci.contains("GPU Compute Runtime Profile (Compute-Only)"),
        "ci workflow must enforce dedicated compute-only runtime profile gate"
    );
    assert!(
        ci.contains("bash scripts/update_gpu_compute_runtime_profile_report.sh"),
        "ci workflow must retain the validated compute-only runtime profile report"
    );
    assert!(
        ci.contains("GenesisCode Authoring Skill Guard"),
        "ci workflow must enforce genesiscode authoring skill drift guard"
    );
    assert!(
        ci.contains("bash scripts/check_genesiscode_authoring_skill.sh"),
        "ci workflow must run genesiscode authoring skill drift guard script"
    );
    assert!(
        ci.contains("Agent Authoring Bundle Guard"),
        "ci workflow must enforce agent authoring bundle drift/orphan guard"
    );
    assert!(
        ci.contains("bash scripts/check_agent_authoring_bundle.sh"),
        "ci workflow must run agent authoring bundle drift/orphan guard script"
    );
    assert!(
        ci.contains("Domain Kit Workflow Guard"),
        "ci workflow must enforce domain kit workflow migration guard"
    );
    assert!(
        ci.contains("bash scripts/check_domain_kit_workflows.sh"),
        "ci workflow must run domain kit workflow migration guard script"
    );
    assert!(
        ci.contains("Agent Generative Workload Gate"),
        "ci workflow must enforce generative agent workload validation gate"
    );
    assert!(
        ci.contains("bash scripts/check_agent_generative_workloads.sh"),
        "ci workflow must run generative agent workload validation script"
    );
    assert!(
        ci.contains("env.GENESIS_CI_PROFILE == 'full' && github.event_name != 'pull_request'"),
        "full-profile strict-equivalence checks in test job must skip pull_request to avoid duplication with pr_strict_equivalence_gate"
    );
}
