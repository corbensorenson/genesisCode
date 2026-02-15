# WASI Tooling Build v0.2 (Rust Bootstrap -> `genesis.wasm`)

Goal: run GenesisCode tooling "on top of wasm" using WASI (`wasm32-wasip1`), without depending on
JS bindings. This is the shortest path toward a wasm-first, eventually self-hosted toolchain.

This build is outside the kernel purity boundary: it does filesystem I/O to read `.gc` files and
print results, but kernel evaluation remains pure.

## Crate

- `crates/gc_wasi_cli` builds a WASI CLI binary: `genesis_wasi.wasm`.
- Current command surface is intentionally minimal (WASI bootstrap):
  - `genesis fmt <file> [--check]`
  - `genesis eval <file>`
  - `genesis pack --pkg <package.toml>`
  - `genesis test --pkg <package.toml> [--caps <caps.toml>]`
  - `genesis run <file> --caps <caps.toml> [--log <out.gclog>]` (local effects only)
  - `genesis replay <file> --log <log.gclog> [--store <dir>]`
  - `genesis store --caps <caps.toml> [--log <out.gclog>] {put|get|has} ...` (local store only)
  - `genesis refs --caps <caps.toml> [--log <out.gclog>] {get|list|set|delete} ...` (local refs only)
  - `genesis pkg --caps <caps.toml> [--log <out.gclog>] {init|add|lock|update|install|verify|list|info|snapshot|export|import} ...` (local-only; no sync)
  - `genesis vcs hash --in <file>`

The interface mirrors the native `genesis` CLI for these commands:
- stable exit codes (see `docs/spec/CLI.md`)
- `--json` envelope support

Notes:
- Networking is denied in the WASI bootstrap. `core/sync::*` is not supported under WASI.

## Build

```bash
rustup target add wasm32-wasip1
cargo build -p gc_wasi_cli --target wasm32-wasip1 --release
```

Convenience script:

```bash
bash scripts/build_wasi.sh
```

## Run (wasmtime)

The WASI module needs a preopened directory to read/write files:

```bash
wasmtime --dir . target/wasm32-wasip1/release/genesis_wasi.wasm --help
wasmtime --dir . target/wasm32-wasip1/release/genesis_wasi.wasm fmt tests/spec/coreform/app_sugar.in.gc --check
```

## Smoke / Equivalence

Run the deterministic smoke test, which asserts equivalence with the native `genesis` CLI:

```bash
wasmtime --version
bash scripts/wasi_smoke.sh
```
