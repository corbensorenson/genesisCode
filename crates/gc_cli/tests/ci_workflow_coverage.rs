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
        ci.contains("bash scripts/check_gpu_compute_device_conformance.sh"),
        "gpu lane must run dedicated gpu device conformance checks"
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
        ci.contains("bash scripts/check_gpu_device_conformance_lane_parity.sh"),
        "release parity gate must compare conformance artifacts for contract parity"
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
        ci.contains("bash scripts/check_gpu_compute_runtime_profile.sh"),
        "ci workflow must run compute-only runtime profile guard script"
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
        ci.contains("env.GENESIS_CI_PROFILE == 'full' && github.event_name != 'pull_request'"),
        "full-profile strict-equivalence checks in test job must skip pull_request to avoid duplication with pr_strict_equivalence_gate"
    );
}
