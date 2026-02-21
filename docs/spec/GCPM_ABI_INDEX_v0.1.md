> Bundle Entry: `docs/spec/GCPM_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# GCPM ABI Index v0.1

Normative schema for `genesis gcpm abi --pkg <package.toml>`.

## Purpose

Provide a deterministic package introspection index for agent planning:

- contract op tables
- declared + inferred type/effect signatures
- required capabilities
- manifest obligations

The command is pure/local (no host effects) and emits `kind = "genesis/pkg-abi-v0.1"`.

## Envelope

In `--json` mode:

- `kind`: `genesis/pkg-abi-v0.1`
- `data.value`: canonical CoreForm map (schema below)

Non-JSON mode prints the same CoreForm map.

## CoreForm Value Schema

Top-level map keys:

- `:ok` (`bool`)
- `:schema` (`"genesis/pkg-abi-v0.1"`)
- `:package` (`map`)
- `:obligations` (`vector` of obligation symbols)
- `:required-caps` (`vector` of capability op symbols)
- `:module-count` (`int`)
- `:export-count` (`int`)
- `:typecheck-ok` (`bool`)
- `:typecheck-errors` (`vector` of strings)
- `:typecheck-warnings` (`vector` of strings)
- `:modules` (`vector` of per-module maps)
- `:index` (`map` from exported symbol -> export ABI entry)

`:package` keys:

- `:name` (`string`)
- `:version` (`string`)
- `:manifest` (`string`, path)
- `:root` (`string`, path)
- `:caps-policy` (`string|nil`)

Per-module entry keys:

- `:path` (`string`)
- `:hash` (`64-char hex`)
- `:intent` (`string|nil`)
- `:exports` (`vector` of symbols)
- `:declared-caps` (`vector` of symbols)
- `:required-caps` (`vector` of symbols)
- `:inferred-ops` (`vector` of symbols)
- `:unknown-ops` (`bool`)
- `:declared-types` (`map` symbol -> type term)
- `:typecheck-ok` (`bool`)
- `:typecheck-errors` (`vector` of strings)
- `:typecheck-warnings` (`vector` of strings)
- `:exports-abi` (`vector` of export ABI entries)

Export ABI entry keys:

- `:name` (export symbol)
- `:module` (module path string)
- `:declared-type` (type term or `nil`)
- `:inferred-type` (type term)
- `:effect-signature-ops` (vector of effect op symbols)
- `:effect-signature-open` (bool; true when effect row is open/unknown)
- `:required-caps` (vector of op symbols derived from effects)
- `:contract-ops` (vector of contract-op entries)

Contract-op entry keys:

- `:op` (operation symbol)
- `:type` (method type term)
- `:effect-signature-ops` (vector of effect op symbols)
- `:effect-signature-open` (bool)

## Determinism

- Output ordering is stable (map keys sorted canonically).
- Re-running with identical inputs and frontend configuration produces identical `data.value`.
