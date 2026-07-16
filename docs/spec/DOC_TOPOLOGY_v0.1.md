# Documentation Topology v0.1

Canonical documentation source-of-truth map for AI-first GenesisCode authoring.

## Authoring

- Canonical root: `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- Skill contract: `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`
- Skill pack: `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- Executable skill conformance: `docs/spec/WRITE_GENESISCODE_SKILL_CONFORMANCE_v0.1.md`

Owner: Language + agent authoring maintainers.

## Runtime

- Canonical root: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
- GPU/graphics bundle: `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
- GPU compute bundle: `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
- GFX runtime bundle: `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
- Runtime backend policy: `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md`
- High-churn migration map: `docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`

Owner: Runtime/effects maintainers.

## Assurance

- Canonical root: `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
- Profile packs: `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`
- Standards crosswalk: `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`

Owner: Assurance + release maintainers.

## Published Presentation

- Public site: `https://corbensorenson.github.io/genesisCode/`
- Site manifest and navigation: `_quarto.yml`
- Curated learning sources: `index.qmd`, `learn/`, and `guides/`
- Reader and agent routing map: `learn/documentation-map.qmd`
- Generated reference sources: `reference/` and `llms.txt`
- Deterministic generator: `scripts/render_quarto_reference.py`
- Generated-reference freshness gate: `scripts/check_doc_topology_drift.sh`
- Rendered completeness, link, fragment, accessibility, sitemap, and provenance validator: `scripts/check_quarto_site.py`
- Real-browser responsive, keyboard, reduced-motion, search, and console validator: `scripts/check_quarto_browser.mjs`
- Commit-bound artifact stamper: `scripts/stamp_quarto_site.py`
- Post-deploy public attestor: `scripts/check_quarto_deployment.py`
- Pages workflow: `.github/workflows/docs-site.yml`

The website is a non-normative projection of topology-owned repository sources. A
generated page, search result, card, index record, or `llms.txt` entry cannot override
the specification, policy, schema, ledger, or executable conformance source it links.
The generator records source identities, enumerates the complete frozen symbol,
host-operation, diagnostic, schema, example, and documentation inventories, and fails
closed on stale output. The site renders every tracked Markdown authority under
`docs/` in addition to the curated learning spine, so discoverability does not create
a competing abbreviated manual. Every rendered artifact carries a source commit,
clean/dirty source state, reference-index digest, and whole-artifact digest. The Pages
workflow pins third-party actions, publishes only the artifact that passed the offline
validator, and then checks the live learning, reference, agent, sitemap, provenance,
and custom-404 surfaces against the deployed commit.

Owner: Documentation + release maintainers.

## Operations

- Live backlog: `upgrade_plan.md`
- Strategic roadmap: `ROADMAP.md`
- Roadmap execution contract: `docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md` (`Roadmap Execution Manifest` section)
- Roadmap execution schema: `docs/spec/ROADMAP_EXECUTION_MANIFEST_v0.1.schema.json`
- Generated roadmap execution graph: `docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json`
- Evidence envelope authority: `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` (`Evidence Envelope Profile` section)
- Evidence schemas: `docs/spec/GENESIS_EVIDENCE_PREDICATE_v0.1.schema.json`, `docs/spec/GENESIS_EVIDENCE_STATEMENT_v0.1.schema.json`, `docs/spec/GENESIS_SLSA_BUILD_v1.schema.json`, `docs/spec/GENESIS_EVIDENCE_BUNDLE_v0.1.schema.json`
- Authenticated evidence vector: `docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json`
- Independent verifier schemas: `docs/spec/GENESIS_EVIDENCE_TRUST_POLICY_v0.1.schema.json`, `docs/spec/GENESIS_ARTIFACT_HASH_TREE_v0.1.schema.json`, `docs/spec/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.schema.json`
- Independent verifier vectors: `docs/program/evidence/GENESIS_EVIDENCE_ARTIFACT_TREE_v0.1.json`, `docs/program/evidence/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.json`, and `docs/program/evidence/artifact/genesis-example.bin`
- Independent verifier implementation: `tools/genesis-evidence-verifier/` with `scripts/check_genesis_evidence_verifier.sh`
- Adversarial matrix schema/data: `docs/spec/EVIDENCE_ADVERSARIAL_MATRIX_v0.1.schema.json` and `docs/program/EVIDENCE_ADVERSARIAL_MATRIX_v0.1.json`
- Cross-boundary adversarial gate: `scripts/check_evidence_adversarial_matrix.sh`, binding standalone-verifier vectors to runtime replay mutation coverage
- Evidence storage authority: `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` (`Evidence storage and immutable publication` section)
- Evidence storage policy/schema: `policies/evidence_storage_classes_v0.1.json` and `docs/spec/EVIDENCE_STORAGE_CLASSES_v0.1.schema.json`
- In-tree fixture classification: `docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json` with `docs/spec/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.schema.json`
- Immutable release renderer/checker: `scripts/render_evidence_release_asset.sh`, `scripts/lib/evidence_storage.py`, and `scripts/check_evidence_storage_classes.sh`
- Prerequisite authority: `genesis.prerequisites.json` with `docs/spec/PREREQUISITES_v0.1.md` and `docs/spec/PREREQUISITES_v0.1.schema.json`
- Read-only prerequisite diagnostic/gate: `scripts/genesis_prerequisites.sh`, `scripts/lib/prerequisite_manifest.py`, and `scripts/check_prerequisite_manifest.sh`
- Reference host authority: `policies/reference_host_profiles_v0.1.json` with `docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md#reference-host-profiles`, `docs/spec/REFERENCE_HOST_PROFILES_v0.1.schema.json`, and `docs/spec/REFERENCE_HOST_OBSERVATION_v0.1.schema.json`
- Portable host probe/gate: `scripts/render_reference_host_observation.sh`, `scripts/lib/reference_host_profiles.py`, and `scripts/check_reference_host_profiles.sh`
- Normalized runtime workload authority: `policies/perf/roadmap_workloads_v0.1.json` with `docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md#normalized-roadmap-workloads`, `docs/spec/ROADMAP_WORKLOADS_v0.1.schema.json`, and `benchmarks/roadmap/v0.1/`
- Runtime workload normalization gate: `scripts/lib/roadmap_workloads.py` and `scripts/check_roadmap_workloads.sh`
- Signed E0 baseline authority: `docs/program/evidence/roadmap-baselines/` with `docs/spec/ROADMAP_BASELINE_STATEMENT_v0.1.schema.json` and `docs/spec/ROADMAP_BASELINE_BUNDLE_v0.1.schema.json`
- Baseline lifecycle: `scripts/update_roadmap_baseline.sh` is the sole append-only capture/signing entrypoint; `scripts/check_roadmap_baseline.sh`, `scripts/lib/roadmap_baseline.py`, and the standalone verifier are read-only
- Release-note authority: `policies/release_notes_v0.1.json` with `docs/spec/RELEASE_NOTES_POLICY_v0.1.schema.json`, `docs/spec/RELEASE_NOTES_v0.1.schema.json`, and `docs/program/RELEASE_NOTES_v0.2.0.json`
- Release-note lifecycle: `scripts/update_release_notes.sh` is the explicit deterministic producer; `scripts/check_release_notes.sh` rejects stale sources, omitted facts, and unsupported authority claims
- Agent training profile authority: `policies/gc_agent_profile_v0.3.json` with `docs/spec/GC_AGENT_PROFILE_v0.3.schema.json` and resolved `docs/spec/GC_AGENT_PROFILE_v0.3.json`
- Agent profile lifecycle: `scripts/update_agent_authoring_bundle.sh profile` explicitly resolves source identities; `scripts/check_gc_agent_profile.sh` executes parser, evaluator, package, and resource cases and rejects unsupported-surface drift
- Compact agent card authority: `policies/gc_agent_core_card_v0.3.json` generates `docs/spec/GC_AGENT_CORE_CARD_v0.3.md` and `docs/spec/GC_AGENT_CORE_CARD_v0.3.json`; `scripts/check_gc_agent_core_card.sh` enforces freshness, the ASCII/token upper bound, and parser conformance
- Task-card authority: `policies/gc_agent_task_cards_v0.3.json` generates `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md` and embedded `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`; `scripts/check_gc_agent_task_cards.sh` enforces AB-2 budgets, source anchors, fail-closed intent selection, and production/reference parity
- Agent symbol-index authority: `policies/gc_agent_symbol_index_v0.3.json` generates `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json` under the closed `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.schema.json`; `scripts/check_gc_agent_symbol_index.sh` enforces complete frozen-surface coverage and exact production lookup
- Dependency mirror authority: `genesis.dependency-mirror.json` with `docs/spec/DEPENDENCY_MIRROR_v0.1.md`, `docs/spec/DEPENDENCY_MIRROR_v0.1.schema.json`, and `docs/spec/DEPENDENCY_MIRROR_MANIFEST_v0.1.schema.json`
- Dependency mirror lifecycle: `scripts/update_dependency_mirror.sh` is the sole fetch entrypoint; `scripts/check_dependency_mirror_contract.sh` is read-only conformance; `scripts/test_offline_dependency_mirror.sh` proves a clean build under kernel network denial
- Roadmap execution policy: `policies/roadmap_execution_v0.1.json`
- Release notes: `CHANGELOG.md`
- Versioning policy: `docs/spec/VERSIONING_v0.1.md`
- Version-surface authority: `genesis.version-surfaces.json`
- Version compatibility and migration contract: `docs/spec/VERSION_SURFACES_v0.1.md` with `docs/spec/VERSION_SURFACES_v0.1.schema.json`
- Version-surface drift gate: `scripts/check_version_surfaces.sh`
- Release smoke contract: `docs/spec/RELEASE_SMOKE_v0.1.md`
- Active risk rollup: `docs/status/REDTEAM_REPORT.md`
- Agent-first onboarding spine: `docs/AGENT_ONBOARDING_v0.1.md`
- Canonical capability/evidence ledger: `docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json`
- Generated capability compatibility, maturity, and release-claim view: `feature_matrix.md`
- Generated semantic selfhost authority view: `docs/status/SELFHOST_AUTHORITY_v0.1.md`
- Normative check/update lifecycle: `docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md`
- Canonical check/update boundary policy: `policies/check_update_boundary_v0.1.json`
- Generated check/update audit: `docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json`
- Optional local selfhost-readiness scorecard: `.genesis/perf/selfhost_readiness_report.json` (explicit updater: `scripts/update_selfhost_readiness_scorecard_report.sh`)
- Optional local bootstrap-retirement report: `.genesis/perf/bootstrap_retirement_gate_report.json` (explicit updater: `scripts/update_bootstrap_retirement_gate_report.sh`)
- Optional local full-selfhost-cutover report: `.genesis/perf/full_selfhost_cutover_profile_report.json` (explicit updater: `scripts/update_full_selfhost_cutover_profile_report.sh`)
- Optional local remote-registry parity report: `.genesis/perf/remote_registry_runtime_parity_report.json` (explicit updater: `scripts/update_remote_registry_runtime_parity_report.sh`)
- Optional local GCPM contract-pack report: `.genesis/perf/gcpm_operation_contract_pack_report.json` (explicit updater: `scripts/update_gcpm_operation_contract_pack_report.sh`)
- Optional local VCS self-host report: `.genesis/perf/vcs_selfhost_contract_report.json` (explicit updater: `scripts/update_vcs_selfhost_contract_report.sh`)
- Optional local symbol-ownership report: `.genesis/perf/selfhost_symbol_ownership_report.json` (explicit updater: `scripts/update_selfhost_symbol_ownership_report.sh`)
- Optional local CLI-diagnostics report: `.genesis/perf/cli_diagnostics_contract_report.json` (explicit updater: `scripts/update_cli_diagnostics_contract_report.sh`)
- Optional local foundation-stdlib report: `.genesis/perf/foundation_stdlib_conformance_report.json` (explicit updater: `scripts/update_foundation_stdlib_conformance_report.sh`)
- Optional local fuzz/differential report: `.genesis/perf/fuzz_differential_hardening_report.json` (explicit updater: `scripts/update_fuzz_differential_hardening_report.sh`)
- Optional local WASM production-surface report: `.genesis/perf/wasm_production_surface_report.json` (explicit updater: `scripts/update_wasm_production_surface_report.sh`)
- Optional local WebXR browser report: `.genesis/perf/webxr_browser_conformance_report.json` (explicit updater: `scripts/update_webxr_browser_conformance_report.sh`)
- Optional local GFX runtime report set: `.genesis/perf/gfx_runtime_profile_report.json` (explicit updater: `scripts/update_gfx_runtime_profile_report.sh`)
- Optional local production CLI help report: `.genesis/perf/production_cli_help_surface_report.json` (explicit updater: `scripts/update_production_cli_help_surface_report.sh`)
- Optional local production CLI parse report: `.genesis/perf/production_cli_parse_surface_report.json` (explicit updater: `scripts/update_production_cli_parse_surface_report.sh`)
- Optional local agent gauntlet report: `.genesis/perf/agent_capability_gauntlet_report.json` (explicit updater: `scripts/update_agent_reference_workflows_report.sh`)
- Optional local generative-workload report: `.genesis/perf/agent_generative_workloads_report.json` (explicit updater: `scripts/update_agent_generative_workloads_report.sh`)
- Optional local agent-scenario report: `.genesis/perf/agent_scenario_perf_report.json` (explicit updater: `scripts/update_agent_scenario_perf_report.sh`)
- Optional local agent runtime-parity report: `.genesis/perf/agent_workflow_runtime_parity_report.json` (explicit updater: `scripts/update_agent_workflow_runtime_parity_report.sh`)
- Optional local runtime-microbench report set: `.genesis/perf/runtime_microbench_metrics.json` (explicit updater: `scripts/update_runtime_microbench_budgets_report.sh`)
- Optional local hot-path budget report set: `.genesis/perf/hot_path_metrics.json` (explicit updater: `scripts/update_hot_path_budgets_report.sh`)
- Optional local general performance-budget report set: `.genesis/perf/perf_budget_metrics.json` (explicit updater: `scripts/update_perf_budgets_report.sh`)
- Optional local evaluator-workload report set: `.genesis/perf/runtime_workload_bench_report.json` (explicit updater: `scripts/update_runtime_workload_budgets_report.sh`)
- Optional local AI iteration-SLO report set: `.genesis/perf/ai_iteration_slo_metrics.json` (explicit updater: `scripts/update_ai_iteration_slo_report.sh`)
- Optional local AI stress-suite report set: `.genesis/perf/ai_stress_suite_metrics.json` (explicit updater: `scripts/update_ai_stress_suite_report.sh`)
- Optional local backend-starter workflow report set: `.genesis/perf/backend_starter_workflows_report.json` (explicit updater: `scripts/update_backend_starter_workflows_report.sh`)
- Optional local domain-starter registry report: `.genesis/perf/domain_starter_registry_bootstrap_report.json` (explicit updater: `scripts/update_domain_starter_registry_bootstrap_report.sh`)
- Optional local full-cross-host report set: `.genesis/perf/full_cross_host_profile_report.json` (explicit updater: `scripts/update_full_cross_host_profile_budget_report.sh`)
- Optional local target-runtime evidence set: `.genesis/perf/gcpm_target_runtime_evidence_report.json` (explicit updater: `scripts/update_gcpm_target_runtime_pipelines_report.sh`)
- Optional local runtime-backend matrix report set: `.genesis/perf/runtime_backend_feature_matrix_report.json` (explicit updater: `scripts/update_runtime_backend_feature_matrix_report.sh`)
- Optional local write-skill conformance report set: `.genesis/perf/write_genesiscode_skill_conformance_report.json` (explicit updater: `scripts/update_write_genesiscode_skill_conformance_report.sh`)
- Optional local tracked-decomposition parity report: `.genesis/perf/source_decomposition_tracked_parity_report.json` (explicit updater: `scripts/update_source_decomposition_tracked_parity_report.sh`)
- Optional local large-workspace performance report set: `.genesis/perf/large_workspace_agent_perf_report.json` (explicit updater: `scripts/update_large_workspace_agent_perf_report.sh`)
- Optional local aggregate health profile report set: `.genesis/perf/upgrade_plan_health_profile_report.json` (explicit updater: `scripts/update_upgrade_plan_health_report.sh`)
- Optional local GPU compute-profile report set: `.genesis/perf/gpu_compute_runtime_profile.json` (explicit updater: `scripts/update_gpu_compute_runtime_profile_report.sh`)
- Optional local GPU device-conformance report set: `.genesis/perf/gpu_device_conformance_report.json` (explicit updater: `scripts/update_gpu_compute_device_conformance_report.sh`)
- Optional local GPU device-parity report: `.genesis/perf/gpu_device_lane_parity_report.json` (explicit updater: `scripts/update_gpu_device_conformance_lane_parity_report.sh`)
- Optional local GPU device-matrix report: `.genesis/perf/gpu_device_conformance_matrix_report.json` (explicit updater: `scripts/update_gpu_device_conformance_matrix_report.sh`)
- Optional local GPU/GFX headroom report set: `.genesis/perf/gpu_gfx_headroom_conformance_report.json` (explicit updater: `scripts/update_gpu_gfx_headroom_conformance_report.sh`)
- Optional local GPU/XR productization report: `.genesis/perf/gpu_xr_productization_kits_report.json` (explicit updater: `scripts/update_gpu_xr_productization_kits_report.sh`)
- Optional local task-concurrency stress report set: `.genesis/perf/task_concurrency_stress_report.json` (explicit updater: `scripts/update_task_concurrency_stress_report.sh`)
- Optional local host-bridge fault report set: `.genesis/perf/host_bridge_fault_injection_report.json` (explicit updater: `scripts/update_host_bridge_fault_injection_report.sh`)
- Optional local doc-complexity report: `.genesis/perf/doc_complexity_report.json` (explicit updater: `scripts/update_doc_complexity_report.sh`)
- Optional local migration-plan report: `.genesis/perf/selfhost_gc_migration_plan_report.json` (explicit updater: `scripts/update_selfhost_gc_migration_plan_report.sh`)
- Optional local source-decomposition report: `.genesis/perf/source_decomposition_progress_report.json` (explicit updater: `scripts/update_source_decomposition_progress_report.sh`)
- Optional local host-API contract report: `.genesis/perf/host_api_evolution_contract_report.json` (explicit updater: `scripts/update_host_api_evolution_contract_report.sh`)
- Optional local qualification-lineage report: `.genesis/perf/tool_qualification_lineage_report.json` (explicit updater: `scripts/update_tool_qualification_lineage_report.sh`)
- Optional local assurance-profile report: `.genesis/perf/assurance_profile_packs_report.json` (explicit updater: `scripts/update_assurance_profile_packs_report.sh`)
- Optional local assurance-crosswalk report: `.genesis/perf/assurance_standards_crosswalk_report.json` (explicit updater: `scripts/update_assurance_standards_crosswalk_report.sh`)
- Optional local changed-loop report: `.genesis/perf/test_changed_fast_metrics.json` (explicit updater: `scripts/update_test_changed_fast_metrics.sh`)
- Optional local panic-guard report: `.genesis/perf/no_user_panics_report.json` (explicit updater: `scripts/update_no_user_panics_report.sh`)
- Optional local artifact-freshness report: `.genesis/perf/selfhost_artifact_fresh_report.json` (explicit updater: `scripts/update_selfhost_artifact_fresh_report.sh`)
- Optional local dashboard-freshness report: `.genesis/perf/selfhost_dashboard_fresh_report.json` (explicit updater: `scripts/update_selfhost_dashboard_fresh_report.sh`)
- Canonical docs index: `docs/INDEX.md`
- Leaf ownership registry: `docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md`
- Complexity targets: `docs/spec/DOC_COMPLEXITY_TARGETS_v0.1.md`

Owner: Release ops maintainers.

## Update Workflow

1. Edit canonical topology-owned docs first.
2. Run `bash scripts/update_capability_status_views.sh` for ledger-derived views, then update any other derived docs or skill pointers in the same change.
3. Run `python3 scripts/render_quarto_reference.py` to update site indexes after an authority or inventory changes.
4. Run drift checks:
   - `bash scripts/check_doc_topology_drift.sh`
   - `bash scripts/check_doc_complexity_budget.sh`
   - `bash scripts/check_feature_matrix_gap_hygiene.sh`
   - `bash scripts/check_redteam_report.sh`
   - `quarto render && python3 scripts/check_quarto_site.py`
5. Only then mark backlog items complete in `upgrade_plan.md`.
