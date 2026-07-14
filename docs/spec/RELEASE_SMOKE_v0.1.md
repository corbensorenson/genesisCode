# Release Smoke Contract v0.1

Minimal fail-closed release readiness checks for a GenesisCode source checkout.

## Command

Run:

```bash
bash scripts/check_release_smoke.sh
```

The default smoke is intentionally lightweight and safe on a dirty local checkout. Set `GENESIS_RELEASE_SMOKE_PACKAGE_DRY_RUN=1` to also run full `cargo package --workspace --allow-dirty --no-verify` packaging.

## Required Checks

`scripts/check_release_smoke.sh` must verify:

- `docs/spec/VERSIONING_v0.1.md`, `docs/spec/RELEASE_SMOKE_v0.1.md`, and `CHANGELOG.md` exist.
- `scripts/check_release_notes.sh` proves the retained machine-readable release facts and generated changelog block exactly match canonical inputs and contain no unsupported success or authority claim.
- All workspace crates report the same version through `cargo metadata --no-deps --format-version 1`.
- `cargo package --workspace --allow-dirty --no-verify --list` succeeds, proving package file selection is coherent without publishing.
- Native CLI help remains callable through `cargo run -p gc_cli -- --help` and exposes `Usage: genesis`.
- WASI CLI help remains callable through `cargo run -p gc_wasi_cli --bin genesis_wasi -- --help` and exposes `Usage: genesis_wasi`.
- Native and default WASI CLI `--version` output is exactly `genesis <workspace-version>`.
- `cargo run -p gc_wasi_cli -- --version` works without `--bin`, proving the production default binary is unambiguous.
- The intended local install path remains documented as `cargo install --path crates/gc_cli --locked --root .cargo-install-target`.

## Publish Boundary

This project is not publish-ready just because the smoke passes. Publishing requires a release owner to also run the full profile, review package contents, verify changelog migration notes, and confirm the registry policy in `docs/spec/REGISTRY_POLICY.md`.

Local install smoke command:

```bash
cargo install --path crates/gc_cli --locked --root .cargo-install-target
```
