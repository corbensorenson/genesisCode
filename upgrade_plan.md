# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-24

Scope:
- List only unresolved blockers/risk-reduction work for selfhost closure and AI-first productization.
- Remove completed items; rely on git history and perf artifacts for closure evidence.
- Machine-readable source of unresolved IDs: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 2

## Selfhost Closure (Rust Ownership Still In Critical Path)

- [ ] P2.1 Move `crates/gc_obligations/src/obligation_exec.rs` from `phase-1 in-progress` to `phase-3 migrated` by routing production obligation orchestration through `.gc` ownership and archiving Rust semantic sidecars to parity-only paths.
- [ ] P2.2 Move `crates/gc_cli_driver/src/semantic_workspace.rs` from `phase-1 in-progress` to `phase-3 migrated` with GC-owned planning/edit graph logic as source of truth for agent workspace mutation workflows.
- [x] P2.3 Move `crates/gc_patches/src/lib.rs` from `phase-1 in-progress` to `phase-3 migrated` so semantic patch construction/normalization is GC-owned in production execution paths.

### Current execution batch (2026-02-24)

- [x] P2.1.a Migrated production obligation report assembly for `core/obligation::unit-tests`, `core/obligation::determinism`, and `core/obligation::capabilities-declared` into GC-owned prelude contracts in `prelude/modules/30_service_orchestration.gc` (`core/obligation::unit-tests-report`, `core/obligation::determinism-report`, `core/obligation::capabilities-declared-report`).
- [x] P2.1.b Routed `crates/gc_obligations/src/obligation_exec.rs` to fail-closed on those prelude contracts (no Rust report-shape fallback) and derive `unit-tests` pass/fail from the contract-produced report payload.
- [x] P2.1.c Added regression coverage in `crates/gc_obligations/src/tests/mod.rs` (`core_obligation_report_builders_match_exec_report_shapes`) and validated with `cargo test -p gc_obligations -- --nocapture`.
- [x] P2.1.d Added GC-owned obligation orchestration contract `core/obligation::plan` in `prelude/modules/30_service_orchestration.gc` with deterministic first-seen dedupe semantics and explicit rejected-entry signaling.
- [x] P2.1.e Routed production obligation execution sequencing in `crates/gc_obligations/src/obligations/types_api.rs` through `obligation_plan_symbols` (fail-closed contract decode in `crates/gc_obligations/src/obligation_exec.rs`) instead of direct manifest vector iteration.
- [x] P2.1.f Added regression coverage for contract + runtime planning (`core_obligation_plan_contract_dedupes_and_preserves_order`, `obligation_plan_symbols_routes_through_gc_contract`) and revalidated with:
  `cargo test -p gc_prelude --test prelude_modularization -- --nocapture`,
  `cargo test -p gc_obligations -- --nocapture`,
  `cargo test -p gc_cli --test cli_semantic_edit -- --nocapture`.
- [x] P2.1.g Upgraded `core/obligation::plan` in `prelude/modules/30_service_orchestration.gc` to enforce a GC-owned allowlist of supported obligations, reject unknown obligation symbols at plan time, and keep deterministic first-seen ordering for accepted entries.
- [x] P2.1.h Removed Rust-side unknown-obligation fallback result synthesis in `crates/gc_obligations/src/obligations/types_api.rs`; execution now fails closed if `core/obligation::plan` emits unsupported entries, with regression coverage in `crates/gc_obligations/src/tests/mod.rs` (`core_obligation_plan_contract_rejects_unknown_obligations`, `obligation_plan_symbols_rejects_unknown_obligations`).

- [x] P2.3.a Made selfhost `core/cli::validate-patch` authoritative in `gc_patches` production execution; removed silent Rust fallback on `"unknown :op"` validation failures.
- [x] P2.3.b Added frontend-aware patch validation API (`validate_patch_term_with_frontend`) and wired semantic workspace refactor-plan validation through the selected CoreForm frontend.
- [x] P2.3.c Added regression coverage proving poisoned selfhost `validate-patch` now hard-fails apply-patch instead of falling back to Rust acceptance.
- [x] P2.3.d Removed the Rust-only `validate_patch_term` entrypoint from `gc_patches`; all patch validation callsites now pass explicit frontend + limits.
- [x] P2.2.a Added semantic-edit selfhost poisoning regression (`cli_semantic_edit`) proving refactor-plan fails closed when selfhost `core/cli::validate-patch` rejects patch schema.
- [x] P2.2.b Removed a broken/partial `core/cli::semantic-workspace-graph-analyze` dependency path that produced runtime unbound-symbol failures, restored deterministic workspace-graph generation in `crates/gc_cli_driver/src/semantic_workspace.rs`, and revalidated `cargo test -p gc_cli --test cli_semantic_edit -- --nocapture`.
- [x] P2.2.c Revalidated the deterministic Rust-owned workspace graph derivation path after contract extraction regressions and kept semantic-edit workflows fail-closed/green via `cargo test -p gc_cli --test cli_semantic_edit -- --nocapture`.
- [x] P2.2.d Added dedicated semantic workspace prelude module `prelude/modules/36_semantic_workspace.gc` with full `core/cli::semantic-workspace-graph-analyze` implementation (duplicate-owner detection, edge-event derivation, unresolved-symbol tracking), wired through `prelude/modules/manifest.toml`, and regenerated `prelude/prelude.gc`.
- [x] P2.2.e Restored contract-driven workspace graph derivation in `crates/gc_cli_driver/src/semantic_workspace.rs` (strict payload encode/decode, fail-closed missing-binding and shape validation) as the production source of truth for semantic graph analysis.
- [x] P2.2.f Added prelude-level regression coverage in `crates/gc_prelude/tests/prelude_semantic_workspace.rs` and revalidated with:
  `cargo test -p gc_prelude --test prelude_modularization -- --nocapture`,
  `cargo test -p gc_prelude --test prelude_semantic_workspace -- --nocapture`,
  `cargo test -p gc_cli --test cli_semantic_edit -- --nocapture`,
  `cargo test -p gc_obligations -- --nocapture`.
- [x] P2.2.g Split production workspace-graph contract encode/decode helpers out of `crates/gc_cli_driver/src/semantic_workspace.rs` into `crates/gc_cli_driver/src/semantic_workspace_contract.rs`, reducing hot-module size under decomposition budget (`490 <= 700`) while preserving fail-closed `core/cli::semantic-workspace-graph-analyze` routing.
- [x] P2.3.e Migrated production `rename-symbol` and `split-module` refactor transforms to selfhost contracts (`core/cli::rename-symbol-forms`, `core/cli::split-module-forms`) in `selfhost/patch_schema_refactor_v1.gc`.
- [x] P2.3.f Fixed selfhost refactor contract output-shape root causes (literal vector misuse + malformed curried loop application) and regenerated `selfhost/toolchain.gc` + freshness metadata so `gc_patches` consumes corrected artifact behavior.
- [x] P2.3.g Added selfhost `core/cli::rewrite-meta-list-forms` implementation plus supporting refactor helpers in `selfhost/patch_schema_refactor_v1.gc`; validated parse via `gc_coreform` diag and regenerated `selfhost/toolchain.gc`.
- [x] P2.3.h Wired production `PatchOp::RewriteMetaList` execution through selfhost contract (`core/cli::rewrite-meta-list-forms`), added poison-regression coverage, and removed Rust runtime fallback for rewrite-imports/exports execution.
- [x] P2.3.i Added dedicated selfhost manifest move primitive (`core/cli::manifest-apply-move-module`) in `selfhost/patch_schema_manifest_v1.gc`, wired `PatchOp::MoveModule` through selfhost manifest mutation when selfhost frontend is active, and added fail-closed poison regression (`apply_patch_selfhost_move_module_uses_manifest_contract`).
- [x] P2.3.j Added selfhost `core/cli::migrate-contract-signature-forms` in `selfhost/patch_schema_refactor_v1.gc`, switched production `:migrate-contract-signature` execution to that contract for selfhost frontend, and added fail-closed poison regression (`apply_patch_selfhost_migrate_contract_signature_uses_refactor_contract`).
- [x] P2.3.k Regenerated `selfhost/toolchain.gc` + freshness metadata and validated end-to-end with `cargo test -p gc_patches -- --nocapture`.
- [x] P2.3.l Aligned decomposition ledger phase metadata for `crates/gc_patches/src/lib.rs` to `phase-3 migrated` in `policies/source_decomposition_progress.toml` to match enforced production selfhost ownership.

## Evidence Anchors

- `policies/source_decomposition_progress.toml`
- `docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`
- `docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`
- `docs/spec/SELF_HOST_BOUNDARY.md`
- `docs/spec/TYPES.md`
- `crates/gc_cli_driver/src/cmd_registry.rs`
- `crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
- `crates/gc_effects/src/policy_tests.rs`
- `.genesis/perf/agent_capability_gauntlet_report.json`
- `.genesis/perf/gpu_gfx_headroom_conformance_report.json`
