# Write GenesisCode Skill Conformance v0.1

Executable quality gate for AI-authored GenesisCode workflows.

## Runner

- Script: `scripts/check_write_genesiscode_skill_conformance.sh`
- Retained evidence producer: `scripts/update_write_genesiscode_skill_conformance_report.sh`
- Report: `.genesis/perf/write_genesiscode_skill_conformance_report.json`
- History: `.genesis/perf/write_genesiscode_skill_conformance_history.jsonl`

## Inputs

- `scripts/check_agent_reference_workflows.sh` report:
  - `.genesis/perf/agent_capability_gauntlet_report.json`
- `scripts/check_agent_generative_workloads.sh` report:
  - `.genesis/perf/agent_generative_workloads_report.json`
- runtime backend matrix: `.genesis/perf/runtime_backend_feature_matrix_report.json`
- host-bridge fault evidence: `.genesis/perf/host_bridge_fault_injection_report.json`
- GPU/XR productization evidence: `.genesis/perf/gpu_xr_productization_kits_report.json`
- assurance profile evidence: `.genesis/perf/assurance_profile_packs_report.json`

The check consumes these retained inputs and renders only to a private temporary
root. It fails with the exact corresponding `update_*` producer when evidence
is absent. Only `scripts/update_write_genesiscode_skill_conformance_report.sh`
appends retained conformance history.

## Rubric (100 points)

- Domain checks are manifest-driven from:
  - `docs/skill_pack/write_genesiscode_v1/manifest.json`
  - `distribution_requirements.required_recipe_domains`
- Each required domain maps to a deterministic workflow/report handler in
  `scripts/render_write_genesiscode_skill_conformance_report.sh`.
- Domain points are weighted evenly across all required domains.
- `generative_mutation_suite` is a strict pass/fail companion gate:
  generated mutation report must be `ok`, meet minimum case count, and have
  no parity/history-min failures.

Default pass threshold remains `100/100` (`GENESIS_WRITE_SKILL_CONFORMANCE_MIN_SCORE`).

## Contract

- `kind = "genesis/write-genesiscode-skill-conformance-v0.1"`
- `ok`, `score`, `min_score`, `threshold_ok`
- `profile`, `runtime_profile`
- `gauntlet_report`, `generative_report`, `input_evidence` SHA-256 identities
- `history_samples`, optional `previous_score`, `score_delta`
- rubric detail rows for each required category

## Profile wiring

- `scripts/check_upgrade_plan_health.sh --profile prepush-standard`
- `scripts/check_upgrade_plan_health.sh --profile release-full`
