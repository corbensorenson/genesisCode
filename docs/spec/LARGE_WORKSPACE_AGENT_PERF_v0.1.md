> Bundle Entry: `docs/spec/TESTING_BUNDLE_v0.1.md`

# Large Workspace Agent Perf v0.1

## Goal

Enforce an AI-authoring performance lane over a generated large workspace so
`gcpm` operations remain viable for high-module-count agent projects.

## Gate Command

```bash
bash scripts/check_large_workspace_agent_perf.sh
```

The gate is wired into:

- `scripts/check_upgrade_plan_health.sh --profile release-full`

## Workload Contract

- Generate a deterministic workspace with `GENESIS_LARGE_WORKSPACE_MODULE_COUNT`
  modules (default: `10000`).
- Measure and enforce budgets for:
  - `gcpm lock --strict`
  - `gcpm build --pkg <package.toml> --target <target>`
  - `gcpm test --pkg <package.toml>`
  - `selfhost-artifact --out <file>` refresh

Default build target:

- `GENESIS_LARGE_WORKSPACE_BUILD_TARGET=service-runtime`

## Reports

- metrics report:
  - `.genesis/perf/large_workspace_agent_perf_report.json`
  - kind: `genesis/large-workspace-agent-perf-v0.1`
- runtime report:
  - `.genesis/perf/large_workspace_agent_runtime_report.json`
  - history: `.genesis/perf/large_workspace_agent_runtime_history.jsonl`
  - kind: `genesis/large-workspace-agent-runtime-v0.1`

## Budget Knobs

- `GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_LOCK_MS` (default `90000`)
- `GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_BUILD_MS` (default `300000`)
- `GENESIS_LARGE_WORKSPACE_BUDGET_GCPM_TEST_MS` (default `240000`)
- `GENESIS_LARGE_WORKSPACE_BUDGET_SELFHOST_REFRESH_MS` (default `300000`)
- aggregate lane budget:
  - `GENESIS_LARGE_WORKSPACE_RUNTIME_BUDGET_MS` (default `900000`)

## Determinism and CI

- The workspace corpus is generated deterministically from module count.
- Cargo target dir is configured via `scripts/lib/cargo_target_dir.sh`.
- Disk preflight uses `GENESIS_PERF_DISK_STRICT_MODE` and fails closed in strict
  profile contexts.
