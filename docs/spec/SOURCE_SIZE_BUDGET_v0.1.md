# Source Size Budget v0.1

Status: normative maintainability guard for production Rust modules and selfhost/prelude `.gc` authoring sources.

## Goal

GenesisCode is AI-first. Production source files must stay within bounded size so automated
edits, review diffs, and semantic patching remain tractable.

## Policy

- Policy file: `policies/source_size_budget.toml`
- Current budget:
  - `rust_max_lines = 2200`
  - `gc_max_lines = 1800`
- Applies to:
  - `crates/**/*.rs`
  - all `.gc` files resolved from `gc_source_roots` (default: `prelude/modules`, `selfhost`, `prelude/prelude.gc`)
- Excludes:
  - paths containing `/tests/`, `/benches/`, `/examples/`
  - generated `.gc` artifacts listed in `gc_generated_exclude_paths` (currently `prelude/prelude.gc`)
  - runtime bootstrap uses build-time assembly from `prelude/modules/manifest.toml`; checked-in
    assembled artifacts are compatibility outputs, not authoring roots.

## Enforcement

- Guard script: `scripts/check_source_size_budget.sh`
- CI: enforced in `.github/workflows/ci.yml`
- Upgrade hard-gate mode: enforced in `scripts/check_upgrade_plan_health.sh` when open items reach 0.

## Refactor Rule

When a file approaches the budget, split by stable semantic boundaries:

1. capability/command domain
2. validation/parsing helpers
3. serialization/protocol adapters
4. test-only harness code

Avoid giant monolithic files with mixed concerns.
