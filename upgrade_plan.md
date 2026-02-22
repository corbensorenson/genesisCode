# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 2

## AI-First Authoring + Optimization (P2)

- [ ] P2.4 Add media/asset pipeline contracts for AI-generated games/apps
  - Evidence: host ABI families currently include gfx/gpu/audio/input/window but no media asset decode/encode contract family (`docs/spec/HOST_ABI_INDEX_v0.1.json`).
  - Exit criteria:
    - Add `core/media::*` / host ABI contracts for image/audio asset processing.
    - Add deterministic asset hashing/transcoding policies and constraints.
    - Add domain-kit workflows for asset import/build pipelines.

- [ ] P2.8 Tighten deterministic performance budgets for end-to-end agent workflows
  - Evidence: existing perf gates track core loops and selected suites, but not full user-facing multi-domain scenario latency budgets.
  - Exit criteria:
    - Add end-to-end scenario benchmarks (service + data + gfx + network).
    - Enforce median + p95 budgets with contention-aware sampling.
    - Fail release profiles when scenario budgets regress beyond configured thresholds.
