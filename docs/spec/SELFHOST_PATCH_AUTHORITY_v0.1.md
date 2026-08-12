# Self-host Patch Authority v0.1

Status: normative audit contract; not yet H2-promoting.

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

The current profile deliberately sets `releaseGraphDisposition.h2Eligible` to false.
The outer `gc_cli` and `gc_wasi_cli` packages unconditionally depend on the parity
driver, so Cargo feature unification compiles `gc_patches/parity-oracle` while building
those packages even though the production runtime profile rejects Rust frontend
selection. The generated R4.2.c owner set does not authorize those two manifests.
This verifier preserves that blocker rather than laundering route denial into H2.

R4.2.c and `SD-PATCH` may reach H2 only after an authorized transaction isolates the
parity binaries from both production package feature graphs, the verifier changes the
disposition to eligible, native/WASI and mutation controls pass, and independently
custodied durable evidence reviews the exact artifact and source identities. This
contract claims no bootstrap fixpoint, independent second patch implementation,
release qualification, or downstream-product authorization.
