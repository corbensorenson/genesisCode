# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.

Open checklist items: 0

## P0 - Immediate blockers

- [x] P0.1 Restore planning truthfulness between capability claims and open backlog state.
  - Completion:
    - Upgraded planning drift checks so gap/risk IDs are fail-closed against machine-readable readiness output:
      `scripts/check_feature_matrix_gap_hygiene.sh` and `scripts/check_redteam_report.sh`
      now require unresolved IDs to match `.genesis/perf/selfhost_readiness_report.json`
      (`kind = genesis/selfhost-readiness-v0.1`, `unresolved_upgrade_plan_ids`).
    - Added explicit zero-gap safeguard in `scripts/check_feature_matrix_gap_hygiene.sh`:
      `Known GenesisCode gaps: none` is only allowed when no unresolved IDs remain and non-`✅`
      GenesisCode rows carry explicit inline rationale.
    - Updated planning docs to explicitly document readiness-derived synchronization:
      `feature_matrix.md` and `docs/status/REDTEAM_REPORT.md`.

- [x] P0.2 Replace metadata-only `gcpm build` target support with executable build pipelines.
  - Completion:
    - Refactored target build pipeline implementation into a dedicated domain module:
      `crates/gc_cli_driver/src/pkg_workspace_ops_build.rs`,
      keeping the public `gcpm build` interface stable through
      `crates/gc_cli_driver/src/pkg_workspace_ops.rs`.
    - Upgraded bundle contract from metadata-only output to deterministic runtime-runner bundles:
      `build_manifest.gc` now stamps `:pipeline-kind = "runtime-runner-bundle-v1"` and
      verification lanes (`:contract`, `:boot`, `:smoke`).
    - `gcpm build --target ios|android|edge|service-runtime` now emits runtime-bootable lane artifacts:
      `runtime/runtime_contract.gc`,
      `runtime/boot.sh`,
      `runtime/smoke.sh`,
      plus executable mode enforcement for runner scripts.
    - Added deterministic lane execution checks in command-level tests:
      `crates/gc_cli/tests/cli_pkg_workspace.rs`
      (`gcpm_build_supports_mobile_and_edge_target_contracts`) now validates
      contract/boot/smoke outputs against target + bundle hash.
    - Added release-enforced runtime-pipeline gate:
      `scripts/check_gcpm_target_runtime_pipelines.sh`,
      wired into `prepush-standard` and `release-full` in
      `scripts/check_upgrade_plan_health.sh` so metadata-only regressions fail the release profile.
    - Upgraded agent deployment workflows to run runtime verification lanes:
      `examples/agent_deploy_bundle_workflow/target_workflow_lib.sh`.

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

- [x] P1.2 Reduce local iteration latency to a deterministic AI-authoring inner loop budget.
  - Completion:
    - Added `agent-inner-loop` health profile in `scripts/check_upgrade_plan_health.sh`
      with bounded wall-time budget (`GENESIS_HEALTH_AGENT_INNER_LOOP_BUDGET_MS`, default `300000`)
      and a narrowed deterministic contract set for fast local authoring loops.
    - Added persisted history + p95 enforcement for profile duration using
      `scripts/lib/profile_runtime_budget.py` with deterministic seed baseline:
      `policies/perf/upgrade_plan_health_agent_inner_loop_seed_history.jsonl`,
      report `.genesis/perf/upgrade_plan_health_agent_inner_loop_report.json`,
      history `.genesis/perf/upgrade_plan_health_agent_inner_loop_history.jsonl`.
    - Reduced repeated startup overhead by avoiding full common-gate execution for this profile
      while retaining strict selfhost/planning/diagnostic contract checks + `cli_smoke` + changed-fast.
    - Updated profile documentation and drift guard:
      `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md` and
      `scripts/check_test_execution_profile_matrix.sh`.

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

- [x] P1.4 Decompose oversized high-churn Rust modules for AI maintainability.
  - Completion:
    - Split high-churn CLI driver domains into bounded modules with stable interfaces:
      - Selfhost helper extraction:
        `crates/gc_cli_driver/src/cmd_selfhost_helpers.rs`,
        reducing `crates/gc_cli_driver/src/cmd_selfhost.rs` to focused command orchestration.
      - Semantic workspace type + misc extraction:
        `crates/gc_cli_driver/src/semantic_workspace_types.rs`,
        `crates/gc_cli_driver/src/semantic_workspace_misc.rs`,
        reducing `crates/gc_cli_driver/src/semantic_workspace.rs` below the decomposition threshold.
      - Target build extraction:
        `crates/gc_cli_driver/src/pkg_workspace_ops_build.rs`,
        reducing `crates/gc_cli_driver/src/pkg_workspace_ops.rs`.
    - Added fail-closed decomposition progress policy + gate:
      - Policy: `policies/source_decomposition_progress.toml`
      - Gate: `scripts/check_source_decomposition_progress.sh`
      - Report: `.genesis/perf/source_decomposition_progress_report.json`
      (`kind = genesis/source-decomposition-progress-v0.1`).
    - Wired decomposition gate into health profile execution in
      `scripts/check_upgrade_plan_health.sh` (including `agent-inner-loop` and strict profiles).
    - Kept source-size debt allowlists empty in
      `policies/source_size_budget.toml` (`rust_target_exclude_paths = []`, `gc_target_exclude_paths = []`).

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

- [x] P1.7 Add executable semantic refactor application workflow (not just planning output).
  - Completion:
    - Added `semantic-edit apply-plan` command surface with deterministic plan+apply flow in:
      `crates/gc_cli_driver/src/cli_args.rs`,
      `crates/gc_cli_driver/src/cmd_security_ops.rs`,
      `crates/gc_cli_driver/src/semantic_workspace.rs`.
    - `apply-plan` now emits deterministic patch payload (`patch_hash` + `patch_coreform`), returns workspace-wide conflict diagnostics in `plan-conflicts` mode, and executes obligation-gated patch application when safe.
    - Added deterministic re-application/conflict tests:
      `crates/gc_cli/tests/cli_semantic_edit.rs`
      (`semantic_edit_apply_plan_rename_is_deterministic_on_reapply_conflict`,
      `semantic_edit_apply_plan_reports_workspace_ambiguous_definition_conflict`).
    - Added agent workflow example for plan+apply+verify loop:
      `examples/agent_semantic_refactor_apply_workflow/workflow.sh`.
    - Updated CLI contracts/docs:
      `docs/spec/CLI.md`,
      `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`,
      `crates/gc_cli/tests/cli_json_schema_registry.rs`.

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

- [x] P2.2 Harden production WASM artifact boundary to exclude parity-only Rust frontend surfaces.
  - Completion:
    - Gated parity-only WASM API exports behind explicit non-production feature boundaries:
      `crates/gc_wasm/src/lib.rs` now conditionally exports `*_rust` entrypoints and runtime rust-only methods only under `feature = "parity-harness"`.
    - Added explicit crate-level parity harness feature switch:
      `crates/gc_wasm/Cargo.toml`.
    - Added production WASM exported-surface conformance gate:
      `scripts/check_wasm_production_surface.sh` with machine-readable report
      `.genesis/perf/wasm_production_surface_report.json`
      (`kind = genesis/wasm-production-surface-v0.1`), forbidding parity-only symbol leaks and requiring selfhost artifact APIs.
    - Wired fail-closed enforcement into release profile lanes:
      `scripts/check_upgrade_plan_health.sh` (`release-full` profile).
    - Updated profile docs + drift guard:
      `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`,
      `scripts/check_test_execution_profile_matrix.sh`.

- [x] P2.3 Complete documentation consolidation phase 2 with measurable reduction and stronger ownership boundaries.
  - Completion:
    - Consolidated low-signal top-level docs into canonical roots and converted replaced docs to strict redirect stubs:
      `docs/POLICY_DEFAULTS_v0.1.md`,
      `docs/STACKS_v0.2.md`,
      `docs/STYLE_GUIDE_v0.2.md`,
      with canonical mappings tracked in `docs/DEPRECATION_MAP_v0.1.md`.
    - Added explicit retained-leaf ownership and canonical source registry:
      `docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md`.
    - Added numeric documentation complexity targets and fail-closed policy:
      `docs/spec/DOC_COMPLEXITY_TARGETS_v0.1.md`,
      `policies/docs/doc_complexity_budget.toml`.
    - Added deterministic complexity/ownership enforcement gate + report:
      `scripts/check_doc_complexity_budget.sh` ->
      `.genesis/perf/doc_complexity_report.json`
      (`kind = genesis/doc-complexity-v0.1`).
    - Wired topology/planning/health gate enforcement updates:
      `scripts/check_doc_topology_drift.sh`,
      `scripts/check_planning_docs_fresh.sh`,
      `scripts/check_upgrade_plan_health.sh`,
      `docs/spec/DOC_TOPOLOGY_v0.1.md`,
      `docs/INDEX.md`.
