# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-24

Scope:
- Keep only unresolved blockers for selfhost closure and AI-first agent productization.
- Remove completed work from this file; use git history and perf reports as closure evidence.
- Keep IDs synchronized with `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 1

## Completed This Pass (2026-02-24)

- [x] P1.2 Make selfhost profile guards deterministic against stale binaries in shared target dirs.
  Evidence: `scripts/selfhost_default_profile_guard.sh` only rebuilds when binaries are missing (`if [[ ! -x "$GEN" ]]` / `if [[ ! -x "$GWASI" ]]`), so stale prebuilt binaries can fail artifact decode while source-of-truth binaries pass.
  Done: guard now defaults to forced rebuild (`GENESIS_SELFHOST_DEFAULT_PROFILE_GUARD_FORCE_REBUILD=1`) before assertions, eliminating stale target-dir binary drift.

- [x] P1.3 Add full-selfhost cutover gate coverage to the default code-health path used for daily agent iteration.
  Evidence: `scripts/check_upgrade_plan_health.sh --profile agent-inner-loop` does not run `scripts/check_full_selfhost_cutover_profile.sh`; only `release-full` and `full-selfhost-cutover` profiles run that contract.
  Done: `agent-inner-loop` now runs `GENESIS_FULL_SELFHOST_CUTOVER_REFRESH=0 bash scripts/check_full_selfhost_cutover_profile.sh` and passes in health profile runs.

- [x] P1.4 Expand runtime skill-conformance verification to cover all required agent domains, not just a subset.
  Evidence: `docs/skill_pack/write_genesiscode_v1/manifest.json` requires 13 recipe domains, but `.genesis/perf/write_genesiscode_skill_conformance_report.json` runtime rubric currently validates only 4 domains (`service`, `graphics`, `gpu_compute`, `package_publish_sync`) plus generative suite.
  Done: `scripts/check_write_genesiscode_skill_conformance.sh` now validates all 13 manifest-required domains using gauntlet + runtime backend + host-bridge fault injection + GPU/XR productization + assurance reports, with per-domain rubric entries in `.genesis/perf/write_genesiscode_skill_conformance_report.json`.

- [x] P2.5 Tighten default GPU evidence requirements for agent productization profiles.
  Evidence: `.genesis/perf/agent_capability_gauntlet_report.json` recorded `require_gpu_device_backend=false` in default lanes; `.genesis/perf/gpu_xr_productization_kits_report.json` allowed `required_webxr_runtime_evidence=false` outside strict release profile.
  Done: prepush-standard gauntlet now enforces device-runtime backend evidence (`GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND=1`); `scripts/check_agent_reference_workflows.sh` defaults to strict GPU profile + device requirement for `prepush-standard`; `scripts/check_gpu_xr_productization_kits.sh` now defaults `GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE=1`, and prepush health lane passes with strict evidence.

- Progress note for P2.4 (not complete):
  Progress: split `crates/gc_registry/src/registry/client_impl.rs` into `crates/gc_registry/src/registry/client_impl/mod.rs` + focused submodules (`ping_and_store.rs`, `refs.rs`, `auth.rs`) and removed the deprecated monolith; also split `crates/gc_vcs/src/policy.rs` test payload into `crates/gc_vcs/src/policy/tests.rs`, reducing `policy.rs` to 678 lines. Decomposition migration inventory now tracks 14 migrated modules (`policies/source_decomposition_progress.toml` + `docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`), and both decomposition/migration plan checks pass.

## Critical Blockers (P1)

- None currently open.

## High-Impact Hardening (P2)

- [ ] P2.4 Expand decomposition coverage and split oversized production Rust modules for AI-maintainable structure.
  Evidence: decomposition policy now tracks 14 migrated modules (including recent splits for `crates/gc_registry/src/registry/client_impl/*` and `crates/gc_vcs/src/policy.rs`), while 25 production `crates/*/src/**/*.rs` files remain above 700 lines (for example `crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs`, `crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs`, `crates/gc_obligations/src/obligation_gfx.rs`).
  Done when: decomposition policy includes all oversized production modules with phase/status rows, and each oversized file is split under budget with parity tests.

## Evidence Anchors

- `scripts/selfhost_default_profile_guard.sh`
- `scripts/check_upgrade_plan_health.sh`
- `scripts/check_full_selfhost_cutover_profile.sh`
- `scripts/check_write_genesiscode_skill_conformance.sh`
- `docs/skill_pack/write_genesiscode_v1/manifest.json`
- `.genesis/perf/write_genesiscode_skill_conformance_report.json`
- `policies/source_decomposition_progress.toml`
- `.genesis/perf/agent_capability_gauntlet_report.json`
- `.genesis/perf/gpu_xr_productization_kits_report.json`
