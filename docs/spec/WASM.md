> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# WASM Build (Pure Kernel) v0.2

GenesisCode keeps the kernel pure. The WASM target builds the **parser + canonicalizer + pure evaluator**
so CoreForm can be formatted, hashed, and evaluated in browser or other WASM hosts without effects.

Effectful programs are supported via a host bridge step/resume protocol; see `docs/spec/WASM_HOST_BRIDGE.md`.
For wasm-first CLI tooling (WASI), see `docs/spec/WASI.md`.

## Crate

- `crates/gc_wasm` builds a `cdylib` for `wasm32-unknown-unknown`.
- It depends on `gc_coreform`, `gc_kernel`, `gc_prelude`, and `gc_opt` (for in-kernel Stage-1/Stage-2 gating paths), with no filesystem/network runners embedded in the module.

## Exported API

The WASM module exports these functions via `wasm-bindgen`:

- `fmt_coreform_module(src: &str) -> Result<String, JsValue>`
  - Parse, canonicalize, and return canonical `.gc` bytes with a trailing newline.
- `fmt_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Run the self-hosted CoreForm toolchain inside the kernel to format a module.
  - `step_limit=0` means “no limit”.
  - Toolchain bootstrap is not charged against `step_limit`; the limit applies to formatting the input module.
  - On `wasm32`, this API fails closed unless an explicit artifact is provided via `fmt_coreform_module_selfhost_with_artifact`.
- `fmt_coreform_module_selfhost_with_artifact(src: &str, artifact_src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Selfhost format using a caller-supplied toolchain artifact source (no filesystem dependency).
- `hash_coreform_module(src: &str) -> Result<String, JsValue>`
  - Parse, canonicalize, and return the 32-byte module hash as 64-hex.
- `hash_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Run the self-hosted CoreForm toolchain inside the kernel to hash a module.
  - `step_limit=0` means “no limit”.
  - Toolchain bootstrap is not charged against `step_limit`; the limit applies to hashing the input module.
  - On `wasm32`, this API fails closed unless an explicit artifact is provided via `hash_coreform_module_selfhost_with_artifact`.
- `hash_coreform_module_selfhost_with_artifact(src: &str, artifact_src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Selfhost hash using a caller-supplied toolchain artifact source (no filesystem dependency).
- `eval_coreform_module(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Pure evaluation (no effects runner). `step_limit=0` means “no limit”.
  - If evaluation produces an effect program, returns an error telling the caller to use the host runner.
- `eval_coreform_module_with_gates(src: &str, step_limit: u32, stage1_pipeline: bool, stage1_gate: bool, stage2_gate: bool) -> Result<String, JsValue>`
  - Pure evaluation with optional Stage-1/Stage-2 obligation gates.
  - `stage1_gate` enforces `core/obligation::stage1-validation`.
  - `stage2_gate` enforces Stage-2 translation validation in fail-closed mode:
    unsupported modules fail, and supported modules must validate successfully.
  - Stage-2 validation uses Stage-1 transformed CoreForm input (same policy as package translation-validation), even when `stage1_pipeline=false`.
- `eval_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Run self-hosted parse+canonicalize in-kernel, then pure-evaluate the module.
  - `step_limit=0` means “no limit”.
  - Toolchain bootstrap is not charged against `step_limit`; the limit applies to evaluation of the input module.
  - On `wasm32`, this API fails closed unless an explicit artifact is provided via `eval_coreform_module_selfhost_with_artifact`.
- `eval_coreform_module_selfhost_with_gates(src: &str, step_limit: u32, stage1_pipeline: bool, stage1_gate: bool, stage2_gate: bool) -> Result<String, JsValue>`
  - Same as `eval_coreform_module_selfhost`, plus Stage-1/Stage-2 gating behavior.
- `eval_coreform_module_selfhost_with_artifact(src: &str, artifact_src: &str, step_limit: u32) -> Result<String, JsValue>`
  - Selfhost eval using a caller-supplied toolchain artifact source (no filesystem dependency).
- `eval_coreform_module_selfhost_with_artifact_and_gates(src: &str, artifact_src: &str, step_limit: u32, stage1_pipeline: bool, stage1_gate: bool, stage2_gate: bool) -> Result<String, JsValue>`
  - Artifact-backed selfhost eval with Stage-1/Stage-2 gating.
- `gfx_render_frame_graph_headless_hashes(frame_graph_src: &str, width: u32, height: u32) -> Result<JsValue, JsValue>`
  - Deterministic headless renderer hash API.
  - Accepts either a direct `:gfx/frame-graph` term or a map containing `:frame`/`:frame-graph`.
  - Returns `{width,height,pixel_h,png_h}` for cross-host parity gates.

For effectful programs, the WASM module exports a stateful runtime that supports step/resume:

- `new Runtime(step_limit: u32)`
  - `step_limit=0` means “no limit”.
- `Runtime.eval_module(src: &str) -> Result<JsValue, JsValue>`
  - Parses/canonicalizes/evaluates and returns the first step result (`done` or `effect`).
- `Runtime.eval_module_with_gates(src: &str, stage1_pipeline: bool, stage1_gate: bool, stage2_gate: bool) -> Result<JsValue, JsValue>`
  - Same as `Runtime.eval_module`, with optional Stage-1/Stage-2 gate enforcement before first step.
  - Stage-2 validation uses Stage-1 transformed CoreForm input when enabled.
- `Runtime.eval_module_selfhost(src: &str) -> Result<JsValue, JsValue>`
  - Uses self-hosted parse/canonicalize in-kernel, then returns the first step result (`done` or `effect`).
  - On `wasm32`, this API fails closed unless an explicit artifact is provided via `Runtime.eval_module_selfhost_with_artifact`.
- `Runtime.eval_module_selfhost_with_gates(src: &str, stage1_pipeline: bool, stage1_gate: bool, stage2_gate: bool) -> Result<JsValue, JsValue>`
  - Self-hosted frontend path with optional Stage-1/Stage-2 gate enforcement.
- `Runtime.eval_module_selfhost_with_artifact(src: &str, artifact_src: &str) -> Result<JsValue, JsValue>`
  - Self-hosted frontend path using caller-supplied artifact source.
- `Runtime.eval_module_selfhost_with_artifact_and_gates(src: &str, artifact_src: &str, stage1_pipeline: bool, stage1_gate: bool, stage2_gate: bool) -> Result<JsValue, JsValue>`
  - Artifact-backed self-hosted frontend path with optional Stage-1/Stage-2 gate enforcement.
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
wasm_js_path="$(bash scripts/wasm_bindgen_node.sh | tail -n 1)"
node scripts/wasm_node_smoke.mjs "$wasm_js_path"
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

`scripts/wasm_web_smoke.mjs` enforces cross-host parity against native examples for:
- effect-step hashes (`module_h`, `payload_h`, `cont_h`, `req_h`, `resp_h`, `final_value_h`)
- headless graphics hashes (`gfx_pixel_h`, `gfx_png_h`)
