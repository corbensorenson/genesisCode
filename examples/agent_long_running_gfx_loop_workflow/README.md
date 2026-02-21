# Agent Long-Running GFX Loop Workflow

Deterministic long-running frame loop workflow for agent regression coverage.

This workflow validates:
- package obligations in selfhost mode
- repeated `gfx/time::frame-tick` execution under capability policy
- deterministic run/replay log parity
