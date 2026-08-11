# Normative Form Matrix v0.1

Status: normative for the `genesis/language-profile/v0.2` source and CoreForm surface.

## Authority

The closed machine matrix is `docs/spec/NORMATIVE_FORM_MATRIX_v0.1.json`, validated
against `docs/spec/NORMATIVE_FORM_MATRIX_v0.1.schema.json`. This document and matrix
supersede the conceptual syntax sketches in `docs/PAPER_v0.2.md` and
`docs/TECH_HANDOFF.md` when those historical inputs disagree with production v0.2.

An accepted executable form is exactly one row in the matrix. Adding a reserved head,
literal class, collection expression, grouping rule, module form, or application rule
without adding a reviewed row is invalid. A library binding, Prelude helper, optimizer
pattern, backend shortcut, or self-host wrapper cannot create syntax implicitly.

## Lexical Terms

The parser accepts whitespace and semicolon line comments plus these term classes:

- `nil`, `true`, and `false`
- unbounded base-10 integers with an optional leading minus
- UTF-8 strings with `\\`, `\"`, `\n`, `\r`, `\t`, `\xNN`, and `\uNNNN` escapes
- byte strings `b"..."` with the same named escapes and byte-oriented `\xNN`
- symbols delimited by whitespace, brackets, braces, parentheses, quote, string quote,
  or comment start
- proper lists `( ... )`, vectors `[ ... ]`, and even-entry maps `{k v ...}`
- quote sugar `'datum`, which parses as `(quote datum)`

Unexpected closers, unterminated strings/escapes/containers, invalid hex or Unicode,
odd map entries, and trailing material in single-term parsing are explicit parse errors.
Source syntax cannot construct an improper list.

## Executable Forms

Atoms evaluate as immutable data except a symbol, which performs lexical/module/external
lookup. Vector elements and map keys are data. Map values are expressions evaluated in
stable key order. The empty list evaluates to `nil`; a singleton list is redundant
grouping. Every other non-special proper list is left-associated call-by-value
application.

The only reserved heads are `quote`, `def`, `fn`, `if`, `begin`, `let`, `prim`, `seal`,
and `unseal`:

- `(quote datum)` returns datum without evaluating it.
- `(def name expression)` is valid only at module top level and binds in module order.
- `(fn (parameter...) body...)` requires one or more symbol parameters; source n-ary and
  multi-body functions canonicalize to nested unary functions and `begin`.
- `(if condition consequent alternate)` evaluates one branch; only `nil` and `false` are
  falsey.
- `(begin expression...)` requires at least one expression and returns the final value.
- `(let ((name expression)...) body...)` evaluates bindings sequentially, so a later
  binding sees earlier bindings; bodies canonicalize through `begin`.
- `(prim operation argument...)` requires a symbol operation and evaluates arguments
  left to right before the total primitive boundary returns a value or explicit error.
- `(seal)` creates a fresh unforgeable token; `(seal value token)` creates a sealed value.
- `(unseal wrapped token)` returns the payload only for the identical token and otherwise
  returns `nil`.

Malformed reserved forms fail as explicit canonicalization or kernel `BadForm` errors;
they never fall through to ordinary application.

## Tier Reconciliation

Canonicalization is the source-to-CoreForm authority. The reference evaluator defines
runtime semantics. The compiled AST evaluator is non-authoritative and must match the
reference on value/error, canonical value hash, steps, memory, seals, and coverage facts.
Type inference is gradual but must traverse every executable form according to
`docs/spec/TYPES.md`; effect inference skips quoted data and records no ambient effects.

The current `gc_wasm` runtime parses and canonicalizes through `gc_coreform` and executes
through `gc_kernel::eval_module`. It therefore inherits the reference evaluator; it is
not an independently implemented form tier. Stage2 CoreForm-to-Wasm validation remains a
separate, fail-closed and incomplete translation gate under `docs/spec/WASM.md`.

Prelude exports are ordinary values and functions evaluated through these forms. Prelude
may add APIs and DSL constructors but no hidden parser or evaluator form. The existing
foundation conformance target checks both required Prelude behavior and this matrix.

## Change Rule

Any form change must update this prose, the closed matrix, canonicalization, every
affected evaluator/inference route, positive and malformed controls, profile/version
policy, and migration guidance in one reviewed transaction. An undocumented source head
must remain ordinary application; an undocumented backend-only head is a gate failure.

## Nonclaims

- This matrix does not freeze primitives, Prelude APIs, types, effects, numbers, text,
  patterns, concurrency, modules, FFI, or packages beyond the cited v0.2 behavior.
- It does not claim independent Wasm execution or cross-host parity.
- It does not promote any self-host implementation or semantic decision to H1-H4.
- It changes no stage0 authority, fallback, release status, or downstream-product gate.
