use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

#[test]
fn measure_helper_fails_closed_on_command_error() {
    let root = repo_root();
    let helper = root.join("scripts/lib/measure.sh");
    let status = Command::new("bash")
        .arg("-lc")
        .arg(format!(
            "source {}; genesis_measure_best_of_ms fail_case 0 1 false",
            helper.display()
        ))
        .current_dir(&root)
        .status()
        .expect("run fail-closed measure helper check");
    assert!(
        !status.success(),
        "measure helper unexpectedly succeeded for failing command"
    );
}

#[test]
fn perf_scripts_use_shared_fail_closed_primitives() {
    let root = repo_root();
    let hot = fs::read_to_string(root.join("scripts/check_hot_path_budgets.sh"))
        .expect("read check_hot_path_budgets.sh");
    let perf = fs::read_to_string(root.join("scripts/check_perf_budgets.sh"))
        .expect("read check_perf_budgets.sh");
    let slo = fs::read_to_string(root.join("scripts/check_ai_iteration_slo.sh"))
        .expect("read check_ai_iteration_slo.sh");
    let micro = fs::read_to_string(root.join("scripts/check_runtime_microbench_budgets.sh"))
        .expect("read check_runtime_microbench_budgets.sh");
    let strict_golden = fs::read_to_string(root.join("scripts/selfhost_strict_golden.sh"))
        .expect("read selfhost_strict_golden.sh");
    let wasm_cross = fs::read_to_string(root.join("scripts/wasm_cross_host_determinism.mjs"))
        .expect("read wasm_cross_host_determinism.mjs");
    let full_cross =
        fs::read_to_string(root.join("scripts/check_full_cross_host_profile_budget.sh"))
            .expect("read check_full_cross_host_profile_budget.sh");

    assert!(
        hot.contains("source \"$ROOT_DIR/scripts/lib/measure.sh\""),
        "hot-path script must use shared measure helper"
    );
    assert!(
        perf.contains("source \"$ROOT_DIR/scripts/lib/measure.sh\""),
        "perf script must use shared measure helper"
    );
    assert!(
        !hot.contains("$(measure_ms"),
        "hot-path script must not use command substitution around measure_ms"
    );
    assert!(
        !perf.contains("$(measure_ms"),
        "perf script must not use command substitution around measure_ms"
    );
    assert!(
        hot.contains("write_gcpm_low_caps_fixture \"$TMP_DIR/gcpm_caps.toml\""),
        "hot-path script must use shared gcpm caps fixture generator"
    );
    assert!(
        slo.contains("write_gcpm_low_caps_fixture \"$TMP_DIR/gcpm_caps.toml\""),
        "ai-iteration slo script must use shared gcpm caps fixture generator"
    );
    assert!(
        perf.contains("CARGO_PROFILE=\"${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}\""),
        "perf script must default to release-equivalent cargo profile"
    );
    assert!(
        hot.contains("CARGO_PROFILE=\"${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}\""),
        "hot-path script must default to release-equivalent cargo profile"
    );
    assert!(
        slo.contains("CARGO_PROFILE=\"${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}\""),
        "ai-iteration slo script must default to release-equivalent cargo profile"
    );
    assert!(
        micro.contains("CARGO_PROFILE=\"${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}\""),
        "runtime microbench script must default to release-equivalent cargo profile"
    );
    assert!(
        perf.contains("check_disk_headroom.sh --path \"$ROOT_DIR\" --context \"perf-budgets\" --strict \"$DISK_STRICT_MODE\""),
        "perf script must force strict disk mode"
    );
    assert!(
        hot.contains("check_disk_headroom.sh --path \"$ROOT_DIR\" --context \"hot-path-budgets\" --strict \"$DISK_STRICT_MODE\""),
        "hot-path script must force strict disk mode"
    );
    assert!(
        slo.contains("check_disk_headroom.sh --path \"$ROOT_DIR\" --context \"ai-iteration-slo\" --strict \"$DISK_STRICT_MODE\""),
        "ai-iteration slo script must force strict disk mode"
    );
    assert!(
        micro.contains("check_disk_headroom.sh --path \"$ROOT_DIR\" --context \"runtime-microbench\" --strict \"$DISK_STRICT_MODE\""),
        "runtime microbench script must force strict disk mode"
    );
    assert!(
        perf.contains("\"build_profile\": \"$CARGO_PROFILE\""),
        "perf script report must include build profile metadata"
    );
    assert!(
        hot.contains("\"build_profile\": \"$CARGO_PROFILE\""),
        "hot-path report must include build profile metadata"
    );
    assert!(
        slo.contains("\"build_profile\": profile"),
        "ai-iteration report must include build profile metadata field"
    );
    assert!(
        slo.contains("\"$CARGO_PROFILE\""),
        "ai-iteration report generator must pass active cargo profile into report writer"
    );
    assert!(
        micro.contains("GENESIS_RUNTIME_MICROBENCH_PROFILE=\"$CARGO_PROFILE\""),
        "runtime microbench runner must stamp profile metadata into report"
    );
    assert!(
        strict_golden.contains("profile_runtime_budget.py"),
        "strict-golden lane must emit runtime profile report via shared helper"
    );
    assert!(
        strict_golden.contains("--profile strict-golden"),
        "strict-golden lane must stamp strict-golden profile label"
    );
    assert!(
        wasm_cross.contains("profile_runtime_budget.py"),
        "wasm cross-host lane must emit runtime profile report via shared helper"
    );
    assert!(
        wasm_cross.contains("\"wasm-cross-host\""),
        "wasm cross-host lane must stamp wasm-cross-host profile label"
    );
    assert!(
        full_cross.contains("profile_runtime_budget.py"),
        "full cross-host lane must emit aggregate runtime profile report via shared helper"
    );
    assert!(
        full_cross.contains("--profile full-cross-host"),
        "full cross-host lane must stamp full-cross-host profile label"
    );
}

#[test]
fn changed_fast_supports_explicit_strict_disk_mode() {
    let root = repo_root();
    let changed_fast = fs::read_to_string(root.join("scripts/test_changed_fast.sh"))
        .expect("read test_changed_fast.sh");
    let slo = fs::read_to_string(root.join("scripts/check_ai_iteration_slo.sh"))
        .expect("read check_ai_iteration_slo.sh");

    assert!(
        changed_fast.contains("--strict-disk <mode>"),
        "test-changed-fast help must expose strict disk mode override"
    );
    assert!(
        changed_fast.contains("STRICT_DISK_MODE=\"${GENESIS_TEST_CHANGED_STRICT_DISK:-auto}\""),
        "test-changed-fast must support env-configured strict disk mode"
    );
    assert!(
        changed_fast.contains("check_disk_headroom.sh --path \"$ROOT_DIR\" --context \"test-changed-fast\" --strict \"$STRICT_DISK_MODE\""),
        "test-changed-fast must pass strict mode through to disk check"
    );
    assert!(
        slo.contains("--strict-disk \"$DISK_STRICT_MODE\""),
        "ai-iteration slo must call changed-fast in strict disk mode"
    );
}

#[test]
fn production_rust_frontend_guard_is_wired_and_passes() {
    let root = repo_root();
    let health = fs::read_to_string(root.join("scripts/check_upgrade_plan_health.sh"))
        .expect("read check_upgrade_plan_health.sh");
    assert!(
        health.contains("check_no_production_rust_frontend_refs.sh"),
        "upgrade-plan health script must run production rust frontend guard"
    );

    let status = Command::new("bash")
        .arg(root.join("scripts/check_no_production_rust_frontend_refs.sh"))
        .current_dir(&root)
        .status()
        .expect("run production rust frontend guard");
    assert!(
        status.success(),
        "production rust frontend guard unexpectedly failed"
    );
}

#[test]
fn command_groups_use_shared_contract_descriptors() {
    let root = repo_root();
    let cmd_gc =
        fs::read_to_string(root.join("crates/gc_cli_driver/src/cmd_gc.rs")).expect("read cmd_gc");
    let cmd_refs = fs::read_to_string(root.join("crates/gc_cli_driver/src/cmd_refs.rs"))
        .expect("read cmd_refs");
    let cmd_sync = fs::read_to_string(root.join("crates/gc_cli_driver/src/cmd_sync.rs"))
        .expect("read cmd_sync");
    let cmd_vcs =
        fs::read_to_string(root.join("crates/gc_cli_driver/src/cmd_vcs.rs")).expect("read cmd_vcs");

    assert!(cmd_gc.contains("gc_contract::kind(cmd)"));
    assert!(cmd_gc.contains("gc_contract::log_op(cmd)"));
    assert!(cmd_refs.contains("refs_contract::kind(cmd)"));
    assert!(cmd_refs.contains("refs_contract::log_op(cmd)"));
    assert!(cmd_sync.contains("sync_contract::kind(cmd)"));
    assert!(cmd_sync.contains("sync_contract::log_op(cmd)"));
    assert!(cmd_vcs.contains("vcs_contract::kind(cmd)"));
    assert!(cmd_vcs.contains("vcs_contract::log_op(cmd)"));
}

#[test]
fn upgrade_plan_health_does_not_bypass_ci_gates_when_backlog_is_open() {
    let root = repo_root();
    let output = Command::new("bash")
        .arg(root.join("scripts/check_upgrade_plan_health.sh"))
        .arg("--profile")
        .arg("dev-fast")
        .env("CI", "true")
        .env("GENESIS_HEALTH_ENFORCE_GATES", "1")
        .env("GENESIS_HEALTH_TEST_GATE_OVERRIDE", "true")
        .current_dir(&root)
        .output()
        .expect("run upgrade-plan health script");
    assert!(
        output.status.success(),
        "upgrade-plan health script failed unexpectedly\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("code health gates enforced despite backlog"),
        "expected CI enforcement message when backlog is open\nstdout:\n{}",
        stdout
    );
}

#[test]
fn disk_headroom_strict_and_non_strict_modes_behave_as_expected() {
    let root = repo_root();
    let script = root.join("scripts/check_disk_headroom.sh");

    let non_strict = Command::new("bash")
        .arg(&script)
        .arg("--path")
        .arg(".")
        .arg("--context")
        .arg("disk-test-nonstrict")
        .arg("--min-kb")
        .arg("999999999")
        .arg("--auto-reclaim")
        .arg("0")
        .arg("--strict")
        .arg("0")
        .current_dir(&root)
        .status()
        .expect("run disk headroom non-strict check");
    assert!(
        non_strict.success(),
        "disk headroom check should continue in non-strict mode when below threshold"
    );

    let strict = Command::new("bash")
        .arg(&script)
        .arg("--path")
        .arg(".")
        .arg("--context")
        .arg("disk-test-strict")
        .arg("--min-kb")
        .arg("999999999")
        .arg("--auto-reclaim")
        .arg("0")
        .arg("--strict")
        .arg("1")
        .current_dir(&root)
        .status()
        .expect("run disk headroom strict check");
    assert!(
        !strict.success(),
        "disk headroom check should fail in strict mode when below threshold"
    );
}
