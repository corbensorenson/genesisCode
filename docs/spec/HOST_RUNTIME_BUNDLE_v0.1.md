# Host Runtime Bundle v0.1

Canonical bundle for host ABI, capability dispatch, and runtime boundary rules.

## Included Specs

- `docs/spec/HOST_ABI.md`
- `docs/spec/HOST_ABI_INDEX_v0.1.md`
- `docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.md`
- `docs/spec/HOST_BRIDGE_PROTOCOL.md`
- `docs/spec/CAPS_TOML.md`
- `docs/spec/LIMITS.md`
- `docs/spec/SELF_HOST_BOUNDARY.md`
- `docs/spec/WASM.md`
- `docs/spec/WASI.md`

## Agent Guidance

- Use this bundle as the runtime/effects entrypoint.
- Resolve op-level behavior in `HOST_ABI.md` only after reading bundle summary.
