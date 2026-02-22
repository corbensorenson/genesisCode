# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file tracks unresolved blockers only. Completed work belongs in git history and release notes.

Open checklist items: 12

## P0 - Immediate blockers

- [x] P0.1 Replace false-green selfhost readiness scoring with capability-truth scoring.
  - Completion:
    - Added `critical_gate_truth` scoring dimension in `scripts/check_selfhost_readiness_scorecard.sh`.
    - Readiness now consumes critical runtime truth signals:
      `agent_capability_gauntlet_report.json`,
      `production_cli_help_surface_report.json`,
      and live deployment pipeline execution via `scripts/check_gcpm_target_runtime_pipelines.sh`.
    - Readiness can no longer report green from routing checks alone while critical reports are red.

- [ ] P0.2 Stabilize GPU/GFX agent workflows under constrained disk/temp headroom.
  - Evidence:
    - `.genesis/perf/agent_capability_gauntlet_report.json` failing workflows:
      `agent_gpu_compute_workflow` (exit 70) and `agent_interactive_gfx_compute_workflow` (exit 1) with `No space left on device`.
    - Local headroom is tight (`df -h` shows 99% capacity), and current workflows rely on unrestricted `mktemp` usage.
  - Definition of done:
    - GPU/GFX workflows pass deterministically on both normal and low-headroom environments.
    - Workflow runtime temp roots are deterministic/configurable and preflighted before execution.

- [ ] P0.3 Upgrade `gcpm build --target` from runtime-runner contracts to executable target artifacts.
  - Evidence:
    - `crates/gc_cli_driver/src/pkg_workspace_ops_build.rs` emits `runtime/runtime_contract.gc`, `runtime/boot.sh`, `runtime/smoke.sh` for `ios|android|edge|service-runtime`.
    - `scripts/check_gcpm_target_runtime_pipelines.sh` validates deterministic runner scripts (`contract-ok`, `boot-ok`, `smoke-ok`), not platform-native build/sign/package outputs.
  - Definition of done:
    - Targets emit executable artifacts with target-specific package formats and deterministic provenance/signature metadata.
    - Conformance lanes validate artifact execution on target-appropriate runtime/tooling, not script placeholders.

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

- [ ] P1.2 Bind tool-qualification test artifacts to executed test lineage.
  - Evidence:
    - `parse_test_artifacts` in `crates/gc_cli_driver/src/pkg_assurance_ops.rs` validates only `id=<64-hex>` format.
    - `handle_tool_qualification` marks test entries as `:result :pass` from caller-provided hashes without mandatory run-manifest linkage.
  - Definition of done:
    - `gcpm qualify` requires cryptographic links to executed test manifests, policy snapshot, and run metadata.
    - Qualification fails closed when referenced test artifacts are missing, mismatched, or out-of-lineage.

- [ ] P1.3 Deliver first-class dependency solver/range semantics in `gcpm`.
  - Evidence:
    - `crates/gc_cli_driver/src/cli_args/pkg_cmd.rs` marks `gcpm lock` and `gcpm update` as `local-only v0.1`.
    - Current selector model is commit/snapshot/ref-centric, with no semver/range solving comparable to mature PM workflows.
  - Definition of done:
    - Deterministic range solver with conflict diagnostics, lock reproducibility, and policy-aware registry resolution.
    - Workspace update flows support selective upgrades and auditable constraint rationale.

- [ ] P1.4 Bring production CLI help-surface gate below budget with build reuse.
  - Evidence:
    - `.genesis/perf/production_cli_help_surface_report.json` currently `ok=false` (`elapsed_ms=310587`, `budget_ms=300000`).
    - `scripts/lib/release_bin.sh` always executes `cargo build --release` for each requested binary.
  - Definition of done:
    - Release-help gate consistently meets budget with cached binary reuse and deterministic invalidation.
    - p95 history budget is green in both CI and local strict profiles.

- [ ] P1.5 Add disk-headroom-aware preflight and recovery paths across heavy gates.
  - Evidence:
    - Recent gate failures include `No space left on device` during temp directory creation and build outputs.
    - Current heavy scripts (`check_agent_reference_workflows.sh`, `check_production_cli_help_surface.sh`) do not enforce shared disk preflight contracts.
  - Definition of done:
    - Shared preflight library enforces minimum temp/build headroom and emits actionable remediation.
    - Gates degrade safely (or fail fast with explicit reason) instead of mid-run infra crashes.

- [ ] P1.6 Reduce AI-maintainability risk in high-churn runtime/compiler surfaces.
  - Evidence:
    - Source decomposition report (`.genesis/perf/source_decomposition_progress_report.json`) still tracks multiple near-threshold Rust modules (919-983 LOC).
    - Repository remains Rust-heavy for core behavior (`435` Rust files vs `211` `.gc` files), which slows agent-first language evolution.
  - Definition of done:
    - Additional decomposition targets and stricter module budgets for high-churn domains.
    - Clear migration plan for selfhost-critical logic into GC-authored modules where runtime parity is proven.

- [ ] P1.7 Consolidate and normalize documentation for agent-first authoring.
  - Evidence:
    - The repo currently contains `134` Markdown files, increasing discovery/maintenance overhead for agent execution.
    - Planning, assurance, and runtime docs are still split across multiple overlapping status/spec files.
  - Definition of done:
    - Canonical doc map with deterministic redirects and reduced duplicate guidance.
    - Single agent-first onboarding spine for language semantics, runtime profiles, packaging, assurance, and deployment.

- [ ] P1.8 Complete bootstrap retirement path and production fallback removal.
  - Evidence:
    - Production claims remain selfhost-first with parity artifacts still present; explicit retirement cutover for residual bootstrap/fallback surfaces is not yet finalized.
  - Definition of done:
    - Residual bootstrap-only Rust paths moved to `/old_bootstrap` with non-production scope.
    - Production binaries and release profiles fail closed if deprecated fallback paths are reintroduced.

## P2 - Strategic completeness for "agent can build anything" scope

- [ ] P2.1 Publish `write_genesisCode_skill.md` + executable conformance pack after selfhost closure.
  - Evidence:
    - Current skill-pack contracts exist, but the requested single detailed cross-agent skill file is not yet published as a canonical artifact.
  - Definition of done:
    - Add `write_genesisCode_skill.md` with strict patterns for architecture, contracts, testing, debugging, perf, and assurance.
    - Add conformance tests so Codex/Claude-style agents can be scored on generated GC quality.

- [ ] P2.2 Raise assurance-pack closure from "partial alignment" to auditable high-assurance readiness.
  - Evidence:
    - Feature matrix currently marks regulated standards as partial alignment and explicitly notes external certification responsibilities.
    - Tool/test lineage hardening and stronger trace closure are still open (P1.2).
  - Definition of done:
    - Add trace closure from requirements -> low-level artifacts -> tests -> emitted binaries/object equivalence evidence.
    - Add independent-verifier workflow gates aligned with DAL A/B, NASA Class A/B, and IEC Class C evidence expectations.

- [ ] P2.3 Deepen first-class non-graphics GPU + XR/WebXR productization surface.
  - Evidence:
    - Compute/XR capabilities are present, but packaging/runtime productization for deployable XR and heavy compute workloads is not yet closed end-to-end.
  - Definition of done:
    - Publish dedicated non-gfx GPU kit workflows (data/ML/simulation) and XR deploy/test templates.
    - Ensure wasm/webxr and native runtime lanes share deterministic authoring contracts and replay evidence.
