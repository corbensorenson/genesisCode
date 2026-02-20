# Agent Service Workflow

Deterministic end-to-end reference workflow for AI agents that need package-manager lifecycle + publish/sync + replay in selfhost-only mode.

## What It Covers

- `gcpm` lifecycle (`init`, `add`, `lock`, `install`) using pinned dependency snapshots.
- Policy-gated `pkg publish` to a local file-backed registry.
- `sync pull` in a consumer workspace.
- Effect-log replay determinism (`genesis run` + `genesis replay`).

## Run

From repo root:

```bash
bash examples/agent_service_workflow/workflow.sh
```

The script is deterministic and fails fast on policy, publish, sync, or replay drift.
