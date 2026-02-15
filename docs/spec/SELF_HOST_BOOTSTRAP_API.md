# Self-Host Bootstrap API (CoreForm) v0.2

This document defines the minimal **pure** CoreForm bootstrap API exposed to GenesisCode programs
to enable wasm-first tooling and the self-host migration plan.

These functions are implemented as host intrinsics inside the prelude (`gc_prelude`) and therefore:
- are deterministic and effect-free
- are available in native, WASI (`genesis_wasi.wasm`), and `wasm32-unknown-unknown` builds

## Rationale

The language cannot self-host parsing/printing/hashing without some initial capabilities. This API makes it
possible to write tool logic in GenesisCode while keeping the kernel pure and the overall TCB small.

The long-term goal is to replace these intrinsics with GenesisCode implementations guarded by
translation validation obligations (see `docs/spec/SELF_HOST_BOUNDARY.md`).

## Normative Functions

All functions are total and must not panic. Errors are returned as kernel errors (unsealed) and appear as
evaluation failures unless the caller wraps them.

### `core/coreform::parse-term`

`(core/coreform::parse-term <src>) -> <term>`

Parses a single CoreForm term from the given source and returns it as an immutable datum.

`<src>` is either a UTF-8 string (`"..."`) or UTF-8 bytes (`b"..."`).

### `core/coreform::parse-module`

`(core/coreform::parse-module <src>) -> <vector-of-terms>`

Parses a CoreForm module from the given source and returns a datum vector of top-level forms.

`<src>` is either a UTF-8 string (`"..."`) or UTF-8 bytes (`b"..."`).

### `core/coreform::canonicalize-module`

`(core/coreform::canonicalize-module <vector-of-terms>) -> <vector-of-terms>`

Canonicalizes a parsed module by applying the v0.2 canonicalization rules.

### `core/coreform::print-term`

`(core/coreform::print-term <term>) -> <string>`

Canonical prints a CoreForm term (no trailing newline).

### `core/coreform::print-module`

`(core/coreform::print-module <vector-of-terms>) -> <string>`

Canonical prints a CoreForm module (includes trailing newline per module printing rules).

### `core/coreform::fmt-module`

`(core/coreform::fmt-module <src>) -> <string>`

Equivalent to: parse-module, canonicalize-module, print-module.

`<src>` is either a UTF-8 string (`"..."`) or UTF-8 bytes (`b"..."`).

### `core/coreform::hash-term`

`(core/coreform::hash-term <term>) -> <string>`

Computes the v0.2 term hash as a 64-hex string:
`BLAKE3("GCv0.2\\0" || canonical_print(term))`.

### `core/coreform::hash-module`

`(core/coreform::hash-module <vector-of-terms>) -> <string>`

Computes the v0.2 module hash as a 64-hex string:
`BLAKE3("GCv0.2\\0module\\0" || canonical_print_module(forms))`.

### `core/coreform::hash-module-src`

`(core/coreform::hash-module-src <src>) -> <string>`

Equivalent to: parse-module, canonicalize-module, hash-module.

`<src>` is either a UTF-8 string (`"..."`) or UTF-8 bytes (`b"..."`).
