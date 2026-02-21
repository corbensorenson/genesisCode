# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 11

## P0 - Self-Hosted v1 Blockers

- [ ] P0.1 Wire first-party GPU/desktop runtime backends into production `genesis` builds.
  Evidence:
  - `crates/gc_effects/Cargo.toml` keeps `gpu-device-backend` and `gfx-desktop-backend` as optional with `default = []`.
  - `crates/gc_cli/Cargo.toml` and `crates/gc_cli_driver/Cargo.toml` do not forward runtime-backend feature selection.
  Acceptance:
  - Define explicit production feature profiles for backend-capable vs headless binaries.
  - Add deterministic tests proving expected backend availability in each profile.

- [ ] P0.2 Complete device-runtime coverage for canonical GPU lifecycle ops.
  Evidence:
  - `crates/gc_effects/src/runner_gpu_backend_policy.rs` routes device backend only for submit/introspection ops (`submit`, `limits`, `features`).
  - `crates/gc_effects/src/runner_gpu_host.rs` still serves `create-*`/`write-*`/`read-*`/`destroy-resource` from first-party in-memory path even when `gpu_backend = "device-runtime"`.
  Acceptance:
  - Either implement device-runtime handlers for full lifecycle ops or explicitly split/rename policy semantics so behavior is unambiguous.
  - Add replay tests for all canonical `gpu/compute::*` and `gfx/gpu::*` lifecycle operations.

- [ ] P0.3 Unify GPU backend naming and policy semantics across runtime, microbench, docs, and CI.
  Evidence:
  - Runtime capability policy uses `gpu_backend = "device-runtime"` (`crates/gc_effects/src/runner_gpu_backend_policy.rs`).
  - Microbench/CI/device bridge lanes use `device-bridge` (`crates/gc_runtime_bench/Cargo.toml`, `.github/workflows/ci.yml`, `docs/spec/GPU_COMPUTE_DEVICE_BRIDGE_v0.1.md`).
  Acceptance:
  - Publish one canonical backend vocabulary and alias policy.
  - Eliminate naming drift in specs, env vars, CI, and emitted metrics.

- [ ] P0.4 Make `gcpm env` able to realize environments from lock state on fresh machines.
  Evidence:
  - `crates/gc_cli_driver/src/pkg_workspace_ops.rs` `build_env_deps_term` fails when locked snapshots/commits are absent in local `.genesis/store`.
  - Current `gcpm env` materialization writes deterministic descriptors/artifacts but does not hydrate missing locked deps.
  Acceptance:
  - Add explicit `gcpm env --hydrate` (or equivalent) path that fetches missing locked artifacts deterministically via policy-gated sync/store ops.
  - Keep replay/audit contracts intact.

## P1 - Performance, Reliability, and CI Gaps

- [ ] P1.1 Stop skipping local health gates whenever backlog is non-zero.
  Evidence:
  - `scripts/check_upgrade_plan_health.sh` exits early when unchecked items exist and `GENESIS_HEALTH_ENFORCE_GATES` is not set.
  Acceptance:
  - Always run a minimum mandatory local gate set (boundary/panic/routing guards) regardless of backlog size.
  - Keep optional heavy suites profile-gated.

- [ ] P1.2 Reduce default iteration wall-time for fast loops.
  Evidence:
  - `.genesis/perf/test_changed_fast_metrics.json` shows `elapsed_ms = 152799`.
  - `.genesis/perf/upgrade_plan_health_profile_report.json` shows `profile = dev-fast` with `elapsed_ms = 238744`.
  Acceptance:
  - Bring `test_changed_fast` under 60s on reference dev hardware.
  - Bring `dev-fast` health profile under 120s without losing core regression coverage.

- [ ] P1.3 Add explicit CI coverage for backend feature combinations in production crates.
  Evidence:
  - Current CI runs backend-heavy microbench lane for `gc_runtime_bench` (`device-bridge`) but lacks targeted compile/test matrix for `gc_effects`/`gc_cli` with `gpu-device-backend` and `gfx-desktop-backend`.
  Acceptance:
  - Add compile + smoke tests for:
    - headless profile
    - gpu-device-backend profile
    - gfx-desktop-backend profile
    - combined profile

- [ ] P1.4 Tighten AI-editability budgets by continuing module decomposition of hot files.
  Evidence:
  - Large production file hotspots remain, including:
    - `crates/gc_cli_driver/src/cmd_vcs.rs` (1255)
    - `crates/gc_types/src/lib.rs` (1172)
    - `crates/gc_prelude/src/selfhost_coreform_v1.rs` (1168)
    - `crates/gc_prelude/src/prelude.rs` (1145)
    - `crates/gc_effects/src/runner_vcs_pkg_helpers.rs` (1144)
  Acceptance:
  - Split by stable domain boundaries and preserve behavior/tests.
  - Lower max production module size target for AI-first workflows.

- [ ] P1.5 Improve health profile parallelism defaults outside prepush lane.
  Evidence:
  - `scripts/check_upgrade_plan_health.sh` only auto-shards `prepush-standard`; other profiles default to `1` shard.
  Acceptance:
  - Enable deterministic sharding defaults for `dev-fast` and `release-full` where safe.
  - Record shard config and wall-time deltas in profile report output.

## P2 - AI-First Authoring and Workflow Ergonomics

- [ ] P2.1 Add deterministic custom task-contract execution for `gcpm run`.
  Evidence:
  - `crates/gc_cli_driver/src/pkg_task_runner.rs` supports a fixed command enum (`test|pack|build|typecheck|lint|run|bench|contract|eval|fmt|optimize`) and rejects others.
  Acceptance:
  - Add a contract-first task hook (hash-pinned and policy-gated) so agents can define reusable workflow tasks without shell escape hatches.
  - Preserve deterministic task schema + replay behavior.

- [ ] P2.2 Add freshness gates for planning/capability docs consumed by agents.
  Evidence:
  - `feature_matrix.md` and `upgrade_plan.md` are manually maintained and can drift from actual runtime state.
  Acceptance:
  - Add lightweight validation/generation checks so agent-facing planning/capability docs fail fast when stale.
