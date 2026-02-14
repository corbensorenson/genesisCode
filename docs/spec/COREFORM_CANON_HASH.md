# CoreForm Canonicalization + Hashing v0.2

This document is **normative** for GenesisCode v0.2.

GenesisCode v0.2 defines stable canonical printing and uses **BLAKE3** hashes over **canonical printed bytes**.

## Terms

CoreForm is represented as immutable `Term` values:
- atoms: `nil`, booleans, integers, strings, bytes, symbols
- pairs (proper lists)
- vectors `[ ... ]`
- maps `{k v k2 v2 ...}` (key/value pairs)

## Canonicalization (Source -> Canonical CoreForm)

Canonicalization must:
- Reject improper lists as source forms.
- Normalize multi-arg `(fn (x y z) body)` into nested unary functions.
- Normalize multi-body forms into `(begin ...)`.
- Normalize n-ary application `(f a b c)` into nested binary application `(((f a) b) c)`.
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

## Stability Requirements

- Any change to canonical printing changes hashes and therefore invalidates pinned manifests and evidence; such changes must be treated as a versioned surface change.
- If canonical printing changes intentionally, bump the prefix tag (e.g. `GCv0.3\\0`) and keep v0.2 behavior available if compatibility is required.

