> Deprecated Top-Level Doc: Use `docs/DEPRECATION_MAP_v0.1.md` for canonical replacements.

# GenesisGraph VCS + GenesisPkg Sharing System (v0.2 Addendum)

Audience: Codex agent implementing runtime/tooling.

Goal: GenesisCode does not require Git, GitHub, or an external package manager for versioning,
branching/merging, publishing, installing, and syncing.

This addendum defines the integrated VCS + package-sharing architecture and the minimal object model
needed to make it real, while keeping the kernel pure.

## 0) Core Thesis

GenesisCode replaces external VCS and external package managers with a single integrated system:

- **GenesisGraph (VCS):** a semantic, content-addressed DAG of commits/snapshots/patches/evidence.
- **GenesisPkg (Package Sharing):** publishing, installing, resolving dependencies, and syncing
  branches are done through GenesisGraph objects and refs.

Everything is a contract. A “package” is a contract that responds to standard package operations and
contains references to other contracts (functions/modules/contracts) by hash.

## 1) Non-Negotiable Architecture Constraints

### 1.1 Keep the kernel pure (TCB-A remains tiny)

The Gλ evaluator remains deterministic and effect-free.

Storage, networking, publishing, installing, syncing are effects interpreted by a capability runner
and recorded in effect logs.

### 1.2 Content-address everything

All meaningful objects are immutable artifacts addressed by hash.

### 1.3 Obligations gate acceptance

Publishing or advancing a branch ref is permitted only if the commit carries required obligations +
evidence artifacts, per policy.

### 1.4 History optional in bundles

Sharing can be:

- shallow (snapshot only, no history)
- full (commit history DAG + patches + evidence)

## 2) Object Model: Artifacts in a Merkle DAG

GenesisGraph stores immutable artifacts, each with:

- `:type` (kind)
- `:v` (schema version)
- canonical encoding (for stable hashing)
- content fields (schema-defined)

Artifacts are stored in a content-addressed store. The kernel does not care whether the store is
local, remote-backed, or layered.

### 2.1 Required artifact kinds

- Snapshot: `:vcs/snapshot`
- Patch: `:vcs/patch`
- Commit: `:vcs/commit`
- Evidence: `:vcs/evidence`
- RefState (optional, local serialization): `:vcs/refstate`
- Attestation: `:vcs/attestation`
- Conflict (merge result): `:vcs/conflict`
- Policy artifacts (recommended): `:vcs/policy`

## 3) Store + Refs + Sync as Capabilities (Effects)

Store/refs/sync are *capabilities* implemented by the host runner. All operations are effects,
logged deterministically, and replayable.

- Store capability: `core/store::*`
- Refs capability: `core/refs::*`
- Sync capability: `core/sync::*`

## 4) Packages Are Contracts (First-Class)

Packages must support a normative contract interface (see
`docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md` and `docs/POLICY_DEFAULTS_v0.1.md`).

## 5) Bundles, Locks, Policies, Merges, GC

This addendum is implemented by the following docs:

- CLI and file formats: `docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md`
- Policy defaults + ref protection: `docs/POLICY_DEFAULTS_v0.1.md`
- Lock generator rules/invariants: `docs/LOCK_GENERATOR_RULESET_v0.1.md`
- Remote registry protocol: `docs/REGISTRY_PROTOCOL_MINIMAL_v0.1.md`
- Reachability closure rules: `docs/REACHABILITY_RULES_v0.1.md`
- Garbage collection rules: `docs/GARBAGE_COLLECTION_RULES_v0.1.md`

## 6) Relationship to Existing v0.2 Supply-Chain Work

GenesisGraph attestations/signatures should compose with the existing acceptance-signing and local
transparency log features:

- signatures attach to commits/releases as `:vcs/attestation` artifacts
- policies decide which refs require signatures
- protected ref classes may require explicit attestation roles (for example `:reviewer` and
  `:verifier`) and enforce independence constraints between those roles
- transparency logs can record published commits/bundles/ref movements

Recommended attestation extension for protected profiles:

- optional `:role` field on `:vcs/attestation` (`symbol|string`)
- role-aware policy keys on `:vcs/policy` classes:
  - `:required-attestation-roles`
  - `:role-min-signatures`
  - `:independent-role-pairs`
