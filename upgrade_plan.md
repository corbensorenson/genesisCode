# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 6

## Agent-First Productization Blockers

- [ ] P1.1 Replace smoke-only reference workflows with a scored agent capability gauntlet.
  - Why: “AI can build anything” requires breadth + depth validation, not just per-workflow success exits.
  - Evidence:
    - `scripts/check_agent_reference_workflows.sh` runs example workflows sequentially and succeeds on exit codes; it does not emit a scored capability report or enforce cross-domain minimums.
  - Done when:
    - add a deterministic gauntlet report artifact (e.g., `genesis/agent-capability-gauntlet-v0.1`) covering service, network/process, package publish/sync, graphics, and GPU compute scenarios;
    - CI fails if any required domain falls below declared success/replay/perf thresholds.

- [ ] P1.2 Tighten iteration SLOs for AI coding loops (current defaults remain too high).
  - Why: agent iteration speed degrades when default/high-signal loops are budgeted in multi-minute windows.
  - Evidence:
    - `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md` pins `changed-fast <= 5m` and `full-cross-host <= 20m`.
  - Done when:
    - establish stricter budgets (target: `changed-fast <= 2m`, `strict-golden <= 8m`, `full-cross-host <= 12m`) with history-p95 enforcement;
    - update profile scripts and CI gates to fail closed on regressions.

- [ ] P1.3 Consolidate spec/documentation surface for agent retrieval and reduce context noise.
  - Why: large doc sprawl increases retrieval ambiguity and agent prompt/context cost.
  - Evidence:
    - repository currently contains `91` Markdown docs total, `70` under `docs/spec`.
  - Done when:
    - publish a single normative agent authoring bundle entrypoint that supersedes split docs for common workflows;
    - add a drift/orphan guard that verifies every normative spec path is reachable from the bundle/index and legacy split docs are clearly marked.

- [ ] P1.4 Add CI drift guards for the GenesisCode authoring skill used by coding agents.
  - Why: AI-first authoring quality depends on keeping agent skill guidance synchronized with current schemas/capabilities.
  - Evidence:
    - authoring skill exists at `.agents/skills/genesiscode-authoring/SKILL.md`, but no guard script currently validates freshness or required links against active schema/capability indices.
  - Done when:
    - add a deterministic guard (script + test) that verifies required sections, required spec links, and capability/schema references in the skill file;
    - wire guard into health/profile gates.

## Runtime + Language Breadth

- [ ] P2.1 Expand high-level prelude/domain kits beyond core+gfx+editor foundations.
  - Why: low-level capability wrappers exist, but agent productivity for broad app classes needs richer high-level reusable contracts.
  - Evidence:
    - `prelude/modules/manifest.toml` currently lists only core/system/reachability + gfx/gpu + editor-oriented modules.
  - Done when:
    - add first-class high-level domain kits (service orchestration, data pipeline patterns, network workflow orchestration, game-loop scaffolding) with deterministic contract schemas;
    - migrate reference workflows to these kits instead of ad hoc per-example glue.

- [ ] P2.2 Broaden device-runtime GPU conformance beyond a single CI host class.
  - Why: “build anything” GPU confidence should not depend on one self-hosted Linux lane.
  - Evidence:
    - `.github/workflows/ci.yml` defines one `gpu_device_microbench` job (`self-hosted, linux, x64, gpu`).
  - Done when:
    - add at least one additional independent conformance lane (another OS/host class or equivalent deterministic runner) and compare report artifacts for contract parity;
    - fail release profile if required conformance lanes are unavailable or mismatched.
