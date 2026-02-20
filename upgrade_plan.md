# GenesisCode Upgrade Plan - Red-Team Backlog (Self-Hosted v1)

Last updated: 2026-02-19

This file tracks active and recently completed work from the current red-team pass.
Completed items are retained as checked entries for auditability.

Open checklist items: 4

## P0 - Release/CI Blockers

- [x] P0.1 Repair guard scripts broken by runner modularization.
  Evidence:
  `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_selfhost_boundary.sh` fails with `semantic token added in non-approved file: crates/gc_effects/src/runner_cap_pkg_low.rs`.
  `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_host_abi_conformance.sh` fails with `no implementation ops detected in call_capability dispatch`.
  `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_runner_high_level_op_guard.sh` fails with `no capability ops found in runner dispatch`.
  `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_prelude_capability_coverage.sh` fails with `no gfx/gpu-compute/editor ops found in runner dispatch`.
  Acceptance:
  all four scripts pass on clean HEAD and in CI.
  Status (2026-02-19): complete locally; all four scripts pass:
  `check_selfhost_boundary.sh`, `check_host_abi_conformance.sh`, `check_runner_high_level_op_guard.sh`, `check_prelude_capability_coverage.sh`.

- [ ] P0.2 Remove (or explicitly reclassify) CoreForm semantic logic currently living in effect-runner package ops.
  Evidence:
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs:450`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs:461`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs:472`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs:1726`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs:1737`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_cap_pkg_low.rs:1748`
  Acceptance:
  boundary policy is explicit and enforced: either semantic parsing/canonicalization/hashing moves to selfhost `.gc` modules, or the boundary spec/allowlists are updated with a signed rationale and matching tests.

- [x] P0.3 Fix `gc_cli` integration regressions introduced by stricter selfhost artifact enforcement.
  Evidence:
  `cargo test -p gc_cli --test cli_smoke -- --nocapture` fails at:
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_smoke.rs:70` (`fmt_check_is_idempotent_on_fixture`)
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_smoke.rs:285` (`run_and_replay_roundtrip_effect_program`)
  with exit code `50` requiring explicit `--selfhost-artifact`.
  Acceptance:
  `cargo test -p gc_cli --test cli_smoke` passes.
  Status (2026-02-19): complete locally; `cargo test -p gc_cli --test cli_smoke --quiet` passes.

- [x] P0.4 Eliminate legacy semantic fallback in selfhost-only gcpm workflow executed via `run`.
  Evidence:
  `cargo test -p gc_cli --quiet` fails test:
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_gcpm_selfhost_acceptance.rs:135`
  error:
  `selfhost-only mode detected legacy semantic fallback while running run: core/pkg::init, core/pkg::install, core/pkg::lock`
  fallback detector:
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:425`
  wrapper aliases still point at high-level ops:
  `/Users/corbensorenson/Documents/genesisCode/prelude/modules/00_core.gc:721`
  `/Users/corbensorenson/Documents/genesisCode/prelude/modules/00_core.gc:731`
  `/Users/corbensorenson/Documents/genesisCode/prelude/modules/00_core.gc:741`
  Acceptance:
  selfhost-only gcpm lifecycle test passes with no `core/pkg::*` legacy semantic ops in logs.
  Status (2026-02-19): complete locally; `cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet` passes.

- [x] P0.5 Restore AI iteration SLO gate.
  Evidence:
  `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_ai_iteration_slo.sh` exits non-zero because core suite run fails (`-p gc_cli --test cli_smoke`).
  Acceptance:
  SLO script passes and records stable metrics under current budgets.
  Status (2026-02-19): complete locally; `scripts/check_ai_iteration_slo.sh` passes.

- [x] P0.6 Make workspace clippy gate pass under `-D warnings`.
  Evidence:
  `cargo clippy --workspace --all-targets -- -D warnings` fails with:
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_coreform/src/fixed_decimal.rs:165`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_coreform/src/fixed_decimal.rs:173`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_pkg/src/lock.rs:50`
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_pkg/src/lock.rs:306`
  Acceptance:
  full workspace clippy command succeeds in local and CI environments.
  Status (2026-02-19): complete locally; `cargo clippy --workspace --all-targets -- -D warnings` passes.

## P1 - Spec/Runtime Drift

- [x] P1.1 Reconcile WASI spec with actual shipped command surface and routing.
  Evidence:
  `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md:32` says routed set is only `fmt`, `eval`, `test`, `pack`, `vcs hash`.
  `cargo run -q -p gc_wasi_cli --bin genesis_wasi -- --help` shows broad command surface (`run`, `replay`, `store`, `refs`, `pkg/gcpm`, `sync`, `gc`, etc.).
  Acceptance:
  `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md` and `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md` agree with real behavior.
  Status (2026-02-19): complete; updated `docs/spec/WASI.md` + `docs/spec/CLI.md` to match real `genesis_wasi --help` and verified routed behavior with `cargo test -p gc_wasi_cli --test cli_selfhost_only --quiet`.

- [x] P1.2 Reconcile self-host boundary docs and scripts with split runner modules.
  Evidence:
  `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md:42`
  `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md:78`
  `/Users/corbensorenson/Documents/genesisCode/scripts/check_prelude_capability_coverage.sh:14`
  all assume dispatch lives in `crates/gc_effects/src/runner.rs`.
  Acceptance:
  boundary docs and guards target current module topology (`runner_capability_dispatch.rs`, `runner_cap_*`, `runner_gpu_host.rs`, `runner_editor_host.rs`).
  Status (2026-02-19): complete; docs and guards now target split runner modules.

- [x] P1.3 Replace brittle single-file AWK extraction in guard scripts with module-aware source-of-truth generation.
  Evidence:
  `/Users/corbensorenson/Documents/genesisCode/scripts/check_host_abi_conformance.sh`
  `/Users/corbensorenson/Documents/genesisCode/scripts/check_runner_high_level_op_guard.sh`
  parse only `runner.rs`, which now omits most dispatch arms.
  Acceptance:
  guards derive op surfaces from all capability dispatch modules (or from a generated manifest) and are resistant to refactors.
  Status (2026-02-19): complete; host ABI/high-level/presence guards derive dispatch ops from modular runner files.

- [x] P1.4 Add guard conformance tests that fail fast on future dispatch refactors.
  Evidence:
  current breakage reached runtime scripts without an automated unit/integration test that validates the guard parser assumptions.
  Acceptance:
  new tests validate guard extraction against known op fixtures and run in CI before full workflow steps.
  Status (2026-02-19): complete; added fixture suite + script:
  `/Users/corbensorenson/Documents/genesisCode/scripts/check_guard_extraction_fixtures.sh`,
  `/Users/corbensorenson/Documents/genesisCode/tests/spec/guard_fixtures/*`,
  `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/guard_extraction_fixtures.rs`.

- [x] P1.5 Add upgrade-plan health check so "Open checklist items: 0" cannot coexist with broken mandatory gates.
  Evidence:
  prior `upgrade_plan.md` claimed zero open items while multiple required scripts/tests currently fail.
  Acceptance:
  a CI/local check enforces consistency between red-team plan status and hard gate results.
  Status (2026-02-19): complete; added `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh` and CI-executed test `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/upgrade_plan_health.rs`.

## P2 - Self-Host Completion and AI-First Throughput

- [ ] P2.1 Complete selfhost-first package/VCS execution for effect-program paths.
  Evidence:
  selfhost-only runtime still detects legacy `core/pkg::*` fallback during `run` workflows.
  Acceptance:
  package/VCS/GC/GPK workflows invoked from effect programs execute without legacy semantic fallback and with deterministic replay parity.

- [ ] P2.2 Reduce default local iteration latency (target: sub-10-minute developer loop).
  Evidence:
  user-reported full test loop exceeds practical iteration window; current SLO gate already includes this concern but is red due regressions.
  Acceptance:
  documented fast-path workflow (`changed-file` + warmed artifact + shard selection) is default and objectively measured in CI/local scripts.

- [ ] P2.3 Expand deterministic stress coverage for high-throughput AI workflows (tasks + bridge + gpu/compute).
  Evidence:
  concurrency and bridge surfaces exist, but no single stress gate currently validates combined task scheduling, bridge budgets, replay integrity, and GPU compute paths at scale.
  Acceptance:
  new stress suite runs in CI profile (`standard` or `full`) with explicit latency/error budgets and replay verification artifacts.

## Execution Order (Recommended)

1. P0.1, P1.2, P1.3 (unblock broken guardrails first).
2. P0.3, P0.4, P0.5 (restore deterministic test/SLO signal).
3. P0.6 (restore workspace lint gate).
4. P1.1, P1.4, P1.5 (eliminate spec/process drift).
5. P2.1, P2.2, P2.3 (self-host completion and throughput hardening).
