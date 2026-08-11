# GenesisCode Package Profile Negotiation v0.1

Status: normative pre-v1 profile. The immutable profile ID is
`genesis/profile-negotiation-v0.1`.

## Purpose

Package compatibility is an explicit admission decision, not an inference from a release label,
host executable, file extension, or successful parse. A negotiated package declares language,
capability, artifact, and target requirements. A verifier compares those requirements with a
reviewed offer before execution and either emits one deterministic negotiated identity or fails
closed.

This profile is an opt-in pre-v1 boundary. Legacy modules without either negotiation field remain
outside this compatibility claim. If any module contains `:profile-negotiation` or
`:package-profile-requirements`, every module in the supplied package closure must satisfy this
profile exactly.

## Package declaration

Every module in an active package closure carries identical metadata:

~~~clojure
:profile-negotiation genesis/profile-negotiation-v0.1
:package-profile-requirements {
  genesis/profile-family/language {
    :mode exact
    :profile genesis/language-profile/v0.2}
  genesis/profile-family/capability {
    :mode minimum
    :profile genesis/capability-profile/pure-v0.1}
  genesis/profile-family/artifact {
    :mode exact
    :profile genesis/artifact-profile/coreform-v0.2}
  genesis/profile-family/target {
    :mode exact
    :profile genesis/target-profile/portable-host-v0.1}}
~~~

The requirements map is closed over exactly those four family symbols. Each requirement is a
closed map containing exactly `:mode` and `:profile`, both symbols. The only modes are `exact` and
`minimum`. Missing, additional, malformed, unknown, or module-disagreeing fields fail. Active
negotiation also requires successful `MODULE_RESOLUTION_PROFILE_v0.1`; package order, portable
paths, content, imports, exports, and exact foundational profiles therefore have a prior identity.

## Reviewed compatibility registry

Compatibility is declared by this profile's registry and never parsed from digits, semantic-version
syntax, prefixes, or profile names.

| Family | Ordered compatible lineage | Current meaning |
|---|---|---|
| `genesis/profile-family/language` | `genesis/language-profile/v0.2` | Exact current language semantics. |
| `genesis/profile-family/capability` | `genesis/capability-profile/pure-v0.1`, then `genesis/capability-profile/host-abi-v0.1` | Pure kernel availability, followed by the indexed host-ABI availability surface. |
| `genesis/profile-family/artifact` | `genesis/artifact-profile/coreform-v0.2` | Canonical CoreForm v0.2 source/module artifacts accepted by the current runtime. |
| `genesis/profile-family/target` | `genesis/target-profile/portable-host-v0.1` | The current portable host runtime boundary, not a build or deployment target claim. |

An `exact` requirement succeeds only when the offer explicitly contains the named member. A
`minimum` requirement succeeds only when the offer contains that member or a later member in the
same reviewed lineage. When several offered members satisfy a minimum, negotiation chooses the
earliest satisfying member. This least-compatible selection minimizes accidental semantic and
capability widening. Unknown families, unregistered members, and empty offers fail; no fallback,
alias, nearest version, or host-dependent discovery exists.

Adding a member is not automatically monotonic. A reviewer may append it to an existing lineage
only after proving it satisfies every earlier minimum contract. An incompatible revision requires a
new family or profile version and migration. Reordering or removing lineage members changes this
profile and cannot reinterpret an existing v0.1 identity.

## Offers and authority

`ProfileOffer` is typed verifier input. `ProfileOffer::core_host()` advertises only the current
language, pure and host-ABI capability availability, canonical CoreForm artifact, and portable host
runtime target. A future build/runtime path constructs an explicit offer with
`ProfileOffer::from_profiles`; the constructor rejects unknown families, unknown members, and
duplicates before package negotiation.

An offer states verifier-checked implementation availability. It is not package intent, a lockfile,
a capability grant, an effect policy, a target SDK installation, release evidence, or deployment
authority. `typecheck_package` uses the built-in Core offer. Target-specific callers must use
`typecheck_package_with_profile_offer` and must not execute when the returned report is not `ok`.

## Capability separation

Negotiated capability availability and runtime authorization are separate conjunctive controls:

1. the capability profile must be compatible;
2. module `:caps` must be a duplicate-free symbol vector;
3. the selected pure profile requires `:caps []`;
4. type/effect checking must prove every inferred operation is declared; and
5. the deny-by-default runtime policy must independently grant each operation and resource scope.

The host-ABI availability profile grants nothing. An operation appearing in the host or Prelude
index does not make it authorized. Negotiation cannot weaken policy, invent a capability, widen
resource bounds, bypass hard cancellation, or suppress effect/replay checks.

## Deterministic result and identity

`ProfileNegotiationReport` records activation, success, lexical per-module errors, the canonical
requirements, selected members, and an optional identity. `TypecheckReport::to_term` projects the
same facts under `:profile-negotiation`; `:identity` is the 32-byte hash on success and `nil` for an
inactive or invalid package. The identity exists only when:

- module resolution is active, successful, and has an identity;
- all modules declare exact negotiation-profile and requirement metadata;
- every requirement is registered and compatible with the offer;
- all modules agree byte-semantically on requirements; and
- capability metadata satisfies the selected profile.

The negotiated identity hashes a canonical CoreForm map containing the immutable negotiation
profile ID, module-resolution identity, sorted requirement maps, and sorted selected profiles under
`genesis/hash-profile/gcv0.2-blake3`. It binds the selected result, not unrelated extra members in an
offer. Requirement mode/profile changes, module resolution changes, or selected-member changes alter
the identity. Invalid or inactive packages receive no negotiated identity.

Errors are sorted first by portable module path and then lexically. Unsupported diagnostics name
the mode, required member, family, and explicit offered members. Negotiation performs no filesystem,
environment, network, process, time, random, model, or target probing.

## Execution boundary

The package ABI and package-obligation paths typecheck supplied modules without evaluating their
expressions. Because `typecheck_package` now merges negotiation errors into module errors, an active
package with an unsupported combination returns `ok = false` before any package module expression
is executed. A caller that uses a custom target offer has the same obligation through
`typecheck_package_with_profile_offer`. Raw-file evaluation is not package-profile admission,
receives no negotiated identity, and cannot claim package compatibility. Successful negotiation
does not replace module resolution, contract composition, type/effect checking, artifact
verification, package locking, capability policy, obligations, or translation validation; all
applicable controls must succeed.

## Change and migration rule

A change updates this prose, the recursively closed JSON/schema, registry constants, negotiation and
typecheck implementation, module-resolution relationship, positive and adversarial controls,
generated agent references, migration guidance, and content identity in one reviewed transaction.
Existing v0.1 identities remain immutable. `R5.3.d` owns canonical migration patches; this profile
does not guess replacements for unavailable requirements.

## Nonclaims

- This profile does not claim v1 stability or compatibility with an undeclared profile.
- The portable-host target is not native, browser, edge, WASI, mobile, OCI, component, GPU, game,
  embedded, Raspberry Pi, or MCU build/deployment evidence.
- The CoreForm artifact profile does not stabilize bytecode, native objects, components, packages,
  snapshots, or container formats.
- Host-ABI availability does not grant capabilities or prove an operation exists on every platform.
- Minimum compatibility is not semantic-version comparison and does not cross a family boundary.
- Inactive legacy packages obtain no negotiated identity and cannot claim negotiated compatibility.
- This profile switches no self-host authority and promotes no package, backend, benchmark, Foundry
  result, model, product target, assurance level, or release level.
