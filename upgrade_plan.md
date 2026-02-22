# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.

Open checklist items: 3

## P0 - Immediate blockers

- [x] P0.1 Fix dirty-tree fast-loop command selection in `scripts/test_changed_fast.sh` for non-crate diffs.
  - Evidence:
    - `bash scripts/test_changed_fast.sh --dry-run` with script-only changes currently emits `cargo test -p ` (empty package spec).
    - `bash scripts/test_changed_fast.sh --budget-ms 120000` fails with `--package <SPEC> requires a SPEC format value`.
  - Completed:
    - Added explicit `non-crate-targeted` mode with deterministic command routing for script/docs-only diffs.
    - Added changed-file override support (`GENESIS_TEST_CHANGED_FILES_OVERRIDE` / `--changed-files-from`) for deterministic regression testing.
    - Added regression test `changed_fast_non_crate_targeted_mode_never_emits_empty_package_arg` in `crates/gc_cli/tests/shell_gate_regressions.rs`.
    - Validated dirty-tree and clean-tree loops (`bash scripts/test_changed_fast.sh --budget-ms 120000 --min-history 1`, `bash scripts/check_default_iteration_workflow.sh`).

- [x] P0.2 Restore source-size target conformance for active gates.
  - Evidence:
    - `bash scripts/check_source_size_budget.sh` fails:
      `crates/gc_effects/src/runner_capability_dispatch_tests/extended.rs has 1624 lines (target 1600)`.
  - Completed:
    - Split oversized module `crates/gc_effects/src/runner_capability_dispatch_tests/extended.rs` into:
      - `crates/gc_effects/src/runner_capability_dispatch_tests/extended_plugin.rs`
      - `crates/gc_effects/src/runner_capability_dispatch_tests/extended_crypto.rs`
    - Updated module wiring in `crates/gc_effects/src/runner_capability_dispatch_tests.rs`.
    - Verified `bash scripts/check_source_size_budget.sh` passes with empty debt allowlists.

## P1 - High-impact self-host and AI-first hardening

- [ ] P1.1 Continue doc consolidation to reduce agent retrieval noise.
  - Evidence:
    - `find docs -name '*.md' | wc -l` => `95`.
    - `rg -n "Legacy Split Doc:" docs -g '*.md' | wc -l` => `29`.
  - Acceptance:
    - Reduce markdown doc count to `<= 80`.
    - Reduce legacy split markers to `<= 10`.
    - Preserve bundle-first retrieval and pass doc freshness/hygiene gates.

- [ ] P1.2 Split oversized production Rust modules to AI-maintainable boundaries.
  - Evidence:
    - Current large production modules include:
      `crates/gc_patches/src/lib.rs` (1270),
      `crates/gc_kernel/src/compiled.rs` (1184),
      `crates/gc_cli_driver/src/pkg_workspace_ops.rs` (1148),
      `crates/gc_obligations/src/obligation_exec.rs` (1085),
      `crates/gc_types/src/infer.rs` (1050).
  - Acceptance:
    - Enforce clear module boundaries and reduce each above-file below target thresholds.
    - `scripts/check_source_size_budget.sh` passes without new allowlist debt.

- [ ] P1.3 Split oversized selfhost `.gc` authoring modules.
  - Evidence:
    - Large selfhost sources include:
      `selfhost/cli_reachability_v1.gc` (613),
      `selfhost/parse.gc` (574),
      `selfhost/cli_pkg_runtime_v1.gc` (541),
      `selfhost/cli_coreform_vcs_queries_v1.gc` (539),
      `selfhost/patch_schema_v1.gc` (525).
  - Acceptance:
    - Refactor into narrower domain modules with deterministic manifests.
    - `scripts/check_gc_source_size_budget.sh` remains green without debt allowlist growth.

- [x] P1.4 Make bootstrap-retirement/release guards operable under constrained local disk without weakening CI.
  - Evidence:
    - `bash scripts/check_bootstrap_retirement_gate.sh` currently fails locally due release guard minimum headroom (`2097152KB`) after safe reclaim.
  - Completed:
    - Added deterministic local degraded mode controls in `scripts/check_bootstrap_retirement_gate.sh`:
      - `GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE=auto|0|1`
      - `GENESIS_BOOTSTRAP_RETIREMENT_DISK_AUTO_RECLAIM=0|1`
    - Preserved CI strict fail-closed posture (`auto` disables degraded mode when `CI=true`).
    - Added machine-readable guard report:
      - `.genesis/perf/bootstrap_retirement_gate_report.json`
      - `kind = genesis/bootstrap-retirement-gate-report-v0.1`
      - explicit `status = ok|degraded|fail` (no implicit false-pass semantics).
    - Added regression test
      `bootstrap_retirement_gate_has_explicit_local_degraded_mode` in
      `crates/gc_cli/tests/shell_gate_regressions.rs`.
    - Documented operator guidance in:
      - `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
      - `docs/spec/SELF_HOST_BOUNDARY.md`

- [x] P1.5 Add machine-verifiable evidence mapping for feature-matrix capability claims.
  - Evidence:
    - `scripts/check_feature_matrix_gap_hygiene.sh` currently checks gap-ID sync only; capability `✅` claims are curated text with no per-row executable evidence contract.
  - Completed:
    - Added deterministic ledger generator `scripts/update_feature_matrix_evidence.sh`.
    - Added fail-closed drift guard `scripts/check_feature_matrix_evidence.sh`.
    - Published generated artifacts:
      - `docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json`
      - `docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md`
    - Wired evidence drift guard into `scripts/check_upgrade_plan_health.sh` (mandatory-local + common gates).

- [x] P1.6 Add a dedicated non-crate fast path for agent iteration.
  - Evidence:
    - Script/docs-only changes currently attempt crate test routing first; this is error-prone and wastes iteration budget.
  - Completed:
    - Added dedicated `non-crate-targeted` path in `scripts/test_changed_fast.sh`.
    - Non-crate diffs now execute deterministic lightweight checks first (`check_doc_hygiene`, `check_planning_docs_fresh`, targeted shell regression).
    - Runtime telemetry remains emitted via existing metrics report/historical budget machinery (`genesis/test-changed-fast-metrics-v0.1`).

- [x] P1.7 Improve selfhost artifact reviewability for agent-driven development.
  - Evidence:
    - `selfhost/toolchain.gc` is a single-line canonical artifact (~526KB) that is deterministic but difficult for diffs/reviews.
  - Completed:
    - Added deterministic sidecar generator `scripts/update_selfhost_toolchain_review.sh`.
    - Published review-sidecar artifact `selfhost/toolchain.review.md` with module-level hash/index summaries.
    - Added drift guard `scripts/check_selfhost_toolchain_review_fresh.sh`.
    - Wired sidecar drift guard into `scripts/check_upgrade_plan_health.sh` (mandatory-local + common gates).

## P2 - Strategic completeness for AI-first adoption

- [x] P2.1 Publish a standards crosswalk package for regulated engineering profiles.
  - Scope:
    - Expand DO-178C / NASA NPR 7150.2 / IEC 62304 mapping from summary status to objective-level evidence and explicit non-claims.
  - Completed:
    - Added normative objective-level crosswalk artifacts:
      - `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
      - `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`
    - Added deterministic crosswalk conformance guard:
      - `scripts/check_assurance_standards_crosswalk.sh`
    - Integrated crosswalk guard into health lanes:
      - `scripts/check_upgrade_plan_health.sh`
    - Linked profile-pack evidence outputs and unresolved explicit non-claims across:
      - `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`
      - `docs/spec/GCPM_BUNDLE_v0.1.md`
      - `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`

- [x] P2.2 Elevate `write_genesisCode_skill` into a first-class, versioned agent-distribution artifact.
  - Evidence:
    - `docs/write_genesisCode_skill.md` is currently a lightweight pointer; project needs a direct consumable package for Codex/Claude-style agents.
  - Completed:
    - Published versioned skill pack docs:
      - `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
      - `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`
    - Added conformance gate `scripts/check_write_genesiscode_skill_pack.sh`.
    - Wired pack into canonical docs entrypoints:
      - `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
      - `docs/AGENT_ONBOARDING_v0.1.md`
      - `docs/INDEX.md`
      - `docs/write_genesisCode_skill.md`
    - Wired conformance gate into `scripts/check_upgrade_plan_health.sh` (mandatory-local + common gates).
