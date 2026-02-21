# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 3

## AI-First Iteration + Runtime Performance

- [ ] P2.2 Harden AI iteration SLO gates against host contention noise.
  - Why: agent-facing iteration gates must be stable under normal workstation background load, not only isolated runs.
  - Evidence:
    - under shared-load execution, `check_ai_iteration_slo.sh` observed `incremental_warm_ms=2665` vs regression budget `2268`;
    - isolated rerun passed with `incremental_warm_ms=807`, indicating contention sensitivity.
  - Done when:
    - perf gate runner enforces measurement isolation (or robust multi-sample statistics) and documents contention policy;
    - the SLO check remains deterministic/reliable across repeated local runs.

## AI Authoring Surface + Assurance Readiness

- [ ] P2.4 Consolidate docs into an agent-first canonical set.
  - Why: AI coding agents perform better with smaller, canonical doc surfaces and fewer overlapping specs.
  - Evidence:
    - repository currently has `89` markdown files under `docs/`, increasing retrieval ambiguity.
  - Done when:
    - canonical docs index defines source-of-truth files per domain;
    - superseded/overlapping docs are merged or explicitly deprecated;
    - agent-facing onboarding path is reduced to a minimal, stable subset.

- [ ] P2.5 Close regulated-assurance process gaps beyond current partial alignment.
  - Why: requested government/high-assurance use cases require lifecycle-process evidence, not only runtime semantics.
  - Evidence:
    - `feature_matrix.md` marks `DO-178C DAL A/B`, `NASA NPR 7150.2 Class A/B`, and `IEC 62304 Class C` as `partial alignment`.
  - Done when:
    - `gcpm` emits certification-oriented assurance packs (trace matrix, independence attestations, qualified-tool manifest, coverage exports);
    - policy-gated release workflows can produce reproducible audit bundles for target standard/profile.
