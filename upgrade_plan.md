# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 0

## P0 - Ship-Blocking Reliability

- [x] P0.1 Reconcile fast-loop budget policy drift across execution scripts and profile matrix gates.
  Evidence:
  - `bash scripts/check_test_execution_profile_matrix.sh` fails with:
    - `changed-fast default budget must remain 300000ms (5m)`
  - Current defaults diverged in runtime scripts:
    - `/Users/corbensorenson/Documents/genesisCode/scripts/test_changed_fast.sh` uses `GENESIS_TEST_CHANGED_BUDGET_MS:-60000`
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_default_iteration_workflow.sh` uses `GENESIS_BUDGET_CHANGED_FAST_MS:-60000`
  Impact:
  - `prepush-standard` health lane is red even when code/tests are otherwise healthy.
  Completion:
  - Canonical changed-fast default is pinned to 300000ms (5m) across scripts and matrix checks.
  - `scripts/check_test_execution_profile_matrix.sh` now passes.

- [x] P0.2 Restore source-size budget gate by decomposing oversized host backend test surface.
  Evidence:
  - `bash scripts/check_source_size_budget.sh` fails with:
    - `target violation crates/gc_effects/src/tests_host_backends.rs has 1701 lines (target 1600)`
  Impact:
  - `prepush-standard` health lane fails in common gates.
  Completion:
  - Split first-party backend-heavy tests into:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/tests_host_backends_first_party.rs`
    - trimmed `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/tests_host_backends.rs`
  - `scripts/check_source_size_budget.sh` now passes.

- [x] P0.3 Make workspace clippy clean under `-D warnings`.
  Evidence:
  - `cargo clippy --workspace --all-targets -- -D warnings` fails at:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/tests/prelude_foundation_stdlib_conformance.rs:151`
    - lint: `clippy::op-ref`
  Impact:
  - `prepush-standard` profile cannot be promoted to green release posture.
  Completion:
  - Fixed prelude conformance lint and reduced constructor argument surface in runtime bench reporting.
  - `cargo clippy --workspace --all-targets -- -D warnings` now passes.

- [x] P0.4 Eliminate shell-dependent false-green behavior in rust-frontend retirement guard.
  Evidence:
  - Standalone invocation fails:
    - `bash scripts/check_no_production_rust_frontend_refs.sh`
    - finds `CoreformFrontend::Rust` references in production files.
  - Login-shell invocation passes incorrectly:
    - `bash -lc "bash scripts/check_no_production_rust_frontend_refs.sh"` prints `ok`.
  - Root cause:
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_no_production_rust_frontend_refs.sh` relies on `rg` inside an `if` and does not require/fallback when `rg` is unavailable.
    - `check_upgrade_plan_health.sh` executes gates via `bash -lc`, which can have a different PATH/tool visibility.
  Impact:
  - A production selfhost-boundary violation can slip through guard lanes with false-green status.
  Completion:
  - Guard now fail-closes with deterministic `rg`/`grep` search behavior.
  - Standalone and `bash -lc` executions now return identical results.
  - Production rust-frontend references were removed from guard-tracked sources, making the gate green.

## P1 - AI-First Maintainability and Agent Throughput

- [x] P1.1 Continue decomposition of >1k-line production hotspots to improve agent editability.
  Evidence:
  - Current production hotspots include:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_types/src/lib.rs` (1172)
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs` (1168)
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/prelude.rs` (1145)
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_pkg_helpers.rs` (1144)
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_patches/src/lib.rs` (1161)
  Impact:
  - Large mixed-responsibility modules reduce agent planning precision and increase regression risk.
  Completion:
  - Decomposed `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_pkg_helpers.rs`
    into focused modules:
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_patch_merge.rs`
    - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs`
  - Preserved existing call sites via a thin facade module and re-exports.
  - Validated behavior and reliability gates with:
    - `cargo test -p gc_effects --tests --quiet`
    - `cargo clippy --workspace --all-targets -- -D warnings`
    - `bash scripts/check_source_size_budget.sh`

- [x] P1.2 Add a doc-truth guard for `feature_matrix.md` unresolved-gap hygiene.
  Evidence:
  - Existing freshness guard validates dates/cross-links only:
    - `/Users/corbensorenson/Documents/genesisCode/scripts/check_planning_docs_fresh.sh`
  - No current guard checks whether feature-matrix “known gaps” reflect unresolved plan items.
  Impact:
  - Drift can reappear silently even when date checks pass.
  Completion:
  - Added `/Users/corbensorenson/Documents/genesisCode/scripts/check_feature_matrix_gap_hygiene.sh`.
  - Integrated the guard into `/Users/corbensorenson/Documents/genesisCode/scripts/check_upgrade_plan_health.sh` common gate lane.
