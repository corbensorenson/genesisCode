# WASM Build (Pure Kernel) v0.2

GenesisCode keeps the kernel pure. The WASM target builds the **parser + canonicalizer + pure evaluator**
so CoreForm can be formatted, hashed, and evaluated in browser or other WASM hosts without effects.

## Crate

- `crates/gc_wasm` builds a `cdylib` for `wasm32-unknown-unknown`.
- It depends only on `gc_coreform`, `gc_kernel`, and `gc_prelude` (no runner, no filesystem, no networking).

## Exported API

The WASM module exports these functions via `wasm-bindgen`:

- `fmt_coreform_module(src: &str) -> Result<String, JsValue>`
  - Parse, canonicalize, and return canonical `.gc` bytes with a trailing newline.
- `hash_coreform_module(src: &str) -> Result<String, JsValue>`
  - Parse, canonicalize, and return the 32-byte module hash as 64-hex.
- `eval_coreform_module(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Pure evaluation (no effects runner). `step_limit=0` means “no limit”.
  - If evaluation produces an effect program, returns an error telling the caller to use the host runner.

All outputs are deterministic given the same inputs.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build -p gc_wasm --target wasm32-unknown-unknown
```

