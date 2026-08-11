# Optimizer (gc_opt) v0.2

This document is normative for the behavior and evidence emitted by the v0.2 optimizer.

## Scope

The optimizer rewrites CoreForm terms in a **conservative pure subset**. It must never change
observable behavior of programs that run under the v0.2 kernel.

The optimizer operates over parsed+canonicalized CoreForm `Term` modules (`.gc`), and produces a
new CoreForm module that is also canonicalizable.

## Purity Boundary (Must Not Cross)

The optimizer must treat the following as **opaque** boundaries and must not rewrite through them:

- `(seal ...)` / `(unseal ...)`
- any application whose head is a symbol with prefix:
  - `core/effect::`
  - `core/contract::`

Additionally, the optimizer must not rewrite inside `(quote datum)`.

`CONTRACT_COMPOSITION_PROFILE_v0.1.md` may report a public interface as optimization-eligible only
when it is concrete, shape/effect closed, pure, refinement-free, contract-free, and monomorphic.
That report is a necessary admission fact for transformations relying on the interface, never a
rewrite authorization or equivalence proof. All rules in this document and exact-artifact
translation validation still apply; runtime dispatch shapes and static interface identities cannot
substitute for one another.

## Rewritten Subset

The v0.2 optimizer uses `egg` (e-graphs) to optimize only expressions that can be represented in
the `PureLang` grammar:

- integer literals
- boolean literals
- variables (symbols)
- `(if c t e)` where `c`, `t`, and `e` are each representable in `PureLang`
- `(prim int/add a b)` / `int/sub` / `int/mul` / `int/eq?` / `int/lt?` where `a` and `b` are each
  representable in `PureLang`

Everything else is optimized by structural recursion plus local constant-folding for `prim int/*`
and literal `if` conditions.

All integer folding uses arbitrary-precision values from the normative numeric profile; narrowing,
wrapping, saturation, or host-word-dependent folding is invalid. `int/div` and `int/mod` remain
outside the current rewrite grammar and therefore retain reference-kernel Euclidean semantics.

## Determinism Requirements

Optimization results must be deterministic across platforms and runs.

### E-Graph Limits

The optimizer must cap e-graph search to ensure termination:

- `iter_limit = 8`
- `node_limit = 50_000`

### Deterministic Extraction

When multiple equivalent forms exist in the e-graph, the extracted "best" expression must be chosen
deterministically.

The v0.2 extractor uses a cost function that orders candidates by:

1. smaller node count
2. a deterministic structural `repr` string as a tiebreaker

### Rewrite Statistics

Rewrite stats emitted by tools must be stable:

- rewrite names are strings (not `egg::Symbol`)
- stats maps are emitted in a stable order (Rust `BTreeMap`)

## Rewrite Set (v0.2)

The v0.2 rewrite set is intentionally small and conservative:

- Commutativity:
  - `(+ a b) => (+ b a)`
  - `(* a b) => (* b a)`
- Identities:
  - `(+ 0 a) => a`, `(+ a 0) => a`
  - `(* 1 a) => a`, `(* a 1) => a`
  - `(* 0 a) => 0`, `(* a 0) => 0`
  - `(- a 0) => a`
  - `(- a a) => 0`
- Constant conditionals:
  - `(if true t e) => t`
  - `(if false t e) => e`
- Trivial comparisons:
  - `(== a a) => true`
  - `(< a a) => false`

Constant folding of arithmetic and comparisons is implemented via `egg::Analysis` and is applied
only when both operands are literal integers.

## CLI Evidence

`genesis optimize --json` must emit a `genesis/optimize-v0.2` JSON object that includes:

- `changed: bool`
- `original_hash: 64-hex`
- `optimized_hash: 64-hex`
- optimizer stats:
  - `egg_runs`, `egg_iterations`, `egg_eclasses`, `egg_enodes`
  - `egg_rewrites_applied: { <rewrite_name>: <count>, ... }`

## Stage 2 Artifact Boundary

Stage 2 is a non-authoritative Wasm candidate tier. It may internally lower an accepted subset to
`i64`, but `Int` remains arbitrary precision. Every `emit_wasm` command must first produce a
successful `core/obligation::translation-validation` report for the exact transformed module.
Unsupported lowering, out-of-range values, result drift, or value-hash drift rejects emission even
when the caller did not separately request `stage2_gate`. Direct compiler API output is a candidate
only and has no execution, publication, or release authority without that report.

See `docs/spec/NUMERIC_PROFILE_v0.1.md` for the complete numeric contract.
