# Selfhost Symbol Ownership Index v0.1

This document defines the machine-readable ownership index emitted by:

```bash
genesis --json agent-index
```

under `data.selfhost_symbol_index`.

## Purpose

Provide deterministic symbol-to-module ownership metadata for selfhost toolchain planning so
agents can target edits without scanning the entire selfhost source set.

## Schema

- `schema`: `"genesis/selfhost-symbol-ownership-index-v0.1"`
- `path`: `"selfhost/toolchain_manifest.gc"`
- `loaded`: bool
- `module_count`: int
- `symbol_count`: int
- `required_symbol_count`: int
- `unresolved_required_symbols`: vector of symbol strings
- `duplicate_symbol_owners`: vector of:
  - `symbol`: string
  - `module_paths`: vector<string>
- `symbols`: vector of:
  - `symbol`: string
  - `module_path`: string
  - `module_intent`: string or `null`
  - `required`: bool

## Generation Rules

- Module set is sourced from `selfhost/toolchain_manifest.gc` `:module-paths`.
- Required symbol set is sourced from `selfhost/toolchain_manifest.gc` `:required-symbols`.
- Symbol ownership is derived from top-level `(def <symbol> ...)` forms in each module.
- `::meta` is excluded from ownership entries.
- Module intent is read from module `::meta` `:intent` when present.
- Ownership is deterministic:
  - module iteration follows manifest order
  - output `symbols` ordering is lexical by `symbol`
  - `duplicate_symbol_owners` ordering is lexical by `symbol`

## Failure Semantics

When manifest/module loading fails:

- `loaded = false`
- `error` contains a stable human-readable failure string
- counts are zero
- vectors are empty
- affected paths are surfaced via `data.missing_sources` in `agent-index`.
