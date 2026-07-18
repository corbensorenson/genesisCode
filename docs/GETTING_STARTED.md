# Getting Started (GenesisCode v0.2)

This walkthrough uses the real toolchain and a small example package in `examples/hello_pkg/`.

## 1) Build

```sh
cargo build --workspace
```

The preferred test runner is `cargo nextest` when installed; local scripts auto-fallback to `cargo test` when it is absent. Install `cargo-nextest` before long local sessions if you want the same runner CI uses.

## 2) CoreForm Formatting And Evaluation

Canonical formatting is stable and idempotent:

```sh
cargo run -p gc_cli -- fmt examples/hello_pkg/hello.gc --check
cargo run -p gc_cli -- eval examples/hello_pkg/hello.gc
```

## 3) Run Package Obligations (Tests, Determinism, Typecheck, Translation Validation)

The example package is already pinned (module hashes recorded in `package.toml`), so `test` can run immediately:

```sh
cargo run -p gc_cli -- test --pkg examples/hello_pkg/package.toml
```

This writes evidence artifacts into `examples/hello_pkg/.genesis/store/` and prints the package acceptance artifact hash.

If you edit `examples/hello_pkg/hello.gc`, re-pin module hashes first:

```sh
cargo run -p gc_cli -- pack --pkg examples/hello_pkg/package.toml
```

## 4) Effects With Deterministic Logs And Replay

The `examples/effects_demo/read.gc` program performs a single filesystem read:

```sh
cargo run -p gc_cli -- run examples/effects_demo/read.gc --caps examples/effects_demo/caps.toml --log examples/effects_demo/read.gclog
cargo run -p gc_cli -- replay examples/effects_demo/read.gc --log examples/effects_demo/read.gclog
```

Both commands print the same final value, and `replay` hard-fails if the log doesn’t match the program.

## 5) Package Snapshots And `.gpk` Export/Import

Create a `:vcs/snapshot` for the example package and store it in
`examples/hello_pkg/.genesis/store/`. Run the package commands from the package
directory so package paths stay inside the caps sandbox:

```sh
(
  cd examples/hello_pkg
  SNAP="$(cargo run -p gc_cli --manifest-path ../../Cargo.toml -- pkg --caps toolcaps.toml snapshot --pkg package.toml)"
  echo "$SNAP"
)
```

Export a shallow bundle and import it back into the local store (idempotent):

```sh
(
  cd examples/hello_pkg
  SNAP="$(cargo run -p gc_cli --manifest-path ../../Cargo.toml -- pkg --caps toolcaps.toml snapshot --pkg package.toml)"
  mkdir -p .tmp
  cargo run -p gc_cli --manifest-path ../../Cargo.toml -- pkg --caps toolcaps.toml export --snapshot "$SNAP" --out .tmp/hello.gpk
  cargo run -p gc_cli --manifest-path ../../Cargo.toml -- pkg --caps toolcaps.toml import --input .tmp/hello.gpk
)
```

## 6) WASI (Run Tooling On WASM)

Build the WASI bootstrap CLI:

```sh
# genesis-doc-skip: downloads the checksum-pinned optional SDK and exports its build environment
eval "$(bash scripts/install_wasi_sdk.sh --print-shell-env)"
# genesis-doc-skip: requires optional wasm32-wasip1 target installation
rustup target add wasm32-wasip1
cargo build -p gc_wasi_cli --target wasm32-wasip1 --release
```

Then run inside `wasmtime` (requires a preopened directory):

```sh
# genesis-doc-skip: requires optional wasmtime runtime and WASI build artifact
wasmtime --dir . target/wasm32-wasip1/release/genesis_wasi.wasm test --pkg examples/hello_pkg/package.toml
```

## 7) Browser WASM (Pure Kernel + Host Bridge)

The pure kernel + stepping interface is exposed via `wasm-bindgen` (`crates/gc_wasm`).

See:
- `docs/spec/WASM.md` for build and smoke instructions
- `docs/spec/WASM_HOST_BRIDGE.md` for the normative step/resume protocol

## 8) Graphics Demos (2D UI, 3D Scene, Hybrid View)

Run the end-to-end `.gc` graphics demos:

```sh
cargo run -p gc_cli -- eval examples/gfx_demos/ui_app.gc
cargo run -p gc_cli -- eval examples/gfx_demos/scene3d.gc
cargo run -p gc_cli -- eval examples/gfx_demos/hybrid_web.gc
```

See `docs/spec/GPU_GFX_BUNDLE_v0.1.md` for canonical demo details and test coverage.
