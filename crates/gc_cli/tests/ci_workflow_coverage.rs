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
        ci.contains("GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND: \"device-bridge\""),
        "gpu lane must require device-bridge backend"
    );
    assert!(
        ci.contains("bash scripts/check_runtime_microbench_budgets.sh"),
        "gpu lane must run runtime microbench budget checks"
    );
}
