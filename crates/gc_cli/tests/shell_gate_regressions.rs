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
    let hot = fs::read_to_string(root.join("scripts/render_hot_path_budgets_report.sh"))
        .expect("read render_hot_path_budgets_report.sh");
    let perf = fs::read_to_string(root.join("scripts/render_perf_budgets_report.sh"))
        .expect("read render_perf_budgets_report.sh");
    let slo = fs::read_to_string(root.join("scripts/render_ai_iteration_slo_report.sh"))
        .expect("read render_ai_iteration_slo_report.sh");
    let micro =
        fs::read_to_string(root.join("scripts/render_runtime_microbench_budgets_report.sh"))
            .expect("read render_runtime_microbench_budgets_report.sh");
    let gpu_profile =
        fs::read_to_string(root.join("scripts/render_gpu_compute_runtime_profile_report.sh"))
            .expect("read render_gpu_compute_runtime_profile_report.sh");
    let gpu_headroom = format!(
        "{}\n{}",
        fs::read_to_string(root.join("scripts/check_gpu_gfx_headroom_conformance.sh"))
            .expect("read check_gpu_gfx_headroom_conformance.sh"),
        fs::read_to_string(root.join("scripts/render_gpu_gfx_headroom_conformance_report.sh"))
            .expect("read render_gpu_gfx_headroom_conformance_report.sh")
    );
    let readiness =
        fs::read_to_string(root.join("scripts/render_selfhost_readiness_scorecard_report.sh"))
            .expect("read render_selfhost_readiness_scorecard_report.sh");
    let host_abi = fs::read_to_string(root.join("scripts/check_host_abi_conformance.sh"))
        .expect("read check_host_abi_conformance.sh");
    let strict_golden = fs::read_to_string(root.join("scripts/selfhost_strict_golden.sh"))
        .expect("read selfhost_strict_golden.sh");
    let wasm_cross = fs::read_to_string(root.join("scripts/wasm_cross_host_determinism.mjs"))
        .expect("read wasm_cross_host_determinism.mjs");
    let full_cross =
        fs::read_to_string(root.join("scripts/render_full_cross_host_profile_budget_report.sh"))
            .expect("read render_full_cross_host_profile_budget_report.sh");
    let generative =
        fs::read_to_string(root.join("scripts/render_agent_generative_workloads_report.sh"))
            .expect("read render_agent_generative_workloads_report.sh");
    let parity =
        fs::read_to_string(root.join("scripts/render_agent_workflow_runtime_parity_report.sh"))
            .expect("read render_agent_workflow_runtime_parity_report.sh");
    let gauntlet =
        fs::read_to_string(root.join("scripts/render_agent_reference_workflows_report.sh"))
            .expect("read render_agent_reference_workflows_report.sh");
    let health = fs::read_to_string(root.join("scripts/render_upgrade_plan_health_report.sh"))
        .expect("read render_upgrade_plan_health_report.sh");
    let scenario = fs::read_to_string(root.join("scripts/render_agent_scenario_perf_report.sh"))
        .expect("read render_agent_scenario_perf_report.sh");

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
        slo.contains("GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK"),
        "ai-iteration slo script must expose gcpm lock warm-up control for stabilization"
    );
    assert!(
        slo.contains("GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK"),
        "ai-iteration slo script must expose gcpm lock stabilization retries control"
    );
    assert!(
        micro.contains("CARGO_PROFILE=\"${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}\""),
        "runtime microbench script must default to release-equivalent cargo profile"
    );
    assert!(
        micro.contains("selfhost-strict|release|release-*|production|prod"),
        "runtime microbench script must map release/full profiles to strict GPU backend policy"
    );
    assert!(
        micro.contains("GPU_BACKEND_POLICY=\"require-device\"")
            && micro.contains("GPU_BACKEND_POLICY=\"dev-allow-fallback\""),
        "runtime microbench script must make require-device vs dev-allow-fallback policy split explicit"
    );
    assert!(
        micro.contains("REQUIRED_GPU_BACKEND=\"device-runtime\""),
        "runtime microbench script must require device-runtime backend when strict policy is active"
    );
    assert!(
        micro.contains("MICROBENCH_FEATURES=\"device-bridge\""),
        "runtime microbench script must enable device-bridge features in strict policy mode"
    );
    assert!(
        gpu_profile.contains("selfhost-strict|release|release-*|production|prod"),
        "gpu runtime profile script must map release/full profiles to strict GPU backend policy"
    );
    assert!(
        gpu_profile.contains("GPU_BACKEND_POLICY=\"require-device\"")
            && gpu_profile.contains("GPU_BACKEND_POLICY=\"dev-allow-fallback\""),
        "gpu runtime profile script must make require-device vs dev-allow-fallback policy split explicit"
    );
    assert!(
        gpu_profile.contains("REQUIRED_BACKEND=\"device-runtime\""),
        "gpu runtime profile script must require device-runtime backend when strict policy is active"
    );
    assert!(
        gpu_profile.contains("MICROBENCH_FEATURES=\"device-bridge\""),
        "gpu runtime profile script must enable device-bridge features in strict policy mode"
    );
    assert!(
        gpu_headroom.contains("GENESIS_GPU_GFX_HEADROOM_REQUIRE_DEVICE_LANE")
            && gpu_headroom.contains("GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REPORT")
            && gpu_headroom.contains("update_gpu_compute_device_conformance_report.sh"),
        "gpu headroom conformance must expose device-lane input controls and exact producer guidance"
    );
    assert!(
        gpu_headroom.contains("GENESIS_AGENT_GPU_REQUIRE_DEVICE=\"$require_device\""),
        "gpu headroom conformance script must run workflows with explicit require-device lane control"
    );
    assert!(
        readiness.contains("gpu-gfx-headroom-conformance:missing-lane-backend-modes")
            && readiness.contains("gpu-gfx-headroom-conformance:normal-lane-not-require-device"),
        "selfhost readiness scorecard must consume gpu headroom lane backend mode contract"
    );
    assert!(
        host_abi.contains("docs/policies/ffi_signed_runtime_caps_v0.1.toml")
            && host_abi.contains("extended_ffi")
            && host_abi.contains("untrusted_agent_safety"),
        "host abi conformance must validate signed ffi profile template and enforce abuse-case test lanes"
    );
    assert!(
        perf.contains("check_disk_headroom.sh")
            && perf.contains("--context \"perf-budgets\"")
            && perf.contains("--strict \"$DISK_STRICT_MODE\""),
        "perf script must enforce disk strict mode via check_disk_headroom"
    );
    assert!(
        hot.contains("check_disk_headroom.sh")
            && hot.contains("--context \"hot-path-budgets\"")
            && hot.contains("--strict \"$DISK_STRICT_MODE\""),
        "hot-path script must enforce disk strict mode via check_disk_headroom"
    );
    assert!(
        slo.contains("check_disk_headroom.sh")
            && slo.contains("--context \"ai-iteration-slo\"")
            && slo.contains("--strict \"$DISK_STRICT_MODE\""),
        "ai-iteration slo script must enforce disk strict mode via check_disk_headroom"
    );
    assert!(
        micro.contains("check_disk_headroom.sh")
            && micro.contains("--context \"runtime-microbench\"")
            && micro.contains("--strict \"$DISK_STRICT_MODE\""),
        "runtime microbench script must enforce disk strict mode via check_disk_headroom"
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
    assert!(
        full_cross.contains("GENESIS_FULL_CROSS_HOST_BASELINE_HISTORY"),
        "full cross-host lane must define a baseline history seed path"
    );
    assert!(
        full_cross.contains("--baseline-history \"$EFFECTIVE_BASELINE_HISTORY\""),
        "full cross-host lane must pass baseline history into shared budget helper"
    );
    assert!(
        full_cross.contains("--require-min-history"),
        "full cross-host lane must fail when there are insufficient history samples"
    );
    assert!(
        scenario.contains("GENESIS_AGENT_SCENARIO_BASELINE_HISTORY"),
        "agent scenario perf gate must define a baseline history seed path"
    );
    assert!(
        scenario.contains("GENESIS_AGENT_SCENARIO_REQUIRE_MIN_HISTORY"),
        "agent scenario perf gate must allow explicit minimum-history enforcement control"
    );
    assert!(
        scenario.contains("insufficient scenario history samples for enforcement"),
        "agent scenario perf gate must fail-closed on insufficient sample depth"
    );
    assert!(
        generative.contains("genesis/agent-generative-workloads-v0.1"),
        "generative workload gate must emit a stable machine-readable report kind"
    );
    assert!(
        parity.contains("render_agent_generative_workloads_report.sh"),
        "agent workflow runtime parity gate must include generative workload parity checks"
    );
    assert!(
        gauntlet.contains("source \"$ROOT_DIR/scripts/lib/agent_gpu_profile_contract.sh\""),
        "agent capability gauntlet must source shared agent gpu profile contract helper"
    );
    assert!(
        gauntlet.contains("genesis_apply_agent_gpu_profile_contract \"$GAUNTLET_PROFILE\" \"$AGENT_AUTOMATION_CONTEXT\""),
        "agent capability gauntlet must enforce agent gpu profile contract"
    );
    assert!(
        gauntlet.contains(
            "export GENESIS_GPU_BACKEND_POLICY_DEFAULT=\"$HEALTH_GPU_BACKEND_POLICY_DEFAULT\""
        ),
        "agent capability gauntlet must propagate resolved gpu backend policy default into runtime env"
    );
    assert!(
        gauntlet.contains("GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND")
            && gauntlet.contains("release-full|release|full-selfhost-cutover"),
        "agent capability gauntlet must default release/full profiles to require device-backed gpu lanes"
    );
    assert!(
        health.contains("check_agent_generative_workloads.sh"),
        "upgrade-plan health profiles must include generative workload validation beyond fixed workflow lists"
    );
    assert!(
        health.contains("check_write_genesiscode_skill_conformance.sh"),
        "upgrade-plan health profiles must include executable write_genesiscode_skill conformance validation"
    );
    assert!(
        health.contains("check_write_genesiscode_skill_distribution.sh"),
        "upgrade-plan health profiles must include write_genesiscode_skill distribution-kit validation"
    );
    assert!(
        health.contains("check_gpu_stack_decoupling.sh"),
        "upgrade-plan health profiles must include gpu/gfx stack decoupling topology validation"
    );
    assert!(
        health.contains("check_gfx_runtime_profile.sh"),
        "upgrade-plan health profiles must include gfx-only runtime profile lane validation"
    );
    assert!(
        health.contains("GENESIS_HEALTH_PROFILE_GATE_CACHE")
            && health.contains("GENESIS_HEALTH_PROFILE_GATE_CACHE_TTL_SEC"),
        "upgrade-plan health script must expose deterministic profile gate cache controls"
    );
    assert!(
        health.contains("apply_profile_gate_cache_policy")
            && health.contains("run_cached_health_gate.sh"),
        "upgrade-plan health script must wrap high-cost profile gates with cache policy helper"
    );
    assert!(
        health.contains("full-selfhost-cutover"),
        "upgrade-plan health script must expose dedicated full-selfhost-cutover profile"
    );
    assert!(
        health.contains("GENESIS_GPU_BACKEND_POLICY_DEFAULT")
            && health.contains("profile=$PROFILE"),
        "upgrade-plan health script must emit explicit gpu backend fallback policy defaults per profile"
    );
    assert!(
        health.contains("check_full_selfhost_cutover_profile.sh"),
        "release/full selfhost lanes must validate full-selfhost cutover profile contract"
    );
    assert!(
        health.contains("GENESIS_HEALTH_PREPUSH_BUDGET_MS:-480000"),
        "upgrade-plan health script must pin stricter prepush wall-time budget defaults"
    );
    assert!(
        health.contains("enforce_prepush_history_budget")
            && health.contains("GENESIS_HEALTH_PREPUSH_MIN_HISTORY")
            && health.contains("GENESIS_HEALTH_PREPUSH_HISTORY_SCOPE_KEY"),
        "upgrade-plan health script must enforce history-aware prepush runtime budgets"
    );
    assert!(
        health.contains("GENESIS_HEALTH_STRICT_DISK_POLICY:-fail")
            && health.contains("strict profile overrides GENESIS_HEALTH_STRICT_DISK_POLICY"),
        "strict profiles must default and resolve to fail-closed disk preflight policy"
    );
}

#[test]
fn changed_fast_supports_explicit_strict_disk_mode() {
    let root = repo_root();
    let changed_fast = fs::read_to_string(root.join("scripts/test_changed_fast.sh"))
        .expect("read test_changed_fast.sh");
    let slo = fs::read_to_string(root.join("scripts/render_ai_iteration_slo_report.sh"))
        .expect("read render_ai_iteration_slo_report.sh");

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
fn changed_fast_ambiguous_governance_change_escalates_without_empty_package_arg() {
    let root = repo_root();
    let changed_fast = fs::read_to_string(root.join("scripts/test_changed_fast.sh"))
        .expect("read test_changed_fast.sh");
    assert!(
        changed_fast.contains("GENESIS_TEST_CHANGED_FILES_OVERRIDE"),
        "test-changed-fast must support deterministic changed-file override for regression tests"
    );
    assert!(changed_fast.contains("scripts/lib/changed_impact.py"));

    let output = Command::new("bash")
        .arg("-lc")
        .arg(
            "GENESIS_CHANGED_BASE=HEAD \
             GENESIS_TEST_CHANGED_FILES_OVERRIDE=$'scripts/check_doc_hygiene.sh\\ndocs/INDEX.md' \
             bash scripts/test_changed_fast.sh --dry-run",
        )
        .current_dir(&root)
        .output()
        .expect("run test_changed_fast dry-run with non-crate override");
    assert!(
        output.status.success(),
        "expected dry-run to succeed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mode=profile-fallback"),
        "expected conservative profile fallback, got: {stdout}"
    );
    assert!(
        !stdout.contains("cargo test -p \n") && !stdout.contains("cargo test -p \r"),
        "dry-run must never emit empty cargo package args: {stdout}"
    );
    assert!(
        stdout.contains("bash scripts/test_fast_full.sh"),
        "ambiguous governance impact must escalate to the full fast profile: {stdout}"
    );
}

#[test]
#[ignore = "perf-gate"]
fn bootstrap_retirement_gate_has_explicit_local_degraded_mode() {
    let root = repo_root();

    let degraded = Command::new("bash")
        .arg("-lc")
        .arg(
            "GENESIS_BOOTSTRAP_RETIREMENT_STRICT_RELEASE=1 \
             GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE=1 \
             GENESIS_BOOTSTRAP_RETIREMENT_DISK_AUTO_RECLAIM=0 \
             GENESIS_RELEASE_GUARD_MIN_FREE_KB=999999999 \
             bash scripts/check_bootstrap_retirement_gate.sh",
        )
        .current_dir(&root)
        .output()
        .expect("run bootstrap retirement gate in local degraded mode");
    assert!(
        degraded.status.success(),
        "bootstrap retirement gate should succeed in explicit local degraded mode\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&degraded.stdout),
        String::from_utf8_lossy(&degraded.stderr)
    );
    let degraded_stdout = String::from_utf8_lossy(&degraded.stdout);
    let degraded_stderr = String::from_utf8_lossy(&degraded.stderr);
    assert!(
        degraded_stdout.contains("bootstrap-retirement-gate: degraded")
            || degraded_stderr.contains("bootstrap-retirement-gate: degraded"),
        "degraded mode must be explicit in output\nstdout:\n{}\nstderr:\n{}",
        degraded_stdout,
        degraded_stderr
    );

    let strict_fail = Command::new("bash")
        .arg("-lc")
        .arg(
            "GENESIS_BOOTSTRAP_RETIREMENT_STRICT_RELEASE=1 \
             GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE=0 \
             GENESIS_BOOTSTRAP_RETIREMENT_DISK_AUTO_RECLAIM=0 \
             GENESIS_RELEASE_GUARD_MIN_FREE_KB=999999999 \
             bash scripts/check_bootstrap_retirement_gate.sh",
        )
        .current_dir(&root)
        .output()
        .expect("run bootstrap retirement gate in strict local mode");
    assert!(
        !strict_fail.status.success(),
        "bootstrap retirement gate must fail without degraded local mode under constrained disk\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&strict_fail.stdout),
        String::from_utf8_lossy(&strict_fail.stderr)
    );
}

#[test]
#[ignore = "perf-gate"]
fn production_rust_frontend_guard_is_wired_and_passes() {
    let root = repo_root();
    let health = fs::read_to_string(root.join("scripts/render_upgrade_plan_health_report.sh"))
        .expect("read render_upgrade_plan_health_report.sh");
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
#[ignore = "perf-gate"]
fn upgrade_plan_health_does_not_bypass_ci_gates_when_backlog_is_open() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create open-backlog fixture");
    let plan = temp.path().join("upgrade_plan.md");
    fs::write(&plan, "Open checklist items: 1\n\n- [ ] fixture item\n")
        .expect("write open-backlog fixture");
    let output = Command::new("bash")
        .arg(root.join("scripts/check_upgrade_plan_health.sh"))
        .arg("--profile")
        .arg("dev-fast")
        .env("CI", "true")
        .env("GENESIS_HEALTH_ENFORCE_GATES", "1")
        .env("GENESIS_HEALTH_TEST_GATE_OVERRIDE", "true")
        .env("GENESIS_AGENT_AUTOMATION_CONTEXT", "0")
        .env("GENESIS_HEALTH_PLAN_FILE", &plan)
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
#[ignore = "perf-gate"]
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

#[test]
fn check_reclaim_controls_fail_closed_without_running_maintenance() {
    let root = repo_root();
    let disk = Command::new("bash")
        .arg(root.join("scripts/check_disk_headroom.sh"))
        .arg("--auto-reclaim")
        .arg("1")
        .current_dir(&root)
        .output()
        .expect("run disk reclaim rejection");
    assert_eq!(disk.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&disk.stderr).contains("checks are read-only"),
        "disk check must name the read-only boundary"
    );

    let runtime = Command::new("bash")
        .arg(root.join("scripts/check_runtime_backend_feature_matrix.sh"))
        .env("GENESIS_RUNTIME_BACKEND_MATRIX_AUTO_RECLAIM", "1")
        .current_dir(&root)
        .output()
        .expect("run runtime-matrix reclaim rejection");
    assert_eq!(runtime.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&runtime.stderr).contains("checks are read-only"),
        "runtime matrix must name the read-only boundary"
    );
}

#[test]
#[ignore = "perf-gate"]
fn changed_fast_defaults_to_temporary_metrics_and_ignores_legacy_output_env() {
    let root = repo_root();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let temp = std::env::temp_dir().join(format!(
        "genesis-changed-fast-boundary-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&temp).expect("create changed-fast boundary fixture");
    let changed = temp.join("changed.txt");
    let report = temp.join("legacy-report.json");
    let history = temp.join("legacy-history.jsonl");
    fs::write(&changed, "README.md\n").expect("write changed-file fixture");

    let output = Command::new("bash")
        .arg(root.join("scripts/test_changed_fast.sh"))
        .arg("--runner")
        .arg("cargo")
        .arg("--changed-files-from")
        .arg(&changed)
        .arg("--budget-ms")
        .arg("120000")
        .arg("--min-history")
        .arg("1")
        .env("GENESIS_TEST_CHANGED_REPORT", &report)
        .env("GENESIS_TEST_CHANGED_HISTORY", &history)
        .current_dir(&root)
        .output()
        .expect("run changed-fast temporary metrics probe");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "changed-fast temporary probe failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("report=temporary"));
    assert!(
        !report.exists(),
        "legacy report environment override was honored"
    );
    assert!(
        !history.exists(),
        "legacy history environment override was honored"
    );
    fs::remove_dir_all(&temp).expect("remove changed-fast boundary fixture");
}

#[test]
fn source_decomposition_report_canonicalizes_host_paths() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create source decomposition fixture");
    let policy = temp.path().join("policy.toml");
    let report = temp.path().join("report.json");
    fs::write(
        &policy,
        r#"version = 1
target_max_lines = 700
required_min_phase = "phase-1"
disallowed_statuses = ["planned", "blocked"]

[[tracked_over_budget_rows]]
module_path = "crates/gc_effects/src/runner_capability_dispatch.rs"
phase = "phase-1"
status = "waived"
parity_gate = '''python3 -c 'import pathlib, tempfile; print(pathlib.Path.home() / "private" / "input"); print(pathlib.Path(tempfile.gettempdir()) / "output.json")' '''
waiver_owner = "fixture"
waiver_scope = "portable diagnostic regression"
waiver_rationale = "fixture row exercises successful command output canonicalization"
waiver_review_by = "2099-12-31"
"#,
    )
    .expect("write source decomposition policy fixture");

    let output = Command::new("bash")
        .arg(root.join("scripts/render_source_decomposition_tracked_parity_report.sh"))
        .arg(&report)
        .arg(&policy)
        .env("GENESIS_SOURCE_DECOMPOSITION_REVIEW_DATE", "2026-07-10")
        .current_dir(&root)
        .output()
        .expect("render source decomposition fixture");
    assert!(
        output.status.success(),
        "source decomposition fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = fs::read_to_string(&report).expect("read source decomposition report");
    assert!(
        !report_text.contains("/Users/"),
        "report leaked a user path"
    );
    assert!(
        !report_text.contains("/var/folders/"),
        "report leaked a temporary path"
    );
    assert!(
        report_text.contains("<host-path>"),
        "report did not retain a portable path marker"
    );
}
