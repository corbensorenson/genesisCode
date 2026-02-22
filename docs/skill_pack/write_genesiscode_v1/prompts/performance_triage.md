# Prompt: Performance Triage

Use this prompt when reducing runtime/profile gate latency without reducing determinism.

## Required Inputs

- `.genesis/perf/*` reports for impacted lanes
- `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- affected gate scripts in `scripts/`

## Required Outputs

- top hotspot list with elapsed/budget deltas
- concrete runtime/cache/sharding changes
- SLO guard updates with fail-closed regression checks

## Minimum Verification

- `bash scripts/check_runtime_backend_feature_matrix.sh`
- `bash scripts/check_perf_budgets.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
