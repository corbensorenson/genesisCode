# GenesisCode Upgrade Plan - Open Red-Team Backlog (Self-Hosted + AI-First v1)

Last updated: 2026-02-20

This plan contains only unresolved findings from the latest fine-tooth-comb red-team pass.

Open checklist items: 1

## P0 - Trust and Self-Host Correctness

- [ ] P0.1 Remove Rust frontend execution paths from production binaries (parity-harness only).
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/frontend.rs:19`-`22` keeps `CoreformFrontend::Rust` in the primary frontend enum.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs:129`-`153` and `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs:878`-`929` still execute Rust parser/canonical/hash paths.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs:24`-`182` and peers (`cmd_gc.rs`, `cmd_refs.rs`, `cmd_sync.rs`, `cmd_vcs.rs`) still branch on `CoreformFrontend::Rust`.
  Acceptance:
  - Production CLI/WASI binaries compile and route only selfhost frontend/tool semantics.
  - Rust frontend code is compiled only in dedicated parity binaries/harness crates.
  - Add an enforceable guard script that fails if production `crates/*/src` references `CoreformFrontend::Rust`.
  Progress (2026-02-20):
  - Added enforcement gate `/Users/corbensorenson/Documents/genesisCode/scripts/check_no_production_rust_frontend_refs.sh`.
  - Wired gate into `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh`.
  - Removed direct `CoreformFrontend::Rust` branch references from high-risk production command routers:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_gc.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_refs.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_sync.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_vcs.rs`
  - Removed direct `CoreformFrontend::Rust` branch references from the main obligations library path:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs`
  - Added regression coverage in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/shell_gate_regressions.rs`

- [x] P0.2 Remove non-artifact bootstrap fallback from production runtime paths.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:35`-`37` allows non-artifact bootstrap by `debug_assertions`.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:423`-`427` still derives `Embedded` bootstrap mode.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs:831` and `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs:850` still preserve embedded fallback logic.
  Acceptance:
  - `genesis` and `genesis_wasi` enforce `artifact-only` in all non-parity profiles.
  - `artifact-preferred` and `embedded` are parity-harness-only code paths.
  - CLI/docs/spec behavior are unified with no debug-profile loophole in production binaries.
  Status (2026-02-20):
  - Done by moving non-artifact bootstrap gating from `debug_assertions` to explicit runtime profile wiring (`Production` vs `ParityHarness`).
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` now propagates profile mode to frontend/bootstrap guards via:
    - `gc_prelude::set_bootstrap_runtime_profile_parity_harness(...)`
    - `gc_obligations::set_frontend_runtime_profile_parity_harness(...)`
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs` now treats non-artifact modes as parity-harness-only.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs` and `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/frontend.rs` now use runtime-profile state instead of debug-build checks.
  - Regression coverage added/updated:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_coreform_frontend_profile.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasi_cli/tests/cli_coreform_frontend_profile.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/tests/selfhost_bootstrap_modes.rs`

- [x] P0.3 Fix false-green behavior in `check_hot_path_budgets.sh`.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh:162`-`166` captures `measure_ms` in command substitution; failures in measured commands are not fail-closed.
  - Repro from this pass:
    - Direct command with the script’s own caps policy fails:
      `genesis ... gcpm --caps ... lock --strict` -> `core/caps/denied` on `core/pkg-low::load-lock`.
    - `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh` still exits `0` and reports `hot-path-budgets: ok`.
  Acceptance:
  - Any failed measured command aborts the script with non-zero exit.
  - Add a regression test that injects a known failing subcommand and verifies hard failure.
  Status (2026-02-20):
  - Done via shared fail-closed measurement helper (`/Users/corbensorenson/Documents/genesisCode/scripts/lib/measure.sh`) and call-site migration in `/Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh`.
  - Regression coverage added in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/shell_gate_regressions.rs` (`measure_helper_fails_closed_on_command_error`, script wiring assertions).

- [x] P0.4 Fix the same measurement false-green pattern in `check_perf_budgets.sh`.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_perf_budgets.sh:68` and `/Users/corbensorenson/Documents/genesisCode/scripts/check_perf_budgets.sh:72`-`86` use command substitution around `measure_ms` without explicit fail-closed status handling.
  Acceptance:
  - Switch to explicit status-checked measurement flow (same hardening pattern used in `check_ai_iteration_slo.sh`).
  - Add a regression check that forces one measured command to fail and verifies script exit is non-zero.
  Status (2026-02-20):
  - Done by migrating `/Users/corbensorenson/Documents/genesisCode/scripts/check_perf_budgets.sh` to shared fail-closed measurement primitives.
  - Regression coverage added in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/shell_gate_regressions.rs`.

- [x] P0.5 Eliminate hard-gate bypass when `upgrade_plan.md` has open items.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh:65`-`68` returns success and skips all hard gates whenever open checklist items > 0.
  Acceptance:
  - Core hard gates always run in CI (regardless of backlog count).
  - Optional mode can keep local “defer heavy gates” behavior, but CI must never short-circuit.
  - Health script output clearly separates “plan backlog status” from “code health status”.
  Status (2026-02-20):
  - Done in `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh` by separating backlog status from code-health gating and enforcing gates whenever CI (or `GENESIS_HEALTH_ENFORCE_GATES=1`) is active.
  - Regression coverage added in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/shell_gate_regressions.rs` (`upgrade_plan_health_does_not_bypass_ci_gates_when_backlog_is_open`).

- [x] P0.6 Normalize gcpm low-level capability fixtures across perf/SLO scripts.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh:90`-`113` omits `core/pkg-low::load-lock`/`save-lock`/`env` from caps policy even though lock/env flows require them.
  - This drift already caused one real failure in `check_ai_iteration_slo.sh` this pass before fixing that script.
  Acceptance:
  - Single shared caps fixture generator is used by all gcpm perf/SLO scripts.
  - Fixture includes the full required low-level op closure for each measured command.
  - Any missing-op denial fails the owning script and corresponding test.
  Status (2026-02-20):
  - Done via `/Users/corbensorenson/Documents/genesisCode/scripts/lib/gcpm_caps_fixture.sh`.
  - Wired into `/Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh` and `/Users/corbensorenson/Documents/genesisCode/scripts/check_ai_iteration_slo.sh`.

## P1 - CI, Performance, and Hardware Coverage

- [x] P1.1 Close CI profile coverage gap for full selfhost/parity checks on PRs.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:35` maps PRs to `standard` profile; `full` runs only schedule/manual.
  - Full-only checks are currently deferred from most PRs:
    - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:191`-`192` (`selfhost_strict_golden.sh`)
    - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:215`-`218` (WASM cross-host determinism)
    - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:229`-`232` (web wasm smoke)
  Acceptance:
  - Add a required PR lane that exercises full-profile selfhost golden + wasm determinism (can be sharded/targeted).
  - Keep nightly full sweep, but prevent PR merge without at least one strict full-equivalence gate.
  Status (2026-02-20):
  - Done by adding `pr_strict_equivalence_gate` in `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml`, executed on every `pull_request`.
  - The lane now enforces both `bash scripts/selfhost_strict_golden.sh` and `node scripts/wasm_cross_host_determinism.mjs`.
  - Coverage guard added in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/ci_workflow_coverage.rs`.

- [x] P1.2 Add device-backed GPU compute benchmark enforcement in CI.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_runtime_bench/src/bench_gpu_compute.rs:57`-`91` defaults to deterministic fallback bridge unless `GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD` is set.
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_runtime_microbench_budgets.sh:57` records backend but does not require `device-bridge`.
  Acceptance:
  - Add dedicated GPU lane with `GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD` configured.
  - Enforce `gpu_compute_backend == "device-bridge"` in that lane.
  - Maintain separate budgets for fallback and real-device backends.
  Status (2026-02-20):
  - Done by adding `gpu_device_microbench` in `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml` on dedicated `runs-on: [self-hosted, linux, x64, gpu]` with required `GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD`.
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_runtime_microbench_budgets.sh` now supports backend requirement enforcement (`GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND`) and separate device/fallback budgets.
  - Regression coverage added in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/runtime_microbench_gpu_policy.rs` and `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/ci_workflow_coverage.rs`.

- [x] P1.3 Harden disk-headroom handling to avoid avoidable SLO/perf flakiness.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_disk_headroom.sh:8` hard-codes 1 GiB default minimum and `/Users/corbensorenson/Documents/genesisCode/scripts/check_disk_headroom.sh:65`-`68` hard-fails immediately.
  - During this pass, health/profile runs failed repeatedly near the threshold until manual `scripts/reclaim_build_space.sh --safe`.
  Acceptance:
  - Auto-attempt `reclaim_build_space.sh --safe` once before hard failure.
  - Emit pre/post free-space telemetry.
  - Keep strict fail for CI only after auto-reclaim retry fails.
  Status (2026-02-20):
  - Done in `/Users/corbensorenson/Documents/genesisCode/scripts/check_disk_headroom.sh` with one-pass safe reclaim retry, pre/post telemetry, and strict-mode controls (`--strict` / `GENESIS_DISK_STRICT_MODE`).
  - Regression coverage added in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/shell_gate_regressions.rs` (`disk_headroom_strict_and_non_strict_modes_behave_as_expected`).

- [x] P1.4 Tighten source-size budgets and split oversized production modules for AI-first editing.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/policies/source_size_budget.toml:5` sets `rust_max_lines = 4700`.
  - Current production file sizes from this pass:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs` = 4599 lines
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs` = 2538 lines
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` = 2041 lines
  Acceptance:
  - Reduce Rust budget target to AI-editable limits (e.g., <=2200, then <=1600).
  - Split top offenders into focused modules with stable interfaces.
  - Add per-file ownership/comments and contract tests to keep decomposition safe.
  Status (2026-02-20):
  - Done by enforcing `rust_max_lines = 2200` in `/Users/corbensorenson/Documents/genesisCode/policies/source_size_budget.toml`.
  - Split `stage2_wasm` helpers/tests into focused modules:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/planner_helpers.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/tests/mod.rs`
  - Split `gc_obligations` tests out of the production unit:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/tests/mod.rs`
  - Added module-ownership comments at split boundaries in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/planner_helpers.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs`
  - Verified by:
    - `cargo test -p gc_opt --quiet`
    - `cargo test -p gc_obligations --quiet`
    - `bash scripts/check_source_size_budget.sh`
  - Resulting top production file sizes now:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` = 2044 lines
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs` = 1991 lines
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs` = 1788 lines

## P2 - AI-First Architecture and Drift Control

- [x] P2.1 Remove duplicated Rust/selfhost command program builders to prevent semantic drift.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs:24`-`182` and `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs:183`+ duplicate large command construction logic across Rust and selfhost branches.
  - Similar frontend branching exists in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_gc.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_refs.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_sync.rs`, and `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_vcs.rs`.
  Acceptance:
  - Define one command-contract descriptor layer used by both parity and selfhost execution paths.
  - Program hash/kind/log-op derivation comes from shared descriptors, not duplicated branches.
  - Add drift tests asserting descriptor parity between execution backends.
  Status (2026-02-20):
  - Added shared command-contract descriptor modules:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/gc_contract.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/refs_contract.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/sync_contract.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/vcs_contract.rs`
  - Wired command groups to shared descriptors for kind/log-op derivation in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_gc.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_refs.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_sync.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_vcs.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs` (existing `pkg_contract` parity assertions preserved)
  - Added descriptor drift regressions in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/shell_gate_regressions.rs`
  - Added unit contract uniqueness/stability tests in each contract module.

## Execution Order (Recommended)

1. P0.3 -> P0.4 -> P0.6
2. P0.5 -> P1.3
3. P0.1 -> P0.2 -> P2.1
4. P1.1 -> P1.2 -> P1.4
