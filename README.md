# GenesisCode (v0.2)

GenesisCode is a research language/runtime with:
- a canonical surface syntax (CoreForm) with stable formatting and hashing
- a pure kernel evaluator (Gλ-style) with deterministic seals
- hardened protocol values (UNHANDLED/EFFECT/ERROR) and contract dispatch
- capability-based effects with deterministic logs + replay checking
- packages, obligations, evidence store, and semantic patches

This repo is a Rust workspace. The CLI binary is `genesis`.

## Quickstart

Build:
```sh
cargo build --workspace
```

Format a CoreForm file:
```sh
cargo run -p gc_cli -- fmt path/to/file.gc
cargo run -p gc_cli -- fmt --check path/to/file.gc
```

Evaluate a pure program/module:
```sh
cargo run -p gc_cli -- eval path/to/file.gc
```

Run effects (deny-by-default unless allowed in `caps.toml`) and produce a deterministic `.gclog`:
```sh
cargo run -p gc_cli -- run path/to/file.gc --caps caps.toml --log out.gclog
cargo run -p gc_cli -- replay path/to/file.gc --log out.gclog
```

Run package obligations (writes artifacts into `.genesis/store/` under the package directory):
```sh
cargo run -p gc_cli -- test --pkg path/to/package.toml --caps path/to/caps.toml
cargo run -p gc_cli -- pack --pkg path/to/package.toml
```

Apply a semantic patch:
```sh
cargo run -p gc_cli -- apply-patch path/to/change.gcpatch --pkg path/to/package.toml --caps path/to/caps.toml
```

## Normative Spec

Normative “lock-in” behavior lives in:
- `docs/spec/SEALS_DISPATCH_REPLAY.md`
- `docs/spec/PATCH_SCHEMA.md`

Additional schemas:
- `docs/spec/CAPS_TOML.md`
- `docs/spec/PACKAGE_TOML.md`
- `docs/spec/GCLOG_SCHEMA.md`
- `docs/spec/TEST_SCHEMA.md`

## License

Dual-licensed under Apache-2.0 OR MIT.
