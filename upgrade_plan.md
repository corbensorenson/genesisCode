# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 1

## P1 - Regulated Assurance Readiness (DO-178C / NASA NPR 7150.2 / IEC 62304)

- [ ] P1.8 Add structural coverage obligations suitable for high-assurance profiles (decision/MC/DC).
  Why this matters:
  - Current coverage obligation verifies symbol exercise, but not branch/decision/MC-DC style coverage needed by stricter regulated classes.
  Evidence:
  - `docs/spec/COVERAGE.md` now includes deterministic decision counters, but still lacks statement-site and MC/DC obligations.
  Progress this pass:
  - [x] Added deterministic decision counter collection in kernel evaluation paths (`if` branch outcomes in treewalk + compiled execution).
  - [x] Emitted decision counters in coverage evidence artifacts (per-test and aggregate structural section).
  - [x] Added kernel tests validating decision counters for treewalk and compiled execution.
  - [ ] Add per-site statement/decision coverage accounting with stable site identities.
  - [ ] Add MC/DC condition independence accounting and evidence schema.
  - [ ] Add profile mapping + fail-closed gates (`decision`, `mcdc`) to policy/obligation defaults.
  Exit criteria:
  - Add deterministic structural coverage collection (statement/decision and MC/DC profiles).
  - Emit canonical coverage evidence artifacts tied to commit/release provenance.
  - Define profile mapping (e.g., DAL/Class policy presets) and fail-closed gates.
