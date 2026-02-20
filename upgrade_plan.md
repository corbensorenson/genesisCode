# GenesisCode Upgrade Plan - Open Red-Team Backlog (Self-Hosted + AI-First v1)

Last updated: 2026-02-20

This file contains only unresolved roadblocks from a fresh full-project red-team pass.

Open checklist items: 2

## P0 - Self-Host Safety and Correctness Blockers

- [x] P0.1 Reconcile CLI help surface with parse surface, then gate it in CI.
  Evidence:
  - CLI help is now explicit and agent-readable for accepted frontend/engine values in both production and parity profiles (`crates/gc_cli_driver/src/cli_args.rs`).
  - Help-surface guard now validates canonical accepted-value text for top-level + `fmt --help` on native and WASI binaries (`scripts/check_production_cli_help_surface.sh`), and passes.
  - CI now enforces help-surface guard (`.github/workflows/ci.yml`), and upgrade-plan health common gates include it (`scripts/check_upgrade_plan_health.sh`).
  Acceptance:
  - Production/parity `--help` surface is explicitly agent-readable for engine/frontend accepted values.
  - `scripts/check_production_cli_help_surface.sh` matches the canonical help contract and passes.
  - CI + `scripts/check_upgrade_plan_health.sh` include help-surface guard.

- [x] P0.2 Remove panic-on-invariant paths (`unreachable!`) from user-path runtime/dispatch.
  Evidence:
  - Removed all non-test `unreachable!` macros from production crates (verified by `rg -n "unreachable!\\(" crates --glob '!**/tests/**' --glob '!**/benches/**'` returning no matches).
  - Replaced dispatch fallthrough panics with deterministic sealed errors in package/VCS low-level dispatch modules:
    - `crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs`
    - `crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs`
    - `crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs`
    - `crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs`
    - `crates/gc_effects/src/runner_cap_vcs_low/dispatch_snapshot.rs`
    - `crates/gc_effects/src/runner_cap_vcs_low/dispatch_patch_contract.rs`
  - Added negative no-panic regression tests for malformed routing:
    - `unsupported_pkg_low_op_eff_returns_sealed_error_instead_of_panicking`
    - `unsupported_vcs_low_op_eff_returns_sealed_error_instead_of_panicking`
  - Extended panic guard to explicitly reject any production `unreachable!` macro reintroduction:
    - `scripts/check_no_user_panics.sh` now fails on non-test `unreachable!` occurrences.
  - Also removed internal dispatch-drift panics in CLI/effects/registry/kernel/optimizer paths and replaced them with deterministic internal errors:
    - `crates/gc_cli_driver/src/cmd_pkg.rs`
    - `crates/gc_cli_driver/src/cmd_store.rs`
    - `crates/gc_cli_driver/src/cmd_vcs.rs`
    - `crates/gc_cli_driver/src/selfhost_frontend.rs`
    - `crates/gc_effects/src/log.rs`
    - `crates/gc_effects/src/runner_response_budget.rs`
    - `crates/gc_registry/src/lib.rs`
    - `crates/gc_kernel/src/eval.rs`
    - `crates/gc_kernel/src/compiled.rs`
    - `crates/gc_opt/src/stage2_wasm/callable_emit.rs`
    - `crates/gc_obligations/src/obligation_cache.rs`
    - `crates/gc_obligations/src/obligation_exec.rs`
    - `crates/gc_prelude/src/selfhost_coreform_v1.rs`
  Acceptance:
  - Replace user-path `unreachable!` with deterministic sealed/structured errors.
  - Add negative tests that malformed op routing and unsupported states never panic.
  - Extend panic guard policy to reject new `unreachable!` in production user paths.

- [ ] P0.3 Ship a first-party device-backed GPU compute bridge, not fallback-only.
  Evidence:
  - Current runtime microbench reports `gpu_compute_backend = "deterministic-fallback"` (`.genesis/perf/runtime_microbench_metrics.json:7`).
  - GPU bench defaults to generated shell+python fallback unless external env var points at a custom bridge command (`crates/gc_runtime_bench/src/bench_gpu_compute.rs:22`-`42`, `:57`-`91`).
  Acceptance:
  - In-repo device bridge implementation (not external ad-hoc command) for native GPU compute.
  - Capability policy profiles for deterministic fallback vs device mode.
  - End-to-end tests for both backends and replay/evidence behavior.
  - Runtime bench can require `device-bridge` in configured environments.

## P1 - Performance and Reliability

- [x] P1.1 Tighten AI iteration SLO budgets and reduce core-suite latency.
  Evidence:
  - Tightened default SLO budgets in `scripts/check_ai_iteration_slo.sh`:
    - `incremental_warm_ms`: `60000 -> 5000`
    - `changed_fast_ms`: `300000 -> 15000`
    - `core_suite_ms`: `300000 -> 45000`
    - `gcpm_lock_ms`: `20000 -> 5000`
    - `gcpm_env_ms`: `15000 -> 1000`
  - Added warm-build core-suite measurement path (`run_core_suite --no-run` before timing) to track iteration-time latency rather than cold compile cost.
  - Added history + p95 regression gating in `scripts/check_ai_iteration_slo.sh`:
    - persistent history file `.genesis/perf/ai_iteration_slo_history.jsonl`
    - per-metric baseline p95 tracking
    - percentage-based regression threshold (`GENESIS_AI_ITERATION_SLO_REGRESSION_PERCENT`, default `20%`) with minimum sample gate (`GENESIS_AI_ITERATION_SLO_MIN_HISTORY`, default `5`).
  - Latest run after changes: `core_suite_ms = 2744`, `changed_fast_ms = 3346`, `incremental_warm_ms = 834` (`.genesis/perf/ai_iteration_slo_metrics.json`).
  Acceptance:
  - Set realistic, regression-sensitive SLO budgets for warm loop, changed-fast, and core suite.
  - Reduce `core_suite_ms` materially (target <= 45000 on reference machine/profile).
  - Add p95 history/regression checks (percentage-based guard, not just static max).

- [x] P1.2 Burn down oversized test debt allowlist to zero.
  Evidence:
  - Split oversized suites into focused modules:
    - `crates/gc_effects/tests/sync_registry.rs` + `sync_registry_cases_a.rs` + `sync_registry_cases_b.rs`
    - `crates/gc_opt/src/stage2_wasm/tests/mod.rs` + `tail_cases.rs`
    - `crates/gc_wasi_cli/tests/cli_eval_gates.rs` + `cli_eval_gates_tail.rs`
    - `crates/gc_wasi_cli/tests/cli_selfhost_only.rs` + `cli_selfhost_only_tail.rs`
    - `crates/gc_cli/tests/cli_selfhost_only.rs` + `cli_selfhost_only_tail.rs`
  - Primary suites now meet target (<= 1000 lines), with highest at exactly `1000`:
    - `crates/gc_wasi_cli/tests/cli_eval_gates.rs` (1000)
    - `crates/gc_opt/src/stage2_wasm/tests/mod.rs` (984)
    - `crates/gc_wasi_cli/tests/cli_selfhost_only.rs` (890)
    - `crates/gc_effects/tests/sync_registry.rs` (883)
    - `crates/gc_cli/tests/cli_selfhost_only.rs` (872)
  - Cleared `target_debt_allowlist` in `policies/test_size_budget.toml`.
  - Verified with `bash scripts/check_test_size_budget.sh` -> `test-size-budget: ok`.
  - Targeted regressions pass for touched suites:
    - `cargo test -p gc_effects --test sync_registry --quiet`
    - `cargo test -p gc_wasi_cli --test cli_eval_gates --quiet`
    - `cargo test -p gc_wasi_cli --test cli_selfhost_only --quiet`
    - `cargo test -p gc_cli --test cli_selfhost_only --quiet`
    - `cargo test -p gc_opt stage2_wasm::tests --quiet`
  Acceptance:
  - Split these suites into focused modules <= 1000 lines each.
  - Remove all entries from the test-size debt allowlist while preserving coverage.

- [x] P1.3 Continue modular decomposition of high-churn production files for agent maintainability.
  Evidence:
  - Completed prior passes:
    - `crates/gc_registry/src/lib.rs` split into domain-focused units:
      - `crates/gc_registry/src/registry/types_and_client.rs` (246)
      - `crates/gc_registry/src/registry/client_impl.rs` (771)
      - `crates/gc_registry/src/registry/remote_helpers.rs` (136)
      - `crates/gc_registry/src/registry/file_backend.rs` (258)
    - `crates/gc_effects/src/runner_remote_ops.rs` split into domain-focused units:
      - `crates/gc_effects/src/runner_remote_ops/policy_auth.rs` (453)
      - `crates/gc_effects/src/runner_remote_ops/sync_closure_parallel.rs` (426)
      - `crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs` (483)
      - `crates/gc_effects/src/runner_remote_ops/gpk.rs` (76)
    - `crates/gc_obligations/src/lib.rs` split into focused units:
      - `crates/gc_obligations/src/obligations/types_api.rs` (438)
      - `crates/gc_obligations/src/obligations/frontend_module_ops.rs` (341)
      - `crates/gc_obligations/src/obligations/manifest_hashing.rs` (356)
      - `crates/gc_obligations/src/obligations/test_exec.rs` (352)
  - Completed this pass:
    - `crates/gc_cli_driver/src/cmd_pkg.rs` reduced from 1421 -> 449 lines by extracting frontend dispatch modules:
      - `crates/gc_cli_driver/src/cmd_pkg/frontend_dispatch.rs` (19)
      - `crates/gc_cli_driver/src/cmd_pkg/frontend_dispatch/rust.rs` (169)
      - `crates/gc_cli_driver/src/cmd_pkg/frontend_dispatch/selfhost.rs` (822)
    - Split test helper files moved under per-suite subdirectories to avoid accidental standalone integration-test crates:
      - `crates/gc_effects/tests/sync_registry/cases_a.rs`
      - `crates/gc_effects/tests/sync_registry/cases_b.rs`
      - `crates/gc_wasi_cli/tests/cli_eval_gates/tail.rs`
      - `crates/gc_wasi_cli/tests/cli_selfhost_only/tail.rs`
      - `crates/gc_cli/tests/cli_selfhost_only/tail.rs`
    - Poison-path selfhost parity fixture hardened to remain valid across module layout changes by symbol-driven mutation:
      - `crates/gc_cli/tests/cli_pkg_engine.rs`
      - `crates/gc_wasi_cli/tests/cli_pkg_engine.rs`
  - Validation:
    - `bash scripts/check_source_size_budget.sh` -> ok
    - `bash scripts/check_test_size_budget.sh` -> ok
    - `cargo test -p gc_cli_driver --quiet --no-run` -> ok
    - `cargo test -p gc_cli --test cli_pkg_engine --quiet` -> ok
    - `cargo test -p gc_wasi_cli --test cli_pkg_engine --quiet` -> ok
    - `cargo test -p gc_effects --test sync_registry --quiet` -> ok
    - `cargo test -p gc_registry --quiet` -> ok
    - `cargo test -p gc_obligations --quiet` -> ok
    - `cargo test -p gc_cli --test cli_selfhost_only --quiet` -> ok
    - `cargo test -p gc_wasi_cli --test cli_eval_gates --quiet` -> ok
    - `cargo test -p gc_wasi_cli --test cli_selfhost_only --quiet` -> ok
  Acceptance:
  - Split by capability/domain boundaries into smaller modules (target <= 900 lines/module).
  - Keep behavior/JSON envelopes stable; enforce with existing tests and schema checks.

## P2 - AI-First Authoring Surface

- [x] P2.1 Add machine-readable CLI introspection for agents (not help-text scraping).
  Evidence:
  - Added `genesis cli-schema` command with stable output kind `genesis/cli-schema-v0.1` and versioned schema payload (`schema = genesis/cli-schema-v0.1`) via `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_schema.rs`.
  - Schema output includes recursive command/options/defaults plus profile-specific allowed values for `engine` and `coreform-frontend` (`production` => `selfhost`, `parity-harness` => `selfhost|rust`).
  - Wired command into CLI dispatch and selfhost-only enforcement surface:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_args.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs`
  - Added schema contract docs + registry updates:
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI_SCHEMA_v0.1.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI_JSON_SCHEMAS_v0.1.md`
  - Added CI-covered integration tests:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_cli_schema.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasi_cli/tests/cli_cli_schema.rs`
    - updated `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_json_schema_registry.rs`
  Acceptance:
  - Add a stable command (e.g. `genesis cli-schema --json`) that emits commands/options/defaults/profile-specific allowed values.
  - Version the schema and validate in CI.

- [ ] P2.2 Add end-to-end agent reference workflows that combine package, VCS, effects, and GPU/task features.
  Evidence:
  - Current `examples/` are useful but mostly isolated demos (`effects_demo`, `gfx_demos`, `hello_pkg`, `selfhost_tools`) with no full-stack workflow fixture tying gcpm + vcs + task + gpu together.
  Acceptance:
  - Add at least 2 agent-grade reference projects with deterministic scripts:
    - compute-heavy workflow (gpu/task + package + evidence)
    - app/service workflow (effects + package + publish/install + replay)
  - Add CI smoke runs for these references under selfhost-only mode.

## Recommended Execution Order

1. P0.1 -> P0.2 -> P0.3
2. P1.1 -> P1.2 -> P1.3
3. P2.1 -> P2.2
