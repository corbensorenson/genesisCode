# Version Surfaces v0.1

Normative compatibility map for GenesisCode release identities, serialized formats, hash domains, binary magics, and selfhost artifacts. The machine-readable authority is `genesis.version-surfaces.json`; its closed representation is `docs/spec/VERSION_SURFACES_v0.1.schema.json`.

## Rules

1. A production writer emits only the `current_writer` identity for its surface.
2. A reader rejects absent discriminators and unknown future identities unless a named migration record explicitly defines the exception.
3. Every accepted identity other than the current writer requires a migration record covering read behavior, write behavior, semantic delta, user action, regression evidence, and retirement policy.
4. Compatibility is per surface. The crate release, document version, package semantic version, hash profile, and container schema are independent identities.
5. Changing bytes covered by a hash profile or binary magic requires a new identity. A release-number bump alone cannot reinterpret existing bytes.
6. Current writers must never choose an older format based on optional content. Empty optional sections are encoded in the current format.
7. A migration reader must fail closed on malformed, absent, or future discriminators before interpreting version-specific payload fields.

## Reserved v1 Compatibility Namespace

`genesis.compatibility.json` is the machine-readable reservation ledger for the v1 compatibility namespace. Its closed representation is `docs/spec/V1_COMPATIBILITY_REGISTRY_v0.1.schema.json`, and `scripts/check_v1_compatibility.sh` verifies its semantics, dependency graph, source authorities, and negative controls.

Reservation is not stabilization. Every `genesis/compat/v1/*` ID below is permanently unavailable for any other meaning, but the repository-wide release claim remains `reserved-not-stable`. Nine entries bind to current pre-v1 candidates for development and migration work; bytecode remains deliberately unbound until an executable format exists. No current candidate may emit, accept, or advertise a reserved ID as stable. R9.1.a is the only roadmap task authorized to promote bindings after all seven promotion requirements pass.

| Surface | Reserved stable ID | Current binding | Dependencies |
|---|---|---|---|
| Language profile | `genesis/compat/v1/language-profile` | candidate `genesis/language-profile/v0.2` | none |
| CoreForm | `genesis/compat/v1/coreform` | candidate `genesis/coreform/v0.2` | language profile |
| Value/effect hash | `genesis/compat/v1/value-effect-hash` | candidate `genesis/value-effect-hash/v0.2` | CoreForm |
| Effect log | `genesis/compat/v1/effect-log` | candidate `genesis/effect-log/v3` | CoreForm, value/effect hash |
| Evidence | `genesis/compat/v1/evidence` | candidate `genesis/evidence-profile/v0.1` | language profile |
| Package | `genesis/compat/v1/package` | candidate `genesis/package-profile/v0.2` | CoreForm, evidence, patch, snapshot |
| Patch | `genesis/compat/v1/patch` | candidate `genesis/patch-profile/v0.2` | CoreForm, snapshot |
| Bytecode | `genesis/compat/v1/bytecode` | unbound | language profile, CoreForm, value/effect hash, effect log |
| Snapshot | `genesis/compat/v1/snapshot` | candidate `genesis/vcs-snapshot/v1` | CoreForm |
| Bootstrap | `genesis/compat/v1/bootstrap` | candidate `genesis/bootstrap-profile/v0.2` | language profile, CoreForm, value/effect hash |

### Promotion Contract

Every surface must satisfy the complete promotion set, including an unbound surface before it acquires a candidate:

1. `P-NORMATIVE-SPEC`: complete canonical grammar or wire specification, semantics, limits, and fail-closed behavior.
2. `P-GOLDEN-CORPUS`: portable positive, negative, legacy, malformed, and future-version vectors with deterministic identities.
3. `P-INDEPENDENT-VERIFIER`: separately built implementation that does not import the production implementation.
4. `P-MIGRATION`: deterministic migration and rollback for every predecessor, with semantic delta and support window.
5. `P-TIER-PARITY`: agreement across every shipping interpreter, compiler, host, and selfhost stage.
6. `P-SECURITY`: fuzzing, resource-limit, ambiguity, downgrade, and profile-confusion closure.
7. `P-R9-FREEZE`: independently verified R9.1.a release-candidate freeze.

Promotion is monotonic. A stable identity cannot be reinterpreted, demoted, or reassigned; incompatible bytes or semantics require a new identity. A dependency must be stable under the same release candidate before its dependent can become stable. Retiring a stable v1 reader requires a later major compatibility policy, an offline migrator, corpus or telemetry evidence, and a published support window. Candidate changes before freeze require a new candidate ID, migration record, golden vectors, and synchronized source authority.

## Current Matrix

| Surface | Current writer | Accepted readers | Missing discriminator |
|---|---|---|---|
| Product release | `0.2.0` | `0.2.0` | not applicable |
| `package.toml` | schema `1` | pre-schema, `1` | pre-schema is the named `M-PACKAGE-PRESCHEMA-TO-1` exception |
| `genesis.workspace.toml` | version `1` | `1` | reject |
| `genesis.lock` | version `2` | `1`, `2` | reject |
| `.gclog` | version `3` | `2`, `3` | reject |
| `.gpk` | version `2` | `1`, `2` | impossible after validated magic/version header read |
| Canonical hash profile | `genesis/hash-profile/gcv0.2-blake3` | same | not applicable |
| Compiled module blob | `GCKM5\0` | same | reject as bad magic |
| Selfhost compiled cache | `GCSHC1\0` | same | treat as cache miss; never execute |
| Selfhost toolchain artifact | `genesis/selfhost-toolchain-artifact-v0.2` | same | reject |

## Migration Records

### M-PACKAGE-PRESCHEMA-TO-1

Pre-schema manifests have schema 1 semantics. Maintained manifests and scaffolds emit top-level `schema = 1`; explicit values other than `1` are rejected. Add `schema = 1` as a top-level key. This compatibility reader remains through 0.x and may only be retired after corpus telemetry and a major compatibility decision.

### M-LOCK-1-TO-2

Explicit lock versions 1 and 2 are readable. Version 2 adds resolution strategy and provenance fields; current lock-producing operations write version 2. Regenerate and review the canonical lock diff. Version 1 removal requires an offline migrator and a release-note deprecation cycle.

### M-GCLOG-2-TO-3

Explicit log versions 2 and 3 are replayable. Version 3 binds the current value-hash encoding and deterministic task scheduling metadata; all runtime logs write version 3. Regenerate archival logs when possible. Version 2 removal requires an authenticated migrator and golden replay corpus.

### M-GPK-1-TO-2

Binary GPK versions 1 and 2 are readable. Version 2 adds a deterministic sorted refs section; production exports always write version 2, including a zero-count section. Re-export to migrate. Version 1 removal requires a streaming re-export tool and fixture census.

## GPK Wire Contract

- Header: four-byte `GPK\0` magic, little-endian `u32` version, 32-byte root hash, and little-endian `u64` entry count.
- Index: each fixed-width record contains a 32-byte hash, one-byte kind, seven zero reserved bytes, and little-endian `u64` payload offset and length.
- Payloads: records follow the index in canonical order with contiguous offsets.
- Version 2 refs: little-endian `u64` count followed by sorted unique entries, each containing a little-endian `u16` UTF-8 name length, name bytes, and a 32-byte hash. Zero refs still writes the count.
- Version 1: no refs section; trailing bytes are corruption.
- Readers enforce hard entry, payload, ref-count, and ref-name limits before allocation and reject duplicate hashes, duplicate refs, invalid kinds, non-canonical offsets, truncation, and trailing bytes.
- Import validates the complete bundle before applying optional ref mutations. Version 1 rejects a refs argument rather than discarding it.

## Change Procedure

1. Update the runtime constant and parser branch without reusing an existing identity for changed bytes or semantics.
2. Update `genesis.version-surfaces.json`, the owning format spec, `CHANGELOG.md`, and any generated examples in one change.
3. Add current-write, accepted-legacy, missing-discriminator, future-version, malformed-input, and round-trip tests as applicable.
4. Run `bash scripts/check_version_surfaces.sh`, `bash scripts/check_v1_compatibility.sh`, `bash scripts/check_versioning_release_hygiene.sh`, and `bash scripts/check_release_smoke.sh`.
5. Treat removal of a legacy reader as a compatibility event requiring evidence that the retirement rule is satisfied.
