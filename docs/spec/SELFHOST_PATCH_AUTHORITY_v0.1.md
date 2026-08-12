# Self-host Patch Authority v0.1

Status: normative H2 evidence contract.

This contract inventories the exact GenesisCode modules and `core/cli` bindings that
produce semantic patch identities, normalization, preconditions, refactor plans,
workspace diff and merge decisions, semantic transformations, conflicts, minimization,
and final apply reports. `policies/selfhost_patch_authority_v0.1.json` is the closed
machine profile and `scripts/lib/selfhost_patch_authority.py` is an independent
standard-library verifier that imports no GenesisCode implementation crate.

The verifier must reject source or binding inventory drift, mutable or escaping module
paths, restored Rust report production, missing fail-closed production routing,
compatibility-oracle custody drift, malformed schemas, stale profile identity, and
mutations of every authority-critical profile field. Its runtime mode executes the
same immutable fixture through the native and WASI production entrypoints in separate
workspaces, compares content-addressed output identities, and requires matching
malformed-input and resource-exhaustion behavior.

The profile sets `releaseGraphDisposition.h2Eligible` only while `gc_cli` and
`gc_wasi_cli` keep the parity driver optional, production binaries have no parity
feature requirement, and the dedicated parity binaries require the explicit
`parity-harness` feature. Parity-dependent integration tests are gated individually,
never as whole test targets, so ordinary production assertions remain in the default
suite. The verifier follows direct and helper-mediated parity calls, rejects ungated
parity tests and over-gated production tests, binds the reviewed 104-test/33-file
inventory, inspects both final production package feature graphs, and rejects either
`gc_cli_driver_parity` or `gc_patches/parity-oracle`.

`SD-PATCH` may reach H2 only when native/WASI and mutation controls pass and durable
evidence binds the exact artifact, profile, source, and production graph identities.
This contract claims no bootstrap fixpoint, independent second patch implementation,
release qualification, or downstream-product authorization.
