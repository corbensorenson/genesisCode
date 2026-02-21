# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 1

## AI Authoring Surface + Assurance Readiness

- [ ] P2.5 Close regulated-assurance process gaps beyond current partial alignment.
  - Why: requested government/high-assurance use cases require lifecycle-process evidence, not only runtime semantics.
  - Evidence:
    - `feature_matrix.md` marks `DO-178C DAL A/B`, `NASA NPR 7150.2 Class A/B`, and `IEC 62304 Class C` as `partial alignment`.
  - Done when:
    - `gcpm` emits certification-oriented assurance packs (trace matrix, independence attestations, qualified-tool manifest, coverage exports);
    - policy-gated release workflows can produce reproducible audit bundles for target standard/profile.
