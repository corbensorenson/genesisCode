# Testing Bundle v0.1

Canonical bundle for deterministic test profiles, schemas, and verification lanes.

## Included Specs

- `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- `docs/spec/TEST_SCHEMA.md`
- `docs/spec/PROPERTY_TESTS.md`
- `docs/spec/FUZZ_DIFFERENTIAL_HARDENING_v0.1.md`
- `docs/spec/PARITY_HARNESS.md`
- `docs/spec/AGENT_SCENARIO_PERF_v0.1.md`

## Agent Guidance

- Use changed-aware fast loop first (`scripts/test_changed_fast.sh`).
- Use strict/full lanes only for release-grade confidence and parity hardening.
