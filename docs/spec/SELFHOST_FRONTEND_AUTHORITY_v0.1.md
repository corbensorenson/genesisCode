# Self-host Frontend Authority Profile v0.1

Status: normative for the R4.2.a authority transition.

## Scope

`core/cli::frontend-module` is the single production GenesisCode entrypoint for
source decoding, CoreForm canonicalization, canonical module printing, normative
module hashing, and source-byte span identity. It returns the closed
`genesis/frontend-module-v0.1` map declared by
`policies/selfhost_frontend_authority_v0.1.json`.

The native and WASI hosts may:

- invoke the accepted GenesisCode artifact under declared resource bounds;
- reject a missing binding, sealed error, malformed result, wrong profile, or
  out-of-range field;
- decode the returned forms for kernel execution;
- transport the returned canonical source, module hash, and byte offset;
- derive line and column presentation from a GenesisCode-produced UTF-8 byte offset.

They may not recompute, replace, or silently recover source semantics, canonical
bytes, module identity, or parser offsets. Missing or failed production authority
fails closed. Rust frontend behavior is confined to separately named parity
harness entrypoints and is not a production rollback path.

## Independent Verification

`scripts/lib/selfhost_frontend_authority.py` is intentionally independent of the
GenesisCode Rust crates. It checks frozen source-to-canonical vectors, recomputes
the normative BLAKE3 module identity, verifies exact UTF-8 spans and malformed
diagnostics across native and WASI production entrypoints, checks route custody,
and runs mutation controls that restore fallback or tamper with returned facts.

The verifier uses an independently installed BLAKE3 implementation only as a
cryptographic primitive. It does not import the parser, canonicalizer, printer,
Prelude, runtime driver, or artifact loader under test.

## Nonclaims

- This profile and its local verifier do not alone promote any ownership-ledger
  row to H2.
- It does not prove bootstrap fixpoint, cross-host reproducibility, type/effect
  authority, compiler authority, or an independent full parser implementation.
- Frozen vectors are a minimal independent identity verifier, not exhaustive
  language conformance; broader conformance remains R4.5 and R7.2.f work.
