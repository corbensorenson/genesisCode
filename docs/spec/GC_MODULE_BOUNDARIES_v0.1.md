# GenesisCode `.gc` Module Boundaries v0.1

This document defines maintainability boundaries for source-of-truth `.gc` modules used by AI agents.

## Scope

Applies to:

- all `.gc` paths resolved from policy `gc_source_roots` (directories scanned recursively):
  - `/Users/corbensorenson/Documents/genesisCode/prelude/modules`
  - `/Users/corbensorenson/Documents/genesisCode/selfhost`
  - `/Users/corbensorenson/Documents/genesisCode/prelude/prelude.gc`

Generated artifacts are excluded:

- policy allowlist `gc_generated_exclude_paths` (currently `/Users/corbensorenson/Documents/genesisCode/prelude/prelude.gc`)

Notes:

- `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain.gc` remains a generated assembly artifact, but is now emitted in compact CoreForm form and stays within enforced `.gc` line budgets (no policy carve-out).
- `gc_prelude` bootstrap now assembles embedded prelude source from `prelude/modules/manifest.toml` at build time; runtime no longer consumes `prelude/prelude.gc` as its source of truth.

## Boundary Rules

- Keep modules domain-focused and composable:
  - `prelude/modules/00_*` for core data/effect/protocol helpers
  - `prelude/modules/10_*` for gfx/compute wrappers and runtime traces
  - `prelude/modules/20_*` for editor/tasking surfaces
  - `prelude/modules/30_*` for reusable high-level domain kits (service orchestration, data pipelines, network workflows, game-loop scaffolding)
  - `selfhost/cli_*` for CLI/runtime orchestration
  - `selfhost/{parse,canon,printer,hash}` for frontend core
  - `selfhost/stage1_*` and patch schema modules for optimization/rewrites
- Prefer adding a new module over extending an existing module past budget.
- Expose stable, small top-level entrypoints and keep helper internals local to each module.

## Budget Enforcement

`.gc` source budgets are enforced by:

- `/Users/corbensorenson/Documents/genesisCode/scripts/check_gc_source_size_budget.sh`
- policy file: `/Users/corbensorenson/Documents/genesisCode/policies/source_size_budget.toml`

Current policy tracks:

- `gc_max_lines`
- `gc_target_lines`
- generated-artifact exclusions
- explicit target-debt allowlist (`gc_target_exclude_paths`)

## AI-First Rationale

- Smaller, domain-scoped modules improve agent planning and reduce edit conflicts.
- Stable boundaries reduce prompt context size and increase rewrite reliability.
- Budget gates prevent silent drift into monolithic files that are hard for both agents and humans to maintain.
