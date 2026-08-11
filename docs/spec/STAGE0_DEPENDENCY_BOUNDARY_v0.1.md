# Stage0 Dependency Boundary v0.1

## Purpose

This contract turns the stage0 trust model into an enforceable source and dependency
boundary. It governs the production closures of `gc_coreform`, `gc_kernel`, and
`gc_prelude`; a crate name, repository location, `.gc` route, or successful build does
not make code part of stage0 or grant semantic authority.

The machine authority is
`docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.json`, validated against the closed schema
`docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.schema.json`. It binds the exact
`docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json` identity.

## Production Graph

The permitted workspace graph is exact and acyclic:

1. `gc_coreform` has no workspace dependency.
2. `gc_kernel` may depend on `gc_coreform` only.
3. `gc_prelude` may depend on `gc_coreform` and `gc_kernel` only.

Normal, target-specific, and build dependencies all participate in the production
closure. Dev dependencies are separately enumerated for tests and differential oracles;
they receive no production authority and the resolved non-dev graph must prove they are
unreachable from a production stage0 package.

Every stage0 manifest, direct external dependency, and feature definition is bound. A
new optional dependency, feature activation, target dependency, renamed local
dependency, build dependency, or manifest change fails closed until this contract is
reviewed. For each stage0 root, a root-independent digest binds every resolved non-dev
package identity, enabled feature, target-qualified edge, and package count. The closure
also rejects packages associated with CLI parsing, package/registry behavior, optimizer
authority, ambient effects, networking, processes, clocks, randomness, databases, UI,
GPU access, dynamic loading, and asynchronous runtimes.

## Source Closure

Every Rust source below a declared stage0 source root and every declared production build
script must be a regular non-symlink file. Production sources may not name forbidden
workspace crates. `#[path]` attributes must resolve inside the declaring source root.
Every `include!`, `include_str!`, or `include_bytes!` line is denied unless it exactly
matches an enumerated escape. The only current escapes are the build-produced Prelude
string and the two reviewed embedded selfhost artifacts.

`gc_prelude/build.rs` is a separately declared S0-P build adapter. Its dependency set is
closed and its filesystem/environment access does not become runtime language semantics.

## Enforcement

- Validator: `scripts/lib/stage0_dependency_boundary.py`
- Gate: `scripts/check_selfhost_boundary.sh --strict`
- Inputs: workspace and stage0 manifests, `Cargo.lock`, all declared production source
  roots, this prose/schema/machine contract, and the bound stage0 trust contract.

The validator parses manifests independently, obtains `cargo metadata --locked
--offline`, removes dev-only edges, canonicalizes repository-local package IDs, and
compares each complete resolved production graph with the exact policy. Its self-test
must reject mutations to package membership, manifests, dependency classes, resolved
packages or features, ambient-package exclusions, source imports, include escapes,
schema, prose, duplicate keys, stage0 binding, and content identity.

## Nonclaims

- This contract changes no semantic decision, H-level, production authority, or fallback.
- It does not prove H2 authority, H3 bootstrap identity, H4 independence, or release
  readiness.
- It does not make dev-only parity implementations trusted or permit them in production.
- It does not broaden stage0 beyond the separately bound six-domain trust contract.
