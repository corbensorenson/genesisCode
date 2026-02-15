# Module Scope + `def` Semantics v0.2

This document is **normative** for GenesisCode v0.2 evaluation.

## Goals

- Make top-level modules usable without special recursion forms.
- Keep the kernel deterministic and pure (no effects).
- Keep lexical scoping for local bindings (`let`, `fn`).

## Top-Level Forms

A module is a vector/list of CoreForm terms evaluated left-to-right.

### `(def <sym> <expr>)`

`def` is only recognized at the **top level** by the module evaluator.

Semantics:

1. Evaluate `<expr>` in the current module environment.
2. Bind the result to `<sym>` in the **current module scope** (overwriting any prior binding for the same name).
3. The value of a `def` top-form is `nil`.

### Recursive Module Scope

All `def` bindings in a module share a single module scope frame. That frame is updated as `def`s are evaluated.

Consequence:

- Closures created earlier in the module can call functions defined later in the module.
- Mutual recursion across top-level definitions works (e.g., `even?` and `odd?` defined in either order).

This rule is required to support self-hosted tooling modules (printer, canonicalizer, etc.) without adding a dedicated
`letrec` form in v0.2.

## Lexical Scope

Local bindings (`let`, function parameters) are lexical and create nested scope frames.

Name lookup is:

1. Innermost lexical frame outward through parents.
2. Module scope frame (top-level `def` bindings).

## Hashing Note

Recursive module scope can create cyclic value graphs (a binding closes over the module scope that binds it). Value
hashing remains total and deterministic by applying the cycle handling rule defined in `docs/spec/VALUE_EFFECT_HASH.md`.

