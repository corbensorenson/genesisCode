# Type Terms and Typechecking v0.2

This document is normative for the v0.2 `core/obligation::typecheck` surface.

The v0.2 typechecker is **gradual**: `?` accepts anything. When types are concrete, the checker
attempts to infer and verify conformance for a conservative subset of CoreForm.

## Where Types Live

Each module must define `::meta` as a quoted map datum that includes:

- `:exports` vector of exported symbols
- `:types` map from exported symbol -> type term
- `:caps` vector of declared effect-operation symbols; every statically inferred operation
  must be listed

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

- `tail = nil` closes a row.
- `tail = ?` is an anonymous gradual row and may contain additional or unknown members.
- In an effect row, any other symbol names an implicit rank-1 row variable scoped to that
  one exported type declaration. A named effect-row variable must occur within the
  outermost function parameter type before it can constrain a result or function effect.
  A standalone program/contract or a nested returned function cannot introduce one in v0.2.
- Record and contract row-tail symbols remain open shape markers; they are not effect-row
  variables and are not substituted by function application.
- For `Eff`, op symbols are the fully-qualified operation symbols (e.g. `sys/time::now`).
- Repeating an operation in one `Eff` vector is an error rather than an alternate spelling
  of the canonical set.

## Compatibility Rules (High Level)

Given an inferred type `I` and a declared type `D`:

- If `D` is `?`, it is always accepted.
- If `I` is `?` and `D` is concrete, typecheck fails (cannot establish conformance).
- Records and contracts use width compatibility:
  - every declared field/method must exist in the inferred row and be compatible
  - inferred rows may contain additional entries
- Optional strict shape mode:
  - if `::meta :strict-shapes true`, declared closed rows (`tail = nil`) become exact:
    inferred rows must also be closed and must not include undeclared fields/methods
- Effect rows:
  - a closed declared row accepts only a closed inferred row whose operations are a
    subset of the declared operations
  - an anonymous `?` tail admits additional and unknown operations
  - a named tail captures the inferred remainder after the declared fixed operations are
    removed; every occurrence of the same name in one compatibility check must capture the
    same remainder
  - function applications allocate fresh named-row bindings, apply them to the return type,
    and merge a captured remainder with any fixed result operations
  - an unknown argument binds every effect-row variable reachable from its expected
    parameter type to an unknown row; a symbolic variable may not escape as evidence of a
    known effect set

Named effect rows provide rank-1 effect polymorphism over the explicit function type. For
example, `(Fn (Prog Int (Eff [] e)) (Prog Int (Eff [] e)) (Eff [] nil))` preserves the
argument program's effects. Reusing `e` for two incompatible parameter components is a type
mismatch. `(Fn Int (Prog Int (Eff [] e)) (Eff [] nil))` is invalid because the outermost
parameter does not bind `e`. Higher-rank introduction by a returned function and implicit
per-method contract polymorphism are unsupported in v0.2 and fail declaration validation.

When `:strict-effects true` is active, every declared effect row must be closed or use a
named variable bound by the outermost function parameter. Anonymous `?` tails are rejected.
A sound parameter-bound variable is permitted because application instantiates it from the
argument; it is not an ambient capability wildcard.

## Package Boundaries

Typechecking is package-wide even though diagnostics remain attributed to modules:

- every uniquely owned exported declaration seeds one package type environment before any
  module body is checked, so results do not depend on module input order
- two modules claiming the same exported symbol fail with a deterministic ownership error
- concrete imported function and program signatures propagate their effects through callers
  and into exported-effect/capability checks
- an imported export declared as `?` remains gradual in ordinary mode, but calling it in a
  strict-effects module fails because its effect signature is unknown
- an invalid exported effect-row declaration is not admitted into the shared environment

Package metadata does not grant capability. Each module's `:caps` must cover the typed
transitive effects reachable from its definitions and exports as well as effects found by
direct syntax inference.

## Inference Coverage (v0.2)

The typechecker infers types most precisely for:

- literals (`Int`, `Bool`, `Nil`, `Str`, `Bytes`)
- `fn`, `if`, `begin`, `let`
- `(prim ...)` for core integer primitives and row-aware map operations (`map/get`, `map/put`, `map/merge`)
- `core/msg::*`
- `core/contract::*` (including contract-row extraction from override map literals)
- `core/effect::*` including `core/effect::bind` sequencing (returns a `Prog` type with merged effect rows)
- task wrapper/op-table inference for `core/task::*` helper families:
  - spawn wrappers (`spawn-program`, `spawn-eval*`) map to base `core/task::spawn`
  - pure task DSL constructors (`program*`, `step/*`, `reduce-seq`) do not force `unknown`
- typed fallback function application for declared/known `Fn` values, including curried
  application chains and per-call effect-row instantiation
- package-exported function/program signatures across module boundaries

Applications with unknown/non-function heads are treated conservatively as `?` (but still walked for effect inference).

Unknown effect operations emit a warning and are rejected when `:caps` is empty or
`:strict-effects true`. Strict mode requires literal operation symbols and either closed
declared effect rows or named rows bound by the outermost function parameter. Task-effect
declarations enable strict mode automatically.
