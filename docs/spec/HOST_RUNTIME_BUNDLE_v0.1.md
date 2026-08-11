# Host Runtime Bundle v0.1

Canonical bundle for host ABI, capability dispatch, and runtime boundary rules.

## Included Specs

- `docs/spec/HOST_ABI.md`
- `docs/spec/BROWSER_HOST_RUNTIME_v0.1.md`
- `docs/spec/XR_HOST_RUNTIME_v0.1.md`
- `docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md`
- `docs/spec/HOST_BRIDGE_PROTOCOL.md`
- `docs/spec/CAPS_TOML.md`
- `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
- `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
- `docs/spec/SELF_HOST_BOUNDARY.md`
- `docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json`
- `docs/spec/STAGE0_TRUST_CONTRACT_v0.1.schema.json`
- `docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.md`
- `docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json`
- `docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.schema.json`
- `docs/spec/WASM.md`
- `docs/spec/WASI.md`

## Agent Guidance

- Use this bundle as the runtime/effects entrypoint.
- Resolve op-level behavior in `HOST_ABI.md` only after reading bundle summary.
- Machine-readable index contracts (`HOST_ABI_INDEX_v0.1.json`,
  `HOST_ABI_SCHEMA_INDEX_v0.1.json`) are documented in `HOST_ABI.md`.
- Runtime limit semantics are consolidated into `docs/spec/CAPS_TOML.md`.
- Stage0 trust-domain authority is machine-readable in
  `docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json`; only S0-K is TCB-A.
- Self-host levels are cumulative per semantic decision under
  `docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json`; `N/A` is a reviewed
  non-promoting disposition, not a closure level.
