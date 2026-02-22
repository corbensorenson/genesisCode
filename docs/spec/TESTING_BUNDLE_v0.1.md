# Testing Bundle v0.1

Canonical bundle for deterministic test profiles, schemas, and verification lanes.

## Included Specs

- `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- `docs/spec/AGENT_GENERATIVE_WORKLOADS_v0.1.md`

## Agent Guidance

- Use changed-aware fast loop first (`scripts/test_changed_fast.sh`).
- Use strict/full lanes only for release-grade confidence and parity hardening.
- Test schema, property/fuzz posture, parity harness, and scenario SLO details are
  consolidated into `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`.
