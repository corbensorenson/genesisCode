# Agent GPU Compute Workflow

Deterministic, compute-first selfhost reference workflow for AI agents.

## What It Covers

- Package obligations and evidence via `genesis test --pkg`.
- Task scheduler round-trip (`core/task::*`).
- Canonical compute capability surface (`gpu/compute::submit`) without `gfx/gpu::*`.
- Effect log replay determinism (`genesis run` + `genesis replay`).
- VCS hash surface (`genesis vcs hash --engine selfhost`).

## Run

From repo root:

```bash
bash examples/agent_gpu_compute_workflow/workflow.sh
```

The workflow is deterministic and fails on any contract mismatch.
