# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-23

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.
Machine-readable selfhost readiness source: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 7

## P0 - Immediate blockers

- [x] P0.1 Restore cargo target-dir policy compliance across strict gates.
  - Completion evidence:
    - added `genesis_configure_cargo_target_dir` wiring to:
      - `scripts/check_gcpm_target_runtime_pipelines.sh`
      - `scripts/check_gpu_gfx_headroom_conformance.sh`
      - `scripts/check_selfhost_readiness_scorecard.sh`
    - `bash scripts/check_cargo_target_dir_policy.sh` now passes (`violations=0`).

- [x] P0.2 Reestablish deterministic selfhost toolchain review freshness.
  - Completion evidence:
    - regenerated `selfhost/toolchain.review.md` via `scripts/update_selfhost_toolchain_review.sh`.
    - `bash scripts/check_selfhost_toolchain_review_fresh.sh` now passes.

- [x] P0.3 Bring test-size gate back under target budget.
  - Completion evidence:
    - split `crates/gc_cli/tests/cli_pkg_workspace.rs` helper/runtime-profile coverage into:
      - `crates/gc_cli/tests/support/pkg_workspace_test_support.rs`
      - `crates/gc_cli/tests/cli_pkg_workspace_profile_runtime.rs`
    - `bash scripts/check_test_size_budget.sh` now passes (`cli_pkg_workspace.rs` at/under target).

- [ ] P0.4 Return strict profile health lanes to green in clean local/CI execution.
  - Evidence:
    - `prepush-standard` now executes full gate coverage end-to-end (including runtime-backend matrix + agent gauntlet) without semantic failures, but currently misses wall budget on cold run (`942840ms > 900000ms`).
    - latest `release-full` validation attempt still failed in selfhost-strict compile stage under local disk pressure (`No space left on device`) and must be rerun with stable headroom.
  - Definition of done:
    - both profiles run green from a clean checkout with no degraded/local bypass mode.

## P1 - High-impact self-host and AI-first hardening

- [ ] P1.1 Upgrade WebXR conformance from deterministic-degraded to functional pass.
  - Evidence:
    - `.genesis/perf/webxr_browser_conformance_report.json` is deterministic (`ok=true`) but captures runtime degradation:
      - `frame.status = "timeout"`
      - `session_close.status = "error"`
  - Definition of done:
    - WebXR conformance requires successful frame acquisition and clean session close in release evidence lanes.
    - fallback orientation/headless mode is tracked as non-release evidence only.

- [ ] P1.2 Create headroom in documentation complexity budgets for agent retrieval quality.
  - Evidence:
    - `.genesis/perf/doc_complexity_report.json` is at ceiling (`active_docs_md=106` with `max_active_docs_md=106`; `capability_retrieval_fanout=0.4583` vs cap `0.46`).
  - Definition of done:
    - reduce active docs and fanout below caps with operational headroom.
    - keep canonical agent onboarding path minimal and stable.

- [x] P1.3 Decompose high-churn assurance/runtime surfaces to reduce AI maintenance risk.
  - Completion evidence:
    - split assurance-pack surface into focused modules:
      - `crates/gc_cli_driver/src/pkg_assurance_pack_ops.rs` (orchestration; now `512` lines)
      - `crates/gc_cli_driver/src/pkg_assurance_pack_ops/types.rs`
      - `crates/gc_cli_driver/src/pkg_assurance_pack_ops/profile.rs`
      - `crates/gc_cli_driver/src/pkg_assurance_pack_ops/resolve.rs`
      - `crates/gc_cli_driver/src/pkg_assurance_pack_ops/parse.rs`
      - `crates/gc_cli_driver/src/pkg_assurance_pack_ops/term_helpers.rs`
      - `crates/gc_cli_driver/src/pkg_assurance_pack_ops/bundle.rs`
    - removed deprecated monolith-adjacent bundle file `crates/gc_cli_driver/src/pkg_assurance_pack_bundle.rs`.
    - validation: `cargo test -p gc_cli --test cli_pkg_assurance_pack` passes (`3 passed`).

- [ ] P1.4 Reduce strict health warmup latency for agent inner loops.
  - Evidence:
    - `.genesis/perf/upgrade_plan_health_warmup_prepush-standard.json`: `elapsed_ms=125915`.
    - `.genesis/perf/upgrade_plan_health_warmup_release-full.json`: `elapsed_ms=177279`.
  - Definition of done:
    - profile warmup times are materially reduced via targeted scope/caching while preserving coverage guarantees.

- [ ] P1.5 Retire residual parity-harness ownership dependencies from selfhost readiness posture.
  - Evidence:
    - `.genesis/perf/selfhost_readiness_report.json` still tracks parity-only references (`parity_ref_files` non-empty).
    - feature matrix still marks full zero-bootstrap-language closure as partial.
  - Definition of done:
    - parity ownership checks are migrated to selfhost-native artifacts/tests where feasible.
    - remaining Rust TCB boundaries are explicit and minimized.

## P2 - Strategic completeness for "agent can build anything" scope

- [ ] P2.1 Close feature-matrix partial on full selfhost closure (or codify minimal permanent TCB contract).
  - Evidence:
    - `feature_matrix.md` still marks fully self-hosted toolchain closure as `⚠️` due parity/bootstrap boundary constraints.
  - Definition of done:
    - either promote to `✅` with evidence or publish explicit permanent TCB declaration and reduce claim scope accordingly.

- [ ] P2.2 Add first-class `gcpm` agent scaffolding for end-to-end product archetypes.
  - Evidence:
    - agent-first language posture exists, but project bootstrap for complex domains is still mostly manual composition.
  - Definition of done:
    - ship deterministic scaffolds + policy/caps/deploy presets for core archetypes (web, service, desktop, mobile, XR/game, data/AI workloads) to reduce agent bootstrap friction.
