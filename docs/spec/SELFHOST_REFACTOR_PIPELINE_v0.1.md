# Selfhost Refactor Pipeline v0.1

Status: normative for deterministic selfhost module decomposition edits.

## Goal

Provide an in-repo, deterministic refactor workflow that can:

- split selfhost modules structurally,
- update `selfhost/toolchain_manifest.gc`,
- prove semantic equivalence of assembled selfhost source,
- re-verify run/replay invariants with a rebuilt toolchain artifact.

This pipeline is required for AI-first, high-frequency selfhost refactors without regressing into monolithic authoring.

## Tooling

- Refactor tool: `scripts/selfhost_refactor_pipeline.py`
- Guard gate: `scripts/check_selfhost_refactor_guard.sh`

## Commands

### Verify current selfhost modular state

```bash
python3 scripts/selfhost_refactor_pipeline.py verify
```

Behavior:

- Reads `selfhost/toolchain_manifest.gc` `:module-paths`.
- Assembles module sources in manifest order and computes canonical semantic hash via `genesis vcs hash`.
- Rebuilds a selfhost artifact via `genesis selfhost-artifact`.
- Runs deterministic `run` + `replay` smoke with `--selfhost-only --selfhost-artifact`.

### Split module tail deterministically

```bash
python3 scripts/selfhost_refactor_pipeline.py split-tail \
  --module selfhost/cli_pkg_runtime_v1.gc \
  --new-module selfhost/cli_pkg_runtime_tail_v1.gc \
  --split-form-index 320
```

Behavior:

- Splits a module at top-level form index `N`:
  - source keeps forms `[0..N)`,
  - new module receives forms `[N..end)`.
- Inserts `--new-module` into `:module-paths` immediately after `--module`.
- Enforces semantic-equivalence of assembled module sequence before/after split.
- Rebuilds artifact and runs run/replay verification.
- If any step fails, restores original files and manifest (transactional rollback).

## CI / Health Gate

`scripts/check_selfhost_refactor_guard.sh` enforces:

- `:module-paths` exists and has no duplicates.
- module count is above floor (`GENESIS_SELFHOST_MIN_MODULE_COUNT`, default `12`).
- `selfhost/toolchain.gc` is not used as an authoring source module.
- module files listed in manifest exist.
- full `verify` pipeline succeeds.

This gate is intended to prevent regressions into monolithic selfhost authoring and to keep replay-safety checks attached to structural refactors.
