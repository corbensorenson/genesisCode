# Agent Compute Workflow

Deterministic end-to-end reference workflow for AI agents that need package + VCS + task + GPU capability execution in selfhost-only mode.

## What It Covers

- Package-level obligations and evidence via `genesis test --pkg`.
- Task scheduler capabilities (`core/task::*`).
- GPU bridge capabilities (`gfx/gpu::*`) with deterministic bridge responses.
- Effect log replay determinism (`genesis run` + `genesis replay`).
- VCS hashing surface (`genesis vcs hash --engine selfhost`).

## Run

From repo root:

```bash
bash examples/agent_compute_workflow/workflow.sh
```

The script is deterministic and fails fast on any mismatch.
