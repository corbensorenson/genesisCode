# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.

Open checklist items: 7

## P0 - Immediate blockers

- [ ] P0.1 Restore planning truthfulness between capability claims and open backlog state.
  - Evidence:
    - `feature_matrix.md` marks "Fully self-hosted toolchain with zero bootstrap-language dependency" as partial (`⚠️`) at `feature_matrix.md:20`.
    - The same file reports `Known GenesisCode gaps` as `none` at `feature_matrix.md:70-72`.
    - `docs/status/REDTEAM_REPORT.md` currently reports no active P0/P1 risks (`docs/status/REDTEAM_REPORT.md:11`) while major strategic gaps remain unresolved for selfhost v1 cutover.
  - Acceptance:
    - Open-gap reporting must be derived from objective readiness signals (not manually zeroed).
    - Add fail-closed check that forbids `Known GenesisCode gaps: none` unless all non-`✅` GenesisCode matrix rows have linked closure rationale and no unresolved plan IDs.
    - Add a machine-readable selfhost readiness report consumed by `upgrade_plan.md`, `feature_matrix.md`, and `REDTEAM_REPORT.md`.

- [ ] P0.2 Replace metadata-only `gcpm build` target support with executable build pipelines.
  - Evidence:
    - `gcpm build` currently materializes deterministic metadata bundle files (`build_manifest.gc`, `package.toml`, `package_artifact.txt`, `provenance.gc`) without compiling/running target-specific toolchains (`crates/gc_cli_driver/src/pkg_workspace_ops.rs:254-346` and `crates/gc_cli_driver/src/pkg_workspace_ops.rs:348-406`).
    - Mobile/edge test coverage verifies profile metadata fields only (`crates/gc_cli/tests/cli_pkg_workspace.rs:277-360`), not executable target artifacts.
  - Acceptance:
    - `gcpm build --target ios|android|edge|service-runtime` must emit executable/runtime-bootable artifacts (or equivalent reproducible runner bundles), not metadata-only bundles.
    - Add deterministic post-build verification sub-lanes per target (boot/smoke/contract).
    - Fail `release-full` when target pipeline is metadata-only.

## P1 - High-impact self-host and AI-first hardening

- [x] P1.1 Add end-to-end conformance workflows for mobile/edge/service-runtime targets.
  - Evidence:
    - Current agent deployment workflow validates only `web|desktop|service` (`examples/agent_deploy_bundle_workflow/workflow.sh`).
    - Gauntlet workflow set does not include `ios`, `android`, `edge`, or `service-runtime` domains (`scripts/check_agent_reference_workflows.sh` workflow table).
  - Completion:
    - Added first-party deterministic deployment workflows:
      `examples/agent_deploy_ios_workflow/workflow.sh`,
      `examples/agent_deploy_android_workflow/workflow.sh`,
      `examples/agent_deploy_edge_workflow/workflow.sh`,
      `examples/agent_deploy_service_runtime_workflow/workflow.sh`.
    - Added shared deployment lane helper with deterministic replay assertions and required artifact checks:
      `examples/agent_deploy_bundle_workflow/target_workflow_lib.sh`.
    - Extended gauntlet workflow/domain contracts to include
      `deploy_ios`, `deploy_android`, `deploy_edge`, and `deploy_service_runtime`
      in `scripts/check_agent_reference_workflows.sh`, which is consumed by both
      native and runtime-parity lanes.

- [ ] P1.2 Reduce local iteration latency to a deterministic AI-authoring inner loop budget.
  - Evidence:
    - `dev-fast` health path currently executes a large non-cargo gate set (31 common non-cargo gates observed in live run output) before cargo shards.
    - Runtime parity lane currently costs ~157s (`.genesis/perf/agent_workflow_runtime_parity_report.json:5`) and gauntlet lane ~105s (`.genesis/perf/agent_capability_gauntlet_report.json:1-5` + `elapsed_ms`).
  - Acceptance:
    - Add `agent-inner-loop` profile with strict deterministic contract checks but bounded wall-time target (`<= 300000 ms`) on warm cache.
    - Persist and enforce p95 history for inner-loop profile duration.
    - Remove redundant repeated process startups for high-frequency local workflows.

- [x] P1.3 Enforce explicit SLO fields and fail-closed budgets across all major gauntlet/parity reports.
  - Evidence:
    - `agent_capability_gauntlet_report.json` has `default_max_ms` but no top-level `budget_ms` field (`.genesis/perf/agent_capability_gauntlet_report.json:1-5`).
    - Large lanes exist (`elapsed_ms=104921` gauntlet, `elapsed_ms=157748` parity) but budget semantics are not uniformly normalized across report families.
  - Completion:
    - Standardized gauntlet + parity report schema with explicit top-level SLO fields:
      `elapsed_ms`, `budget_ms`, `history_samples`, `history_p95_ms`,
      `history_p95_enforced`, `history_p95_ok`, `ok`, and `fail_reasons`.
    - Added fail-closed SLO schema validator:
      `scripts/check_slo_report_contracts.sh`.
    - Wired SLO schema checks into health profiles in
      `scripts/check_upgrade_plan_health.sh` (prepush/release).
    - Upgraded gauntlet regression semantics to enforce p95-based regression budgets
      (not single-sample spikes) and added bootstrap-history mode for newly introduced workflows.

- [ ] P1.4 Decompose oversized high-churn Rust modules for AI maintainability.
  - Evidence:
    - Source-size audit still shows very large production modules:
      - `crates/gc_cli_driver/src/cmd_selfhost.rs` (1048 lines),
      - `crates/gc_cli_driver/src/pkg_workspace_ops.rs` (995 lines),
      - `crates/gc_obligations/src/obligation_exec.rs` (977 lines),
      - `crates/gc_gfx/src/lib.rs` (972 lines),
      - `crates/gc_prelude/src/prelude.rs` (949 lines),
      - `crates/gc_cli_driver/src/semantic_workspace.rs` (949 lines),
      from `scripts/check_source_size_budget.sh` output.
  - Acceptance:
    - Split high-churn modules into bounded domain units with stable interfaces.
    - Keep source-size debt allowlists empty.
    - Add decomposition progress checks for production modules over target thresholds.

- [x] P1.5 Expand `write_genesisCode_skill` executable distribution breadth for "make anything" scope.
  - Evidence:
    - Current distribution manifest validates at low corpus size (`prompts=3`, `recipes=4`, `reports=1`) from `scripts/check_write_genesiscode_skill_distribution.sh`.
  - Completion:
    - Expanded distribution corpus to `prompts=10`, `recipes=11` in
      `docs/skill_pack/write_genesiscode_v1/manifest.json`, adding targeted prompt/recipe assets for:
      deployment targets, failure recovery, performance triage, assurance, plugin/FFI, XR, and durable data domains.
    - Added explicit fault-injection recipe mode (`mode="fault-injection"`) and enforced required domain coverage through manifest `distribution_requirements`.
    - Upgraded `scripts/check_write_genesiscode_skill_distribution.sh` to fail-closed on minimum prompt/recipe thresholds, required domain set coverage, mandatory fault-injection recipe presence, and minimum report score thresholds.
    - Updated distribution kit spec to reflect broadened corpus and enforcement contract:
      `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`.

- [x] P1.6 Repair feature-matrix evidence ledger drift for split GPU/GFX and deployment claims.
  - Evidence:
    - Matrix states split compute/gfx bundles + independent lanes (`feature_matrix.md:24`), but evidence ledger still maps that capability only to `GPU_GFX_BUNDLE` + `check_gpu_compute_runtime_profile.sh` (`docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md:25`).
    - Deployment claim is broad (`feature_matrix.md:42`) while evidence ledger maps only generic CLI + health script without target-specific conformance references (`docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md:43`).
  - Completion:
    - Upgraded evidence generation logic in `scripts/update_feature_matrix_evidence.sh` with claim-specific required mappings for:
      - split GPU/GFX capability surface (`GPU_COMPUTE_BUNDLE`, `GFX_RUNTIME_BUNDLE`, independent compute/gfx/decoupling checks),
      - deployment/bundle pipeline (gcpm schemas + target contract test/workflow references).
    - Hardened `scripts/check_feature_matrix_evidence.sh` with fail-closed required mapping assertions for high-impact capability claims.
    - Regenerated synchronized evidence ledgers:
      `docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json` and
      `docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md`.

- [ ] P1.7 Add executable semantic refactor application workflow (not just planning output).
  - Evidence:
    - CLI currently exposes `semantic-edit index`, `workspace-graph`, and `refactor-plan` only (`docs/spec/CLI.md:146-154`).
    - There is no first-class command that applies semantic refactor plans deterministically end-to-end.
  - Acceptance:
    - Add `semantic-edit apply-plan` (or equivalent) with deterministic patch emission, conflict diagnostics, and obligation-gated verification.
    - Add tests for deterministic re-application/idempotence and workspace-wide conflict reporting.
    - Add agent workflow example using plan+apply+verify loop.

## P2 - Strategic completeness for "agent can build anything" scope

- [x] P2.1 Publish machine-readable selfhost completeness scorecard and closure criteria.
  - Evidence:
    - Selfhost status is currently distributed across docs/scripts and partial matrix rows; no canonical machine report currently drives closure readiness.
  - Completion:
    - Added deterministic machine-readable readiness scorecard:
      `scripts/check_selfhost_readiness_scorecard.sh` -> `.genesis/perf/selfhost_readiness_report.json`
      (`kind = genesis/selfhost-readiness-v0.1`) with scored dimensions:
      runtime routing coverage, parity-only surface isolation, bootstrap mode strictness,
      and deprecated bootstrap reference count.
    - Wired readiness scorecard into selfhost dashboard freshness lane:
      `scripts/check_selfhost_dashboard_fresh.sh`.
    - Wired readiness source reference enforcement into drift checks:
      `scripts/check_doc_topology_drift.sh` now requires
      `.genesis/perf/selfhost_readiness_report.json` references in `upgrade_plan.md`
      and `feature_matrix.md`.

- [ ] P2.2 Harden production WASM artifact boundary to exclude parity-only Rust frontend surfaces.
  - Evidence:
    - `gc_wasm` still includes explicit Rust frontend parity/eval pathways in the same crate surface (`crates/gc_wasm/src/lib.rs` comments and methods describing parity-only Rust frontend paths).
  - Acceptance:
    - Gate parity-only APIs behind explicit non-production feature/profile boundaries.
    - Add exported-symbol and API-surface checks proving production WASM artifacts expose only allowed selfhost paths.
    - Fail release lanes when parity-only WASM surfaces leak into production builds.

- [ ] P2.3 Complete documentation consolidation phase 2 with measurable reduction and stronger ownership boundaries.
  - Evidence:
    - Documentation surface remains large (`docs_md=92`, `total_md=117` from live repo count), increasing retrieval ambiguity for agents.
  - Acceptance:
    - Merge low-signal/redundant docs into canonical bundle roots while preserving thin stubs only where necessary.
    - Add numeric documentation complexity targets (file count + average retrieval fan-out per capability).
    - Enforce topology ownership + canonical source links for all retained leaf docs.
