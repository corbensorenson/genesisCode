# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-20

This file contains only unresolved findings from the latest fine-tooth-comb red-team pass.
Completed work is intentionally removed.

Open checklist items: 3

## P0 - Self-Hosted v1 Blockers

- [x] P0.1 Fix interactive gfx adapter output-channel contamination that breaks deterministic agent workflows.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host/terminal_adapter.rs:19` writes terminal title escape sequences to `stdout`.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host/terminal_adapter.rs:33` writes bell bytes to `stdout`.
  - `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_agent_reference_workflows.sh` currently fails in `agent-interactive-gfx-compute-workflow` with escape-sequence-contaminated output (`expected acceptance hash, got: \x1b]0;Genesis...`).
  Acceptance:
  - Interactive host control sequences never pollute command output used for hashes/logs.
  - `scripts/check_agent_reference_workflows.sh` passes end-to-end in selfhost-only mode.

- [ ] P0.2 Ship a production device-backed GPU runtime path in `gc_effects` (not only deterministic in-memory simulation).
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs:13`..`:34` models GPU resources as in-memory `Vec<u8>` state.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs:75`..`:120` routes canonical GPU ops to first-party simulated handlers.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_runtime_bench/src/device_bridge.rs:46`..`:65` exposes optional in-repo device bridge only from benchmark crate feature (`device-bridge`), not integrated as production runtime backend.
  Acceptance:
  - Canonical `gpu/compute::*` and `gfx/gpu::*` support a maintained in-repo device backend path in production runtime.
  - Deterministic replay stays valid (log-driven response replay), with explicit policy-controlled fallback path.

- [ ] P0.3 Add a real non-terminal first-party window/audio backend for app/game-class workloads.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host.rs:10` uses a `terminal_adapter` for interactive first-party profile.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host.rs:167`..`:220` routes interactive profile capabilities through terminal IO adapters.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host/terminal_adapter.rs` is terminal-event/terminal-control based (`crossterm`), not a windowing backend.
  Acceptance:
  - Provide a production window/input/audio backend path (desktop-class, not terminal-only).
  - Keep deterministic headless CI profile and replay semantics intact.

- [x] P0.4 Expand host capability ABI for network/process primitives required to build general services/tools.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md:146`..`:148` lists only `io/fs::*` and `sys/time::now` for core OS-like primitives.
  - No canonical `io/net::*` or `sys/process::*` operations are present in host ABI operation list (`docs/spec/HOST_ABI.md:37`..`:149`).
  Acceptance:
  - Add capability-gated network primitives (minimum: HTTP client or socket domain) and process execution primitives.
  - Ensure deny-by-default policy, deterministic effect logs, and replay behavior are specified and tested.

## P1 - Performance, Reliability, and Platform Gaps

- [x] P1.1 Replace O(n) full-tree watch polling with incremental filesystem watch backend(s).
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:1010`..`:1067` rebuilds full snapshots on each poll.
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:1069`..`:1081` recursively scans full directories each cycle.
  Acceptance:
  - Watch polling scales incrementally for large repos (event-driven or indexed delta approach).
  - Replay semantics remain deterministic and stable across platforms.

- [x] P1.2 Keep local prepush health parity with CI by adding agent workflow gate to `prepush-standard`.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh:272`..`:280` `prepush-standard` profile gates do not include `check_agent_reference_workflows.sh`.
  - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:230`..`:232` does include `check_agent_reference_workflows.sh`.
  Acceptance:
  - `prepush-standard` catches agent workflow regressions before CI.
  - Local default health profile and CI standard profile gate surfaces are intentionally aligned.

- [x] P1.3 Restore selfhost cutover dashboard freshness contract.
  Evidence:
  - `bash /Users/corbensorenson/Documents/genesisCode/scripts/check_selfhost_dashboard_fresh.sh` currently fails:
    - `docs/status/SELFHOST_CUTOVER.md` is stale relative to generated dashboard.
  Acceptance:
  - `check_selfhost_dashboard_fresh.sh` passes on clean tree.
  - Dashboard freshness is regenerated deterministically as part of normal update flow.

- [x] P1.4 Return to lint-clean `-D warnings` status in production crates and keep it enforced.
  Evidence:
  - `cargo clippy -p gc_effects --lib -- -D warnings` currently fails with:
    - `clippy::collapsible_if` at `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host.rs:298`
    - `clippy::question_mark` at `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs:508`
  Acceptance:
  - Current clippy failures are fixed.
  - Health profiles that enforce clippy complete successfully on clean tree.

- [x] P1.5 Add a constrained WASI networking profile that supports package/registry HTTP workflows.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md:59` explicitly rejects `http(s)` registry remotes on `wasm32-wasip1`.
  Acceptance:
  - Define and implement a policy-gated WASI network profile that supports secure remote registry workflows.
  - Maintain deny-by-default and explicit allowlist semantics.

- [x] P1.6 Strengthen deterministic runtime resource model beyond current conservative limits.
  Evidence:
  - Added deterministic runtime budgets in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/policy.rs` (`[runtime]` parser + policy fields):
    - `max_effect_ops`
    - `max_payload_bytes_per_op`
    - `max_payload_bytes_per_run`
    - `max_response_bytes_per_op`
    - `max_response_bytes_per_run`
  - Enforced fail-closed runtime budgets in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_runtime_budget.rs` and integrated into `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs`.
  - Added runtime budget tests in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/tests.rs`:
    - `runtime_policy_max_effect_ops_fail_closes_second_request`
    - `runtime_policy_max_payload_bytes_per_op_fail_closes_oversized_request`
    - `runtime_policy_max_payload_bytes_per_run_fail_closes_on_cumulative_budget`
    - `runtime_policy_max_response_bytes_per_op_fail_closes_oversized_response`
  - Documented runtime budgets in:
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CAPS_TOML.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/LIMITS.md`
  Acceptance:
  - Introduce stronger deterministic resource controls for adversarial AI-generated workloads.
  - Document and test fail-closed behavior under constrained resource budgets.

## P2 - AI-First Authoring and Project Ergonomics

- [x] P2.1 Upgrade `gcpm env` from metadata emitter to full deterministic environment realization.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_workspace_ops.rs:241`..`:388` writes `env.gcenv` + `provenance.gc` metadata only.
  - Current implementation does not install dependencies, realize toolchain state, or materialize runnable workspace assets.
  Acceptance:
  - `gcpm env` materializes deterministic, runnable environment state (toolchain/deps/profile artifacts), not only descriptors.
  - Outputs are content-addressed and replay-auditable.

- [x] P2.2 Expand `gcpm run` task model beyond `test|pack|typecheck`.
  Evidence:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_task_runner.rs:23`..`:36` supports only `test`, `pack`, and `typecheck`.
  Acceptance:
  - Add richer deterministic task graph support suitable for agent-authored project workflows (build/run/bench/lint/custom task contracts).
  - Preserve deterministic task resolution and policy boundaries.

- [ ] P2.3 Reduce high-churn large-file hotspots to improve agent editability and reviewability.
  Progress checklist:
  - [x] Decompose `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_args.rs` into domain command modules.
  - [x] Decompose `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs` to keep editor runtime orchestration thin.
  - [ ] Decompose `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/expr_lowering.rs` by lowering stage families.
  - [x] Decompose `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_exec.rs` by obligation family executors.
  - [ ] Decompose `/Users/corbensorenson/Documents/genesisCode/crates/gc_patches/src/lib.rs` by patch artifact/merge/apply surfaces.
  - [ ] Decompose `/Users/corbensorenson/Documents/genesisCode/crates/gc_kernel/src/eval.rs` by evaluator phase boundaries.
  - [ ] Decompose `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasm/src/lib.rs` by parser/lowering/runtime bridge layers.
  Evidence:
  - Completed this pass:
    - Split `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_args.rs` into domain modules:
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_args.rs` = 759
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_args/pkg_cmd.rs` = 319
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_args/policy_gc_vcs_cmd.rs` = 253
    - Split `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs` task execution into module:
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs` = 491
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_tasks.rs` = 607
    - Split `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_exec.rs` by obligation family executors:
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_exec.rs` = 695
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_exec_tests.rs` = 386
      - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_exec_replay.rs` = 243
  - `wc -l` current hotspots:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasm/src/lib.rs` = 1587
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_kernel/src/eval.rs` = 1549
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_patches/src/lib.rs` = 1518
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/expr_lowering.rs` = 1346
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_exec.rs` = 695
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs` = 491
  Acceptance:
  - Decompose these modules along stable domain boundaries with no behavior drift.
  - Preserve/expand tests while reducing per-file cognitive load for agent-driven edits.
