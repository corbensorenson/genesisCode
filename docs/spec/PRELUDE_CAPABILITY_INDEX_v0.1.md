# Prelude Capability Index v0.1

Machine-readable prelude capability wrapper index:

- `/Users/corbensorenson/Documents/genesisCode/docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json`

Generation + drift checks:

- regenerate: `bash scripts/update_capability_indices.sh`
- verify: `bash scripts/check_capability_indices.sh`

The index is extracted from `core/caps::perform` wrappers in `prelude/modules/*.gc`.

## Schema

Top-level keys:

- `kind = "genesis/prelude-capability-index-v0.1"`
- `generated_from_glob = "prelude/modules/*.gc"`
- `operations` (`string[]`, sorted unique)
- `families` (`map<string, string[]>`, sorted keys and values)

## Usage in Agent Workflows

- Choose capability policies from `operations` directly.
- Build family-level allowlists from `families` (e.g. allow all `gpu/compute` ops in compute pipelines).
- Cross-check prelude wrappers against host ABI index:
  - wrappers should exist in host ABI for effectful execution
  - host-only ops without wrappers should be treated as low-level/runtime-only.
