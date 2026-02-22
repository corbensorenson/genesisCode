# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 7

## P0 - Reliability blockers (must close before claiming stable self-host cutover)

- [ ] P0.1 Fix Bash 3 empty-array crash in health gate partitioning (`scripts/check_upgrade_plan_health.sh`).
  - Evidence: `GENESIS_HEALTH_PROFILE=dev-fast bash scripts/check_upgrade_plan_health.sh` failed with `NON_CARGO_PARTITION[@]: unbound variable` at line 643.
  - Root cause: `set -u` with empty array expansion is not guarded in partition handoff paths on Bash 3.2.
  - Exit criteria: health script passes on Bash 3.2 and Bash 5 for profiles with empty and non-empty gate partitions.

- [ ] P0.2 Add low-disk fail-fast + release artifact sanity validation for bootstrap-retirement/release guards.
  - Evidence:
    - `bash scripts/check_bootstrap_retirement_gate.sh` failed: `native release fmt --engine rust expected exit 2, got 126 (Permission denied)`.
    - Build emitted `LLVM ERROR: IO failure on output stream: No space left on device`.
    - `target/release/genesis` became non-executable and non-Mach-O (`file` did not report executable binary).
  - Root cause: release guard path builds under severe disk pressure without headroom precheck and without validating built artifacts before invoking them.
  - Exit criteria: guards fail with explicit low-disk diagnostics before compile, and verify executable/binary format for release artifacts before semantic assertions.

## P1 - Productization blockers for agent-first "build anything" posture

- [ ] P1.1 Regenerate stale selfhost artifact + freshness metadata.
  - Evidence: `bash scripts/check_selfhost_artifact_fresh.sh` reports committed `selfhost/toolchain.gc` is stale.
  - Exit criteria: fresh `genesis selfhost-artifact --out selfhost/toolchain.gc` output committed and freshness metadata updated.

- [ ] P1.2 Make runtime backend matrix gate disk-aware and cache-stable.
  - Evidence: `bash scripts/check_runtime_backend_feature_matrix.sh` failed with `No space left on device` while compiling `gc_cli_driver`/`gc_cli_driver_parity`.
  - Root cause: script uses default `target/` with many profile/feature rebuilds and no disk preflight.
  - Exit criteria: script supports shared configurable cargo target dir, performs headroom precheck, and emits deterministic failure reason when capacity is insufficient.

- [ ] P1.3 Standardize cargo target-dir policy across compile-heavy scripts to prevent cache sprawl.
  - Evidence: 33 scripts under `scripts/` contain cargo invocations but no explicit `CARGO_TARGET_DIR` strategy.
  - Impact: duplicate build caches inflate disk usage and slow local iteration loops.
  - Exit criteria: shared helper/enforcement for cargo target-dir policy adopted across script fleet with conformance check.

- [ ] P1.4 Add timing telemetry for compile-heavy non-health lanes to make iteration optimization actionable.
  - Evidence: failing/slow lanes (for example runtime backend matrix) do not emit per-stage timing artifacts comparable to health profile reports.
  - Impact: difficult to prioritize and validate test/runtime speedups for local + CI loops.
  - Exit criteria: each heavyweight gate emits machine-readable timing report with historical deltas.

## P2 - Agent usability and docs maintainability

- [ ] P2.1 Consolidate docs into stricter agent-facing bundles and reduce markdown sprawl.
  - Evidence: `docs/` currently contains 96 markdown files; retrieval context is fragmented across many v0.1 shard docs.
  - Goal: lower retrieval ambiguity for coding agents while preserving normative references.
  - Exit criteria: bundle-first navigation path documented, duplicate/low-signal docs merged or archived, and index remains freshness-checked.
