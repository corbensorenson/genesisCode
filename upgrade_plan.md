# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 0

## P0 - Self-Hosted v1 Blockers

- [x] P0.1 Wire first-party GPU/desktop runtime backends into production `genesis` builds.
  Evidence:
  - Added explicit runtime profile features in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/Cargo.toml`:
    - `profile-headless` (default)
    - `profile-gpu`
    - `profile-gfx`
    - `profile-backend`
    - plus backend feature forwards:
      - `gpu-device-backend -> gc_effects/gpu-device-backend`
      - `gfx-desktop-backend -> gc_effects/gfx-desktop-backend`
  - Added compile-time runtime profile conflict guards in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` to prevent ambiguous profile combinations.
  - Added profile/backend consistency module + tests in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/runtime_backend_profile.rs`
    - test: `backend_feature_flags_match_active_profile`
  - Wired production CLI profile features in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/Cargo.toml`:
    - `default = ["profile-headless"]`
    - profile forwards (`profile-headless|profile-gpu|profile-gfx|profile-backend`)
    - backend forwards (`gpu-device-backend`, `gfx-desktop-backend`)
    - `gc_cli_driver` dependency now uses `default-features = false` so selected CLI profile controls driver profile deterministically.
  - Mirrored feature vocabulary in parity driver package to keep shared-source `cfg(feature=...)` checks valid:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver_parity/Cargo.toml`
  - Added normative profile doc:
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md`
    - referenced from `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md` and `/Users/corbensorenson/Documents/genesisCode/docs/INDEX.md`.
  Acceptance:
  - Define explicit production feature profiles for backend-capable vs headless binaries.
  - Add deterministic tests proving expected backend availability in each profile.

- [x] P0.2 Complete device-runtime coverage for canonical GPU lifecycle ops.
  Evidence:
  - Explicit backend scope split added in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_backend_policy.rs`:
    - `gpu_backend = "device-runtime"` now canonically means submit/introspection scope.
    - `gpu_backend = "device-runtime-full"` now explicitly requests canonical lifecycle routing.
    - aliases normalized (`device-bridge`, `device-runtime-submit`, `device-runtime-lifecycle`).
  - Host routing now follows backend scope deterministically in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs`
    - device fallback metadata now reports the effective requested backend scope label.
  - Added replay/behavior tests in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/tests_host_backends.rs`
    - `gpu_compute_device_runtime_submit_scope_keeps_lifecycle_on_first_party`
    - `gpu_compute_device_runtime_full_lifecycle_require_device_fails_closed`
    - `gpu_compute_device_runtime_full_lifecycle_allow_fallback_marks_lifecycle_ops`
  Acceptance:
  - Either implement device-runtime handlers for full lifecycle ops or explicitly split/rename policy semantics so behavior is unambiguous.
  - Add replay tests for all canonical `gpu/compute::*` and `gfx/gpu::*` lifecycle operations.

- [x] P0.3 Unify GPU backend naming and policy semantics across runtime, microbench, docs, and CI.
  Evidence:
  - Canonical emitted backend label unified to `device-runtime` in runtime microbench device path:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_runtime_bench/src/device_bridge.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_runtime_bench/src/bench_gpu_compute.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_runtime_bench/tests/device_bridge_replay.rs`
  - Added canonical/legacy normalization policy in guard scripts:
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_runtime_microbench_budgets.sh`
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_gpu_compute_runtime_profile.sh`
    - `device-bridge` input now normalizes to `device-runtime`.
  - CI required-backend vocabulary aligned:
    - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml`
    - `GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND: "device-runtime"`
  - Specs/docs updated to make canonical vocabulary + alias policy explicit:
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CAPS_TOML.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_COMPUTE_DEVICE_BRIDGE_v0.1.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_GPU_SLO_v0.1.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/policies/README.md`
  Acceptance:
  - Publish one canonical backend vocabulary and alias policy.
  - Eliminate naming drift in specs, env vars, CI, and emitted metrics.

- [x] P0.4 Make `gcpm env` able to realize environments from lock state on fresh machines.
  Evidence:
  - Added explicit `--hydrate` path on `gcpm env`:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_args/pkg_cmd.rs`
  - Added deterministic missing-lock artifact discovery:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_workspace_ops.rs`
    - `collect_missing_locked_hashes(workspace_file, lock_file)` (sorted + deduped hashes).
  - Added policy-gated hydration effect program:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/program_builders/pkg.rs`
    - `mk_pkg_env_hydrate_program(missing_hashes)` emits deterministic ordered `core/store::get` requests.
  - Wired hydration execution into env materialization path with real effect logs:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs`
    - sealed effect errors are surfaced with stable exit semantics (`EX_CAPS_DENIED` for caps denial, else `EX_EVAL`).
  - Added end-to-end remote read-through test:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_pkg_workspace.rs`
    - `gcpm_env_hydrate_fetches_missing_locked_artifacts_via_store_get`
    - validates plain `gcpm env` fails when lock artifacts are missing, and `gcpm env --hydrate` succeeds + populates local store + logs `core/store::get`.
  - Updated docs:
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_WORKSPACE_v0.1.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_ENV_v0.1.md`
  Acceptance:
  - Add explicit `gcpm env --hydrate` (or equivalent) path that fetches missing locked artifacts deterministically via policy-gated sync/store ops.
  - Keep replay/audit contracts intact.

## P1 - Performance, Reliability, and CI Gaps

- [x] P1.1 Stop skipping local health gates whenever backlog is non-zero.
  Evidence:
  - Updated `scripts/check_upgrade_plan_health.sh` to run mandatory local guards even when backlog is non-zero and `GENESIS_HEALTH_ENFORCE_GATES` is not set.
  - Mandatory local gate set now includes:
    - `scripts/check_selfhost_boundary.sh --strict`
    - `scripts/check_redteam_report.sh`
    - `scripts/check_planning_docs_fresh.sh`
    - `scripts/check_no_user_panics.sh`
    - `scripts/check_no_production_rust_frontend_refs.sh`
  - Script now writes health profile report output for this mandatory-local lane instead of exiting before gate/report execution.
  Acceptance:
  - Always run a minimum mandatory local gate set (boundary/panic/routing guards) regardless of backlog size.
  - Keep optional heavy suites profile-gated.

- [x] P1.2 Reduce default iteration wall-time for fast loops.
  Evidence:
  - Tightened fast-loop defaults to target SLOs:
    - `/Users/corbensorenson/Documents/genesisCode/scripts/test_changed_fast.sh`
      - default budget reduced from `300000` to `60000`.
      - history P95 now compares budget/mode/runner-compatible samples first, preventing stale mixed-budget regressions from poisoning the fast lane.
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh`
      - `DEV_FAST_BUDGET_MS` default reduced to `60000`.
      - new `DEV_FAST_PROFILE_WALL_BUDGET_MS` default `120000`.
      - backlog mandatory-local lane now uses deterministic sharding (`HEALTH_SHARDS`) instead of forced serial execution.
      - dev-fast mandatory-local lane now reports/enforces 120s wall budget.
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_default_iteration_workflow.sh`
      - default changed-fast budget reduced to `60000`, runner switched to `auto`.
  - Verified metrics on current reference workspace:
    - `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/test_changed_fast_metrics.json`
      - `elapsed_ms = 1580`, `budget_ms = 60000`, `history_p95_ms = 1769`.
    - `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/upgrade_plan_health_profile_report.json`
      - `profile = "dev-fast"`, `elapsed_ms = 8984`, `budget_ms = 120000`, `ok = true`.
  Acceptance:
  - Bring `test_changed_fast` under 60s on reference dev hardware.
  - Bring `dev-fast` health profile under 120s without losing core regression coverage.

- [x] P1.3 Add explicit CI coverage for backend feature combinations in production crates.
  Evidence:
  - Added deterministic runtime backend matrix guard:
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_runtime_backend_feature_matrix.sh`
    - validates:
      - `gc_effects` feature combos: headless/gpu/gfx/combined
      - `gc_cli` profile combos: `profile-headless|profile-gpu|profile-gfx|profile-backend`
      - `gc_cli_driver` profile/backend consistency tests across all profiles
  - Added CI enforcement step in:
    - `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml`
    - step: `Runtime Backend Feature Matrix Guard` for `standard|full`.
  - Added health profile enforcement hooks in:
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh`
    - included in `prepush-standard` and `release-full` profile gates.
  Acceptance:
  - Add compile + smoke tests for:
    - headless profile
    - gpu-device-backend profile
    - gfx-desktop-backend profile
    - combined profile

- [x] P1.4 Tighten AI-editability budgets by continuing module decomposition of hot files.
  Evidence:
  - Split `cmd_vcs` helper/parsing/extraction surface into a dedicated module:
    - added `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/vcs_helpers.rs` (395 LOC)
    - moved:
      - refs extractors
      - pkg spec + strategy parsing helpers
      - set-ref parsing/validation helpers
      - VCS/pkg result hash extractors
  - Updated root wiring/imports to consume the decomposed module:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs`
  - Hotspot reduction achieved:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_vcs.rs`
      - reduced from 1255 LOC to 861 LOC.
  - Behavior parity validated:
    - `cargo test -p gc_cli_driver --lib --quiet`
    - `cargo test -p gc_cli_driver_parity --lib --quiet`
  Acceptance:
  - Split by stable domain boundaries and preserve behavior/tests.
  - Lower max production module size target for AI-first workflows.

- [x] P1.5 Improve health profile parallelism defaults outside prepush lane.
  Evidence:
  - Updated `scripts/check_upgrade_plan_health.sh` default shard policy:
    - `dev-fast`: `3|2|1` shards by host CPU count tiers.
    - `prepush-standard`: unchanged `4|2|1`.
    - `release-full`: `4|3|2|1` shards by host CPU count tiers.
  - Enhanced `write_health_profile_report` to include wall-time delta fields when previous report exists:
    - `previous_elapsed_ms`
    - `elapsed_delta_ms`
  Acceptance:
  - Enable deterministic sharding defaults for `dev-fast` and `release-full` where safe.
  - Record shard config and wall-time deltas in profile report output.

## P2 - AI-First Authoring and Workflow Ergonomics

- [x] P2.1 Add deterministic custom task-contract execution for `gcpm run`.
  Evidence:
  - Added a dedicated hash-pinned contract task action in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_task_runner.rs`
    - `WorkspaceTaskAction::Contract { file, caps, log, engine, contract_hash_hex }`
  - `cmd = "contract"` now requires `--contract-h <hex64>` (or `--contract-hash`) and rejects unsupported args.
  - Added deterministic file hash gate:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_task_runner.rs`
    - `verify_contract_task_file_hash(file, expected_hash_hex)` fails closed on mismatch.
  - Wired execution path in:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs`
    - `gcpm run` verifies the contract hash pin before policy-gated `cmd_run` execution.
  - Added parser/hash tests:
    - `resolves_contract_task_with_hash_pin_and_validates_file_hash`
    - `contract_task_requires_hash_pin_and_reports_mismatch`
  - Updated docs/specs for the contract task hook:
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_WORKSPACE_v0.1.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_CLI_CONTRACT_v0.1.md`
    - `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
  Acceptance:
  - Add a contract-first task hook (hash-pinned and policy-gated) so agents can define reusable workflow tasks without shell escape hatches.
  - Preserve deterministic task schema + replay behavior.

- [x] P2.2 Add freshness gates for planning/capability docs consumed by agents.
  Evidence:
  - Added `scripts/check_planning_docs_fresh.sh` to validate:
    - `upgrade_plan.md` has parseable `Last updated: YYYY-MM-DD`.
    - `feature_matrix.md` has parseable `Audit Date: YYYY-MM-DD` in title.
    - `docs/INDEX.md` has parseable `Last updated: YYYY-MM-DD`.
    - `feature_matrix` and `docs/INDEX` are not older than `upgrade_plan`.
    - index/matrix cross-reference planning docs.
  - Integrated `scripts/check_planning_docs_fresh.sh` into `scripts/check_upgrade_plan_health.sh`:
    - mandatory local guards
    - common full gate lane
  Acceptance:
  - Add lightweight validation/generation checks so agent-facing planning/capability docs fail fast when stale.
