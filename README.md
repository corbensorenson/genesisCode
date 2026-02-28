# GenesisCode (v0.2)

GenesisCode is an AI-first language/runtime project focused on deterministic evaluation, sealed error/effect boundaries, capability-gated host effects, and reproducible execution evidence.

The workspace builds a CLI binary named `genesis`.

## Design goals

- Pure, deterministic kernel evaluator (`Gλ` style)
- Unforgeable `UNHANDLED` / `EFFECT` / `ERROR` protocol values via seals
- Deny-by-default effect runner with deterministic logs and replay checks
- Package/obligation/evidence workflows designed for agent-driven iteration
- Strict hardening gates (panic guards, capability conformance, replay integrity)

## Repository layout

- `crates/`: Rust workspace crates (kernel, CLI, effects, obligations, patches, etc.)
- `prelude/`: Prelude modules and language surface helpers
- `selfhost/`: Selfhost artifact/toolchain material
- `docs/`: Specs, handoff, policy, and status docs
- `scripts/`: test/health/profile gates and CI-style contract checks

## Quickstart

Build everything:

```sh
cargo build --workspace
```

Run the CLI:

```sh
cargo run -p gc_cli -- --help
```

Format CoreForm:

```sh
cargo run -p gc_cli -- fmt path/to/file.gc
cargo run -p gc_cli -- fmt --check path/to/file.gc
```

Evaluate pure code:

```sh
cargo run -p gc_cli -- eval path/to/file.gc
```

Run effects with capability policy and deterministic log:

```sh
cargo run -p gc_cli -- run path/to/file.gc --caps caps.toml --log out.gclog
cargo run -p gc_cli -- replay path/to/file.gc --log out.gclog
```

Package/testing flow:

```sh
cargo run -p gc_cli -- test --pkg path/to/package.toml --caps path/to/caps.toml
cargo run -p gc_cli -- pack --pkg path/to/package.toml
```

Apply semantic patch:

```sh
cargo run -p gc_cli -- apply-patch path/to/change.gcpatch --pkg path/to/package.toml --caps path/to/caps.toml
```

## Local development gates

Fast changed-aware loop:

```sh
bash scripts/test_changed_fast.sh
```

Alias / broader loop:

```sh
bash scripts/test_fast.sh
bash scripts/test_fast.sh --full
```

Strict profile used for release-quality agent readiness:

```sh
bash scripts/check_upgrade_plan_health.sh --profile prepush-standard
```

## Specs and core docs

- Docs index: `docs/INDEX.md`
- Primary design/paper: `docs/PAPER_v0.2.md`
- Technical handoff: `docs/TECH_HANDOFF.md`
- Seals/dispatch/replay spec: `docs/spec/SEALS_DISPATCH_REPLAY.md`
- Patch schema spec: `docs/spec/PATCH_SCHEMA.md`
- Capability surface matrix: `feature_matrix.md`

## License

Dual licensed under either:

- Apache-2.0 (`LICENSE-APACHE`)
- MIT (`LICENSE-MIT`)

See `LICENSE` for the dual-license notice.
