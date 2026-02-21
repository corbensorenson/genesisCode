# GenesisCode Red-Team Report

Last updated: 2026-02-21

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P0.1` Production binaries do not yet have explicit backend-capable feature profile wiring for first-party GPU/desktop lanes.
  - Next action: define and enforce backend-capable vs headless production feature profiles.
- `P0.2` `device-runtime` does not yet cover full canonical GPU lifecycle operations.
  - Next action: implement full lifecycle coverage (or explicitly split semantics) and add replay tests for lifecycle ops.
- `P0.3` GPU backend naming and policy vocabulary is split (`device-runtime` vs `device-bridge`) across runtime, microbench, docs, and CI.
  - Next action: unify backend terminology and aliases across all surfaces.
- `P0.4` `gcpm env` still depends on pre-populated local store artifacts and cannot hydrate missing locked deps in-place.
  - Next action: add deterministic lock-hydration flow for missing env artifacts.
- `P1.1` Local health gate default can skip all checks whenever upgrade backlog is non-zero.
  - Next action: always run a minimum mandatory local gate set even during active backlog work.
- `P1.2` Fast iteration loops remain slower than target envelopes (`test_changed_fast`/`dev-fast` wall time).
  - Next action: reduce default wall time through targeted sharding/cache/warm-path improvements.
- `P1.3` CI does not yet exercise all production backend feature combinations in `gc_effects`/`gc_cli`.
  - Next action: add matrix coverage for headless/gpu/desktop/combined feature profiles.
- `P1.4` Large production module hotspots still hinder AI-driven edits and review isolation.
  - Next action: continue decomposition on highest-churn >1k-line modules.
- `P1.5` Health profile auto-sharding defaults are limited to prepush profile.
  - Next action: enable deterministic sharding defaults for dev-fast/release-full profiles where safe.
