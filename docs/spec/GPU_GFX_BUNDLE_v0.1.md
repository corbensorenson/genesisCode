# GPU/GFX Bundle v0.1

Canonical bundle for graphics and compute capability surfaces.

## Included Specs

- `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
- `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
- `docs/spec/GFX_CAPS.md`
- `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`

## Agent Guidance

- Start here for any GPU or graphics task.
- Resolve compute-first policy/profile requirements in `GPU_COMPUTE_BUNDLE_v0.1.md`.
- Resolve rendering/runtime-first requirements in `GFX_RUNTIME_BUNDLE_v0.1.md`.
- Prefer canonical `gpu/compute::*` ops for compute workloads and use gfx wrappers only when needed.
- Architecture/obligation/device-bridge specifics are consolidated into
  `docs/spec/GFX_CAPS.md` and `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`.

## Demo Workloads

Canonical end-to-end `.gc` graphics demo entrypoints:

- `/Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/ui_app.gc`
- `/Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/scene3d.gc`
- `/Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/hybrid_web.gc`

Run demos via CLI:

```sh
cargo run -p gc_cli -- eval examples/gfx_demos/ui_app.gc
cargo run -p gc_cli -- eval examples/gfx_demos/scene3d.gc
cargo run -p gc_cli -- eval examples/gfx_demos/hybrid_web.gc
```

Validation coverage:

- Pure evaluation + deterministic shape checks:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/tests/gfx_demos_examples.rs`
- CLI execution smoke checks:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_gfx_demos.rs`
