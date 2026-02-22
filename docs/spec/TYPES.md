# Type Terms and Typechecking v0.2

This document is normative for the v0.2 `core/obligation::typecheck` surface.

The v0.2 typechecker is **gradual**: `?` accepts anything. When types are concrete, the checker
attempts to infer and verify conformance for a conservative subset of CoreForm.

## Where Types Live

Each module must define `::meta` as a quoted map datum that includes:

- `:exports` vector of exported symbols
- `:types` map from exported symbol -> type term

If an exported symbol is missing from `:types`, typecheck fails.

## Type Term Grammar

Type terms are CoreForm data terms. The supported constructors are:

- `?` top / unknown type
- `Int`, `Bool`, `Nil`, `Str`, `Bytes`, `Symbol`
- `(Msg PayloadType)`
- `(Fn ParamType ReturnType (Eff [op1 op2 ...] tail))`
- `(Prog ReturnType (Eff [op1 op2 ...] tail))`
- `(Rec [[k Ty] ...] tail)`
- `(Contract [[op Ty] ...] tail)`

Notes:

- `tail` is `nil` for a closed row, or `?` / any symbol for an open row.
- For `Eff`, op symbols are the fully-qualified operation symbols (e.g. `sys/time::now`).

## Compatibility Rules (High Level)

Given an inferred type `I` and a declared type `D`:

- If `D` is `?`, it is always accepted.
- If `I` is `?` and `D` is concrete, typecheck fails (cannot establish conformance).
- Records and contracts use width compatibility:
  - every declared field/method must exist in the inferred row and be compatible
  - inferred rows may contain additional entries
- Effect rows:
  - closed declared effect rows require inferred ops to be a subset and require `unknown = false`
  - open declared effect rows are permissive (they admit additional and/or unknown ops)

## Inference Coverage (v0.2)

The typechecker infers types most precisely for:

- literals (`Int`, `Bool`, `Nil`, `Str`, `Bytes`)
- `fn`, `if`, `begin`, `let`
- `(prim ...)` for core integer primitives and row-aware map operations (`map/get`, `map/put`, `map/merge`)
- `core/msg::*`
- `core/contract::*` (including contract-row extraction from override map literals)
- `core/effect::*` including `core/effect::bind` sequencing (returns a `Prog` type with merged effect rows)
- typed fallback function application for declared/known `Fn` values (including curried application chains)

Applications with unknown/non-function heads are treated conservatively as `?` (but still walked for effect inference).
