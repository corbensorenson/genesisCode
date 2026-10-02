# CoreForm Canonicalization + Hashing v0.2

This document is **normative** for GenesisCode v0.2.

GenesisCode v0.2 defines stable canonical printing and uses **BLAKE3** hashes over **canonical printed bytes**.

The complete accepted lexical and executable-form inventory, arities, scopes, runtime
semantics, and tier bindings are normative in
`docs/spec/NORMATIVE_FORM_MATRIX_v0.1.md`. This document governs representation,
canonicalization, printing, ordering, and hashing only.

## Terms

CoreForm is represented as immutable `Term` values:
- atoms: `nil`, booleans, integers, strings, bytes, symbols
- pairs (proper lists)
- vectors `[ ... ]`
- maps `{k v k2 v2 ...}` (key/value pairs)

### Canonical domain admission

A symbol name must parse as exactly one symbol with identical text. Empty names,
`nil`, `true`, `false`, names starting with an ASCII digit or `-` followed by an
ASCII digit, and names containing a lexer delimiter are not canonical symbols.
The delimiters are ASCII space, tab, LF, CR, `(`, `)`, `[`, `]`, `{`, `}`, `'`,
`"`, and `;`. Other Unicode scalars retain their v0.2 behavior; `.` is an ordinary
symbol and does not introduce a dotted-list form in this profile.

Admission examines every nested list element, vector element, map key and map
value. A pair's cdr must be nil or another admitted pair. Arbitrary runtime pairs
remain legitimate data for `pair/cons`, `pair/car` and `pair/cdr`; an improper pair
has no canonical v0.2 serialization and must be rejected at that boundary rather
than serialized as a placeholder. The proper list `(pair <improper>)` is ordinary
canonical data and must retain its historical bytes and identity.

The Rust reference library provides `Term::try_symbol`, `validate_symbol_name`,
`validate_canonical_term`, and checked term/module printer and hash APIs for host
boundary admission. Raw `Term` construction is not an admission proof. Infallible
reference printer/hash helpers require an already admitted canonical term;
diagnostic rendering of other runtime data must not become wire bytes or identity.
The GenesisCode printer and hash library reject inadmissible input as a sealed
`core/type-error`. Valid v0.2 printing, ordering, prefixes and hashes are unchanged.

Runtime hashing, including noncanonical runtime pairs, is governed separately by
`VALUE_EFFECT_HASH.md`. Canonical admission does not repair its existing runtime
alias counterexamples or authorize a silent hash/log format change.

## Canonicalization (Source -> Canonical CoreForm)

Canonicalization must:
- Reject improper lists as source forms.
- Normalize multi-arg `(fn (x y z) body)` into nested unary functions.
- Normalize multi-body forms into `(begin ...)`.
- Normalize n-ary application `(f a b c)` into nested binary application `(((f a) b) c)`.
- Normalize singleton list grouping `(f)` into `f`.
- Preserve data literals: vectors/maps are treated as data; canonicalization must not desugar application sugar inside them.
- Quote sugar `'x` parses as `(quote x)`.

## Canonical Printing

Canonical printing must be deterministic:
- 2-space indentation
- max width 100 columns
- maps must print keys in stable order (the total ordering of `Term`)
- applications must print in nested binary form (no `(f x y)` in canonical output)

## Term Ordering (Map Key Order)

Maps are ordered by a total order on `Term` with the following type tag precedence:
`Nil < Bool < Int < Str < Bytes < Symbol < Pair < Vector < Map`.

Within each type:
- `Bool`, `Int`, `Str`, `Bytes`, `Symbol` use their natural order (lexicographic for strings/symbols, bytewise for bytes).
- `Pair` compares lexicographically by car then cdr.
- `Vector` compares lexicographically by elements then by length.
- `Map` compares lexicographically by key then value pairs (in their own canonical key order) then by length.

## Hashing

All hashes are BLAKE3, output as 32 bytes (or hex encoding for manifests/artifacts).

### Term Hash

The hash of a single term is:
- `BLAKE3( "GCv0.2\\0" || canonical_print(term) )`

### Module Hash

The hash of a module (vector of top-level forms) is:
- `BLAKE3( "GCv0.2\\0module\\0" || canonical_print_module(forms) )`

## Production Frontend Authority

For the R4.2.a production profile, the accepted GenesisCode artifact binding
`core/cli::frontend-module` produces one closed result containing the canonical forms,
canonical module bytes, module hash, UTF-8 source span, and exact frontend profile. Native
and WASI hosts validate and transport that result; they do not independently recompute or
replace any of those semantic facts. A missing binding, sealed error, malformed result,
profile mismatch, or span mismatch fails closed rather than selecting the Rust reference
frontend.

`docs/spec/SELFHOST_FRONTEND_AUTHORITY_v0.1.md` defines the transition boundary and
`policies/selfhost_frontend_authority_v0.1.json` freezes independently checked valid and
malformed vectors. The Rust parser, canonicalizer, printer, and hasher remain available only
to explicitly named parity harnesses and Stage0 artifact admission where the Stage0 trust
contract permits them; their existence is not a production semantic fallback.

## Stability Requirements

- Any change to canonical printing changes hashes and therefore invalidates pinned manifests and evidence; such changes must be treated as a versioned surface change.
- If canonical printing changes intentionally, bump the prefix tag (e.g. `GCv0.3\\0`) and keep v0.2 behavior available if compatibility is required.
- A production frontend profile change must update its schema, frozen vectors, content
  identity, independent verifier, and every affected artifact or evidence identity in one
  reviewed transition.
