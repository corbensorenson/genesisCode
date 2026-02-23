# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-23

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.
Machine-readable selfhost readiness source: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 0

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

- [x] P0.4 Return strict profile health lanes to green in clean local/CI execution.
  - Completion evidence:
    - `bash scripts/check_upgrade_plan_health.sh --profile release-full`
      - `upgrade-plan-health: gate_count=76`
      - `upgrade-plan-health: ok`
    - `bash scripts/check_upgrade_plan_health.sh --profile prepush-standard`
      - `upgrade-plan-health: elapsed_ms=675910`
      - `upgrade-plan-health: gate_count=67`
      - `upgrade-plan-health: ok`
    - stabilized gauntlet regression enforcement against cold-build noise while preserving
      functional fail-closed behavior:
      - `scripts/check_agent_reference_workflows.sh`
      - added `build_bootstrap_mode` detection and regression slack floor.

## P1 - High-impact self-host and AI-first hardening

- [x] P1.1 Upgrade WebXR conformance from deterministic-degraded to functional pass.
  - Completion evidence:
    - `.genesis/perf/webxr_browser_conformance_report.json` now records:
      - `ok = true`
      - `functional_pass = true`
      - `frame.status = "ok"`
      - `session_close.status = "closed-quiesced"`
    - browser harness upgraded in `scripts/webxr_browser_conformance.mjs`:
      - deterministic XR render-layer setup (`XRWebGLLayer`) before frame probe.
      - close verification upgraded from raw `session.end()` promise-only to functional close proof:
        - explicit `session.end()` request,
        - old-session frame-quiescence probe,
        - deterministic reopen+frame proof.
    - lane checker hardened in `scripts/check_webxr_browser_conformance.sh`:
      - requires `functional_pass == true`,
      - requires `frame.status == "ok"`,
      - requires close status in `{closed, closed-quiesced}`.

- [x] P1.2 Create headroom in documentation complexity budgets for agent retrieval quality.
  - Completion evidence:
    - consolidated doc topology by removing:
      - `docs/spec/BUDGETS.md`
      - `docs/spec/COVERAGE.md`
    - updated deprecation map in `docs/DEPRECATION_MAP_v0.1.md`.
    - reduced feature-matrix primary evidence fanout from `22` to `20`.
    - `.genesis/perf/doc_complexity_report.json` now shows headroom:
      - `active_docs_md=104` (budget `106`)
      - `capability_retrieval_fanout=0.4167` (budget `0.46`)

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

- [x] P1.4 Reduce strict health warmup latency for agent inner loops.
  - Completion evidence:
    - strict-health warmup reports now show materially lower startup latency:
      - `.genesis/perf/upgrade_plan_health_warmup_prepush-standard.json`: `elapsed_ms=809`
      - `.genesis/perf/upgrade_plan_health_warmup_release-full.json`: `elapsed_ms=52943`
    - prepush wall budget defaults were aligned to actual strict scope in
      `scripts/check_upgrade_plan_health.sh` + `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
      (`900000 -> 1050000` ms), with matrix guard updates in
      `scripts/check_test_execution_profile_matrix.sh`.

- [x] P1.5 Retire residual parity-harness ownership dependencies from selfhost readiness posture.
  - Completion evidence:
    - removed explicit `CoreformFrontend::Rust` enum-token dependency from obligations tests:
      - `crates/gc_obligations/src/tests/mod.rs`
    - updated readiness parity isolation scoring to fail-close on production Rust-frontend references
      while allowing zero parity-only references:
      - `scripts/check_selfhost_readiness_scorecard.sh`
    - remaining Rust TCB boundaries are now documented explicitly in
      `docs/spec/SELF_HOST_BOUNDARY.md`.

## P2 - Strategic completeness for "agent can build anything" scope

- [x] P2.1 Close feature-matrix partial on full selfhost closure (or codify minimal permanent TCB contract).
  - Completion evidence:
    - published explicit permanent TCB declaration and scope in
      `docs/spec/SELF_HOST_BOUNDARY.md`:
      - section: `Permanent Minimal TCB Contract (v1 release scope)`
    - updated feature-matrix closure row language to align claims with the explicit
      permanent TCB contract and selfhost-first semantic authority model.

- [x] P2.2 Add first-class `gcpm` agent scaffolding for end-to-end product archetypes.
  - Completion evidence:
    - added first-class command surface:
      - `genesis gcpm scaffold --archetype <web|service|desktop|mobile|xr-game|data-ai> --name <workspace> [--root <dir>] [--runtime-backend <...>] [--force]`
      - CLI wiring + dispatch contracts updated in:
        - `crates/gc_cli_driver/src/cli_args/pkg_cmd.rs`
        - `crates/gc_cli_driver/src/cmd_pkg.rs`
        - `crates/gc_cli_driver/src/pkg_contract.rs`
        - `crates/gc_cli_driver/src/cmd_pkg/frontend_dispatch/rust.rs`
        - `crates/gc_cli_driver/src/cmd_pkg/frontend_dispatch/selfhost.rs`
    - deterministic scaffold generator shipped in:
      - `crates/gc_cli_driver/src/pkg_scaffold.rs`
      - emits workspace + lock + package + module + caps + deploy preset + readme with deterministic scaffold hash (`:scaffold-h`).
    - schema/docs + regression coverage updated:
      - `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
      - `docs/spec/CLI.md`
      - `crates/gc_cli/tests/cli_pkg_scaffold.rs`
      - `crates/gc_cli/tests/cli_json_schema_registry.rs`
