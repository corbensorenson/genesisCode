# WASM Build (Pure Kernel) v0.2

GenesisCode keeps the kernel pure. The WASM target builds the **parser + canonicalizer + pure evaluator**
so CoreForm can be formatted, hashed, and evaluated in browser or other WASM hosts without effects.

Effectful programs are supported via a host bridge step/resume protocol; see `docs/spec/WASM_HOST_BRIDGE.md`.
For wasm-first CLI tooling (WASI), see `docs/spec/WASI.md`.

## Crate

- `crates/gc_wasm` builds a `cdylib` for `wasm32-unknown-unknown`.
- It depends only on `gc_coreform`, `gc_kernel`, and `gc_prelude` (no runner, no filesystem, no networking).

## Exported API

The WASM module exports these functions via `wasm-bindgen`:

- `fmt_coreform_module(src: &str) -> Result<String, JsValue>`
  - Parse, canonicalize, and return canonical `.gc` bytes with a trailing newline.
- `fmt_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Run the self-hosted CoreForm toolchain inside the kernel to format a module.
  - `step_limit=0` means “no limit”.
- `hash_coreform_module(src: &str) -> Result<String, JsValue>`
  - Parse, canonicalize, and return the 32-byte module hash as 64-hex.
- `hash_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Run the self-hosted CoreForm toolchain inside the kernel to hash a module.
  - `step_limit=0` means “no limit”.
- `eval_coreform_module(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Pure evaluation (no effects runner). `step_limit=0` means “no limit”.
  - If evaluation produces an effect program, returns an error telling the caller to use the host runner.

For effectful programs, the WASM module exports a stateful runtime that supports step/resume:

- `new Runtime(step_limit: u32)`
  - `step_limit=0` means “no limit”.
- `Runtime.eval_module(src: &str) -> Result<JsValue, JsValue>`
  - Parses/canonicalizes/evaluates and returns the first step result (`done` or `effect`).
- `Runtime.step() -> Result<JsValue, JsValue>`
  - Advances until `done` or `effect` (errors if a pending effect hasn't been responded to yet).
- `Runtime.respond_data(resp_term_src: &str) -> Result<JsValue, JsValue>`
  - Responds with a CoreForm datum (parsed as a single term).
- `Runtime.respond_denied() -> Result<JsValue, JsValue>`
  - Responds with a sealed `core/caps/denied` ERROR constructed inside the kernel.
- `Runtime.respond_error(code: &str, message: &str) -> Result<JsValue, JsValue>`
  - Responds with a sealed ERROR constructed inside the kernel.

The step/resume semantics and hashing requirements are specified in `docs/spec/WASM_HOST_BRIDGE.md`.

All outputs are deterministic given the same inputs.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build -p gc_wasm --target wasm32-unknown-unknown
```

## Node (wasm-bindgen) Smoke

To generate Node bindings and run a deterministic smoke test:

```bash
cargo install wasm-bindgen-cli --version 0.2.108 --locked
bash scripts/wasm_bindgen_node.sh
node scripts/wasm_node_smoke.mjs
```

## Browser (wasm-bindgen) Smoke

To generate Web bindings and run a headless browser determinism smoke test:

```bash
cargo install wasm-bindgen-cli --version 0.2.108 --locked
bash scripts/wasm_bindgen_web.sh

# JS deps for headless browser (Playwright)
npm ci
npx playwright install chromium
node scripts/wasm_web_smoke.mjs
```
