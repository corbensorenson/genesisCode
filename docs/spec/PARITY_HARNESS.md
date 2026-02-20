# Rust Parity Harness Scope v0.1

Status: normative for bootstrap/parity tooling boundaries.

## Purpose

Rust frontend/engine execution paths exist only to compare selfhost behavior against historical
Rust semantics during cutover. They are not part of the production `genesis`/`genesis_wasi`
workflow.

## Allowed Entrypoints

- `target/debug/genesis_parity`
- `target/debug/genesis_wasi_parity`

Production binaries must reject:

- `--engine rust`
- `--coreform-frontend rust`

## Enforcement

- `scripts/check_rust_engine_compat.sh`:
  - fails if scripts/tests/workflows invoke rust engine/frontend paths without dedicated parity
    binaries (or an explicit `RUST_ENGINE_COMPAT_EXCEPTION` marker).
- `.github/workflows/ci.yml` runs this guard for every CI profile.
- `scripts/check_upgrade_plan_health.sh` includes the same guard in zero-open hard-gate mode.

## Developer Workflow

- Use `genesis`/`genesis_wasi` for production flows (selfhost-only defaults).
- Use parity binaries only for:
  - differential regression checks
  - migration/cutover validation against prior rust semantics

Parity harness artifacts and wrappers are bootstrap-only and may be retired after final selfhost
cutoff.
