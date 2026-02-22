# Write GenesisCode Skill Conformance v0.1

Executable quality gate for AI-authored GenesisCode workflows.

## Runner

- Script: `scripts/check_write_genesiscode_skill_conformance.sh`
- Report: `.genesis/perf/write_genesiscode_skill_conformance_report.json`
- History: `.genesis/perf/write_genesiscode_skill_conformance_history.jsonl`

## Inputs

- `scripts/check_agent_reference_workflows.sh` report:
  - `.genesis/perf/agent_capability_gauntlet_report.json`
- `scripts/check_agent_generative_workloads.sh` report:
  - `.genesis/perf/agent_generative_workloads_report.json`

## Rubric (100 points)

- `service` (20): `agent_service_workflow` must pass deterministic run/replay and include `service` + `package_publish_sync`.
- `game_loop` (20): `agent_long_running_gfx_loop_workflow` (or fallback `agent_interactive_gfx_compute_workflow`) must pass deterministic run/replay and include `graphics`.
- `gpu_compute` (20): `agent_gpu_compute_workflow` (or fallback `agent_compute_workflow`) must pass deterministic run/replay and include `gpu_compute`.
- `package_workflow` (20): `agent_multi_package_publish_workflow` must pass deterministic run/replay and include `package_publish_sync`.
- `generative_mutation_suite` (20): generated mutation report must be `ok`, have minimum case count, and have no parity/history-min failures.

Default pass threshold is `100/100` (`GENESIS_WRITE_SKILL_CONFORMANCE_MIN_SCORE`).

## Contract

- `kind = "genesis/write-genesiscode-skill-conformance-v0.1"`
- `ok`, `score`, `min_score`, `threshold_ok`
- `profile`, `runtime_profile`
- `gauntlet_report`, `generative_report`
- rubric detail rows for each required category

## Profile wiring

- `scripts/check_upgrade_plan_health.sh --profile prepush-standard`
- `scripts/check_upgrade_plan_health.sh --profile release-full`
