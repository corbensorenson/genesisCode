# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.
Machine-readable selfhost readiness source: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 2

## P0 - Immediate blockers

- [x] P0.1 Replace false-green selfhost readiness scoring with capability-truth scoring.
  - Completion:
    - Added `critical_gate_truth` scoring dimension in `scripts/check_selfhost_readiness_scorecard.sh`.
    - Readiness now consumes critical runtime truth signals:
      `agent_capability_gauntlet_report.json`,
      `production_cli_help_surface_report.json`,
      and live deployment pipeline execution via `scripts/check_gcpm_target_runtime_pipelines.sh`.
    - Readiness can no longer report green from routing checks alone while critical reports are red.

- [x] P0.2 Stabilize GPU/GFX agent workflows under constrained disk/temp headroom.
  - Completion:
    - Added dedicated conformance lane:
      `scripts/check_gpu_gfx_headroom_conformance.sh`
      (`kind = genesis/gpu-gfx-headroom-conformance-v0.1`).
    - Lane enforces two execution modes over GPU/GFX workflows:
      - normal headroom mode (strict preflight),
      - simulated low-headroom mode (intentional insufficient free-space threshold with non-strict continuation).
    - Both workflows (`agent_gpu_compute_workflow`, `agent_interactive_gfx_compute_workflow`) now run twice per lane and require deterministic replay equivalence.
    - `TMPDIR` is deterministic/configurable per lane and validated through shared preflight contracts.
    - Wired conformance enforcement into strict health profiles:
      `scripts/check_upgrade_plan_health.sh` (`prepush-standard`, `release-full`).
    - Wired readiness truth model to include this report in
      `scripts/check_selfhost_readiness_scorecard.sh` critical gate checks.

- [x] P0.3 Upgrade `gcpm build --target` from runtime-runner contracts to executable target artifacts.
  - Completion:
    - Replaced runtime-runner bundle outputs in
      `crates/gc_cli_driver/src/pkg_workspace_ops_build.rs`
      with executable target bundle pipeline kind
      `executable-target-bundle-v2`.
    - Added modular artifact writer:
      `crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
      generating deterministic target package files, detached signature metadata (`sha256`), and executable launch artifacts per target.
    - Build manifests now carry explicit artifact layout and signature verification lanes (`:artifact-signature`, `:boot`, `:smoke`) instead of runtime runner contracts.
    - Updated conformance lane:
      `scripts/check_gcpm_target_runtime_pipelines.sh`
      now validates package/signature integrity and executes target launch artifacts for `ios|android|edge|service-runtime`.
    - Updated workspace build integration tests:
      `crates/gc_cli/tests/cli_pkg_workspace.rs`
      to assert executable-target artifact surfaces and launch-lane behavior.

- [x] P0.4 Prevent planning truth drift when critical reports fail.
  - Completion:
    - Hardened `scripts/check_feature_matrix_gap_hygiene.sh` to fail zero-gap declarations when critical reports are missing/red (`agent-capability-gauntlet`, `production-cli-help-surface`).
    - Hardened `scripts/check_redteam_report.sh` to fail if unresolved P0/P1 risk set is empty while critical reports are missing/red.
    - Planning docs are now fail-closed against critical report-family regressions instead of readiness-only ID drift.

## P1 - High-impact self-host and AI-first hardening

- [x] P1.1 Derive selfhost cutover coverage from the real CLI command registry.
  - Completion:
    - Replaced static cutover rows with command-registry-driven row generation in
      `crates/gc_cli_driver/src/cmd_selfhost_helpers.rs` via `Cli::command()` introspection.
    - `crates/gc_cli_driver/src/cmd_selfhost.rs` now computes coverage from generated rows
      instead of `SELFHOST_CUTOVER_ROWS`.
    - Removed deprecated static coverage table from
      `crates/gc_cli_driver/src/lib.rs` (no dual-source drift path remains).
    - Added fail-closed metadata parity checks:
      - missing metadata for any live CLI command is an error,
      - stale metadata entries not present in CLI are an error.
    - Refreshed committed dashboard output:
      `docs/status/SELFHOST_CUTOVER.md`.

- [x] P1.2 Bind tool-qualification test artifacts to executed test lineage.
  - Completion:
    - `gcpm qualify` now requires `--snapshot <hex64>` and validates release snapshot/policy/commit bindings for tool-qualification artifacts.
    - `--test-artifact` now resolves run-manifest lineage from local `.genesis/store` (`id=<run-manifest-hex64>`), not caller-asserted raw test hashes.
    - Added fail-closed lineage validator module:
      `crates/gc_cli_driver/src/pkg_assurance_ops_qualification.rs`, enforcing canonical run-manifest bytes and mandatory fields:
      `:test-id`, `:artifact`, `:result :pass`, `:profile`, `:run-id`, `:runner`, and `:release` bindings.
    - Referenced test artifacts must exist in store, parse as CoreForm, and declare `:ok true`; out-of-lineage or mismatched lineage/policy/snapshot/profile now fails closed.
    - Policy/runtime qualification validators now enforce snapshot + lineage fields via
      `crates/gc_vcs/src/assurance.rs` and updated gate contexts in publish/refs/registry paths.
    - Added dedicated gate/report:
      `scripts/check_tool_qualification_lineage.sh`
      -> `.genesis/perf/tool_qualification_lineage_report.json`.

- [ ] P1.3 Deliver first-class dependency solver/range semantics in `gcpm`.
  - Evidence:
    - `crates/gc_cli_driver/src/cli_args/pkg_cmd.rs` marks `gcpm lock` and `gcpm update` as `local-only v0.1`.
    - Resolver now supports commit/snapshot/ref selectors plus deterministic `semver:<range>` tag resolution, but still lacks registry-aware conflict diagnostics and mature workspace upgrade ergonomics.
  - Progress (this pass):
    - [x] Added deterministic semver selector support `semver:<range>` in selector classification/inference:
      `crates/gc_pkg/src/lock.rs`.
    - [x] Added deterministic semver range resolution against `refs/tags/*` in pkg-low resolver:
      `crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs`
      (policy-controlled highest/lowest selection with stable tie-breaking).
    - [x] Added strict lock-invariant checks for semver range selectors:
      resolved tag must parse as semver and satisfy the declared range.
    - [x] Updated CLI selector guidance and strategy validation:
      `crates/gc_cli_driver/src/cli_args/pkg_cmd.rs`,
      `crates/gc_cli_driver/src/vcs_helpers.rs`,
      `docs/spec/CLI.md`.
    - [ ] Add registry-aware conflict diagnostics for incompatible ranges across workspace members.
    - [ ] Add selective upgrade flows (`gcpm update --only ...`) with auditable constraint rationale.
  - Definition of done:
    - Deterministic range solver with conflict diagnostics, lock reproducibility, and policy-aware registry resolution.
    - Workspace update flows support selective upgrades and auditable constraint rationale.

- [x] P1.4 Bring production CLI help-surface gate below budget with build reuse.
  - Completion:
    - Consolidated release help-surface builds into one cargo invocation in
      `scripts/check_production_cli_help_surface.sh` with shared binary assertions from
      `scripts/lib/release_bin.sh`.
    - Added history-scope partitioning support to runtime-budget tooling:
      - `scripts/lib/profile_runtime_budget.py`
      - `scripts/lib/profile_gate_timing.sh`
    - Bound production help-surface budgets to a scoped pipeline key
      (`single-build-v1`) so p95 enforcement tracks comparable post-optimization runs.
    - Verified default gate is green:
      `.genesis/perf/production_cli_help_surface_report.json`
      now reports `ok=true` with low elapsed and scoped p95.

- [x] P1.5 Add disk-headroom-aware preflight and recovery paths across heavy gates.
  - Completion:
    - Added shared heavy-gate preflight library:
      `scripts/lib/heavy_gate_preflight.sh`.
    - Wired shared preflight into both heavy gates called out in the backlog:
      - `scripts/check_agent_reference_workflows.sh`
      - `scripts/check_production_cli_help_surface.sh`
    - Preflight now enforces:
      - disk headroom check via `scripts/check_disk_headroom.sh` (with reclaim + strict-mode support),
      - deterministic writable temp root probing,
      - explicit `TMPDIR` export to a stable `.genesis/tmp/...` path.
    - Result: disk/tmp failures now fail fast with actionable diagnostics instead of mid-run temp-dir crashes.

- [x] P1.6 Reduce AI-maintainability risk in high-churn runtime/compiler surfaces.
  - Completion:
    - Tightened high-churn decomposition budget and expanded tracked module surface in
      `policies/source_decomposition_progress.toml`
      (`target_max_lines` reduced to `990`, tracked modules expanded to 10).
    - Added explicit selfhost migration plan contract:
      `docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`
      (`Selfhost Migration Plan (High-Churn Rust -> GC)` section).
    - Added fail-closed migration-plan drift gate:
      `scripts/check_selfhost_gc_migration_plan.sh`
      (emits `.genesis/perf/selfhost_gc_migration_plan_report.json`).
    - Wired migration/decomposition guard coverage into health profiles via
      `scripts/check_upgrade_plan_health.sh`.

- [x] P1.7 Consolidate and normalize documentation for agent-first authoring.
  - Completion:
    - Locked canonical doc topology and drift gates across planning/runtime docs:
      `docs/INDEX.md`, `docs/spec/DOC_TOPOLOGY_v0.1.md`,
      `scripts/check_doc_topology_drift.sh`, `scripts/check_doc_hygiene.sh`.
    - Promoted `docs/AGENT_ONBOARDING_v0.1.md` as the single agent-first onboarding spine,
      explicitly covering semantics, runtime profiles, packaging/deployment, assurance, and active risk sources.
    - Kept duplicate-leaf growth fail-closed via doc complexity budgets:
      `scripts/check_doc_complexity_budget.sh` and
      `.genesis/perf/doc_complexity_report.json` (`active_docs_md=106`, `active_top_level_leaf_docs=6`).

- [x] P1.8 Complete bootstrap retirement path and production fallback removal.
  - Completion:
    - Enforced retirement/fallback closure in strict health profiles by wiring
      `scripts/check_bootstrap_retirement_gate.sh` into
      `scripts/check_upgrade_plan_health.sh` (`prepush-standard`, `release-full`).
    - Retirement gate fail-closes on archived-bootstrap regressions via
      `scripts/check_old_bootstrap_retirement.sh` and
      release/default selfhost guard checks (`selfhost_release_profile_guard.sh`, `selfhost_default_profile_guard.sh`).
    - Residual bootstrap-only Rust semantics remain isolated under
      `old_bootstrap/rust_semantics/` with production runtime references blocked by gate enforcement.

## P2 - Strategic completeness for "agent can build anything" scope

- [x] P2.1 Publish `write_genesisCode_skill.md` + executable conformance pack after selfhost closure.
  - Completion:
    - Published detailed canonical handbook:
      `docs/write_genesisCode_skill.md`
      with explicit architecture/contract/testing/debugging/perf/assurance patterns.
    - Added fail-closed guide-structure checker:
      `scripts/check_write_genesiscode_skill_guide.sh`.
    - Verified executable conformance pack gates are green:
      - `scripts/check_genesiscode_authoring_skill.sh`
      - `scripts/check_write_genesiscode_skill_pack.sh`
      - `scripts/check_write_genesiscode_skill_distribution.sh`
      - `scripts/check_write_genesiscode_skill_conformance.sh`
    - Wired guide conformance into health profiles through
      `scripts/check_upgrade_plan_health.sh`.

- [ ] P2.2 Raise assurance-pack closure from "partial alignment" to auditable high-assurance readiness.
  - Evidence:
    - Feature matrix currently marks regulated standards as partial alignment and explicitly notes external certification responsibilities.
    - High-assurance closure still needs stronger object-equivalence and independent-verifier execution workflows beyond lineage closure.
  - Definition of done:
    - Add trace closure from requirements -> low-level artifacts -> tests -> emitted binaries/object equivalence evidence.
    - Add independent-verifier workflow gates aligned with DAL A/B, NASA Class A/B, and IEC Class C evidence expectations.

- [x] P2.3 Deepen first-class non-graphics GPU + XR/WebXR productization surface.
  - Completion:
    - Consolidated canonical productization guidance into existing bundles:
      - `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
        (`Productization Kits (Non-Gfx + XR)` section),
      - `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`.
    - Expanded distribution recipe domains in
      `docs/skill_pack/write_genesiscode_v1/manifest.json`.
    - Added fail-closed productization checker:
      `scripts/check_gpu_xr_productization_kits.sh`
      (report: `.genesis/perf/gpu_xr_productization_kits_report.json`), validating:
      - native replay-capable compute/XR workflows,
      - WebXR CI lane presence,
      - WebXR deterministic replay evidence when report artifacts are present.
    - Wired productization checker into strict profile lanes in
      `scripts/check_upgrade_plan_health.sh`.
