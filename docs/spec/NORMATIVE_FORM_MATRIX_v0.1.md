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

## Bindings And Pattern Boundary

v0.2 has binders, but no pattern language. A function parameter and a `let` binding
name must each be one symbol. Destructuring binders are malformed. Names within one
source `fn` parameter list or one `let` binding list must be unique; canonicalization
and typechecking reject the second occurrence with a deterministic diagnostic. This
applies even though multi-parameter functions later canonicalize to nested unary
functions and `let` right-hand sides evaluate sequentially.

Repeated top-level `def` names are different: they intentionally replace the current
module binding in module order as specified by `docs/spec/MODULE_SCOPE.md`.

There is no `match`, `case`, destructuring, wildcard, alternation, pattern guard, or
exhaustiveness rule in the v0.2 grammar. Consequently guards are unsupported and
exhaustiveness is not applicable. A pattern-like head not present in the nine-symbol
reserved inventory is ordinary application and normally fails as an unbound function;
no frontend or backend may recognize it as hidden syntax.

## Error And Location Boundary

User-controlled failure is explicit at every boundary. Parser and canonicalizer APIs
return Rust errors internally. A fatal evaluator failure such as malformed CoreForm,
an unbound symbol, exhaustion, or a non-callable value returns `KernelError` to its
caller. Recoverable language/library failures return a value sealed by the trusted
Prelude `ERROR` token under `genesis/error-v0.2`. A map with error-shaped fields or a
value sealed by a user-created token is not protocol `ERROR`.

Parser locations are zero-based offsets in the exact UTF-8 source bytes. EOF is the
source byte length; unexpected tokens and invalid integers use the token start; escape
errors use the backslash; unterminated strings/byte strings and containers use their
opening delimiter. Native Prelude and self-host parser adapters must preserve the same
code and offset in sealed error payloads.

The spanless CoreForm v0.2 term API cannot truthfully recover a source byte location
after parsing. Canonicalization therefore reports the zero-based module-form ordinal
and full cause. Typechecking reports module path plus deterministic diagnostic ordinal.
Runtime CoreForm errors have no source location. Missing locations remain absent rather
than being fabricated as byte zero or attributed to the whole input. A future
source-map profile must be versioned and must not alter CoreForm hashes.

## Tier Reconciliation

Canonicalization is the source-to-CoreForm authority. The reference evaluator defines
runtime semantics. The compiled AST evaluator is non-authoritative and must match the
reference on value/error, canonical value hash, steps, memory, seals, and coverage facts.
Type inference is gradual but must traverse every executable form according to
`docs/spec/TYPES.md`; effect inference skips quoted data and records no ambient effects.
Reference and compiled tiers receive canonical CoreForm, so rejected binders and hidden
pattern forms cannot become valid in one tier only. Fatal errors compare by kind and
message; sealed errors compare by trusted token role and canonical payload.

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

- This matrix does not add a pattern language or freeze future pattern design; it freezes
  the absence of patterns and guards in v0.2. It does not freeze primitives, Prelude APIs,
  types, effects, numbers, text, concurrency, modules, FFI, or packages beyond the cited
  v0.2 behavior.
- It does not claim independent Wasm execution or cross-host parity.
- It does not promote any self-host implementation or semantic decision to H1-H4.
- It changes no stage0 authority, fallback, release status, or downstream-product gate.
