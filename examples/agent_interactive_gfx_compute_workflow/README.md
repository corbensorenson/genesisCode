# Agent Interactive GFX + Compute Workflow

Deterministic end-to-end selfhost workflow for an interactive surface that uses
window/input/audio and canonical compute.

This workflow uses first-party runtime backends only (no external host bridge
script required).

## What It Covers

- Package obligations/evidence via `genesis test --pkg`.
- Interactive capability families:
  - `gfx/window::*`
  - `gfx/input::*`
  - `gfx/audio::*`
- Canonical compute capability: `gpu/compute::submit`.
- Effect-log replay determinism (`genesis run` + `genesis replay`).
- VCS hash determinism (`genesis vcs hash --engine selfhost`).

## Run

From repo root:

```bash
bash examples/agent_interactive_gfx_compute_workflow/workflow.sh
```
