# Assurance Artifacts v0.1

This document defines deterministic assurance evidence artifacts used by regulated-release policy gates.

Objective-level standards posture mapping is tracked in:
- `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
- `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`

## 0. Evidence Envelope Profile

### 0.1 Pinned standards and schemas

GenesisCode evidence uses three distinct layers and MUST NOT call the in-toto Statement an envelope:

1. The statement layer is [in-toto Attestation Framework v1.2](https://github.com/in-toto/attestation/tree/main/spec/v1), with `_type` exactly `https://in-toto.io/Statement/v1`.
2. The authentication layer is [DSSE v1](https://github.com/secure-systems-lab/dsse), with `payloadType` exactly `application/vnd.in-toto+json` and signatures over DSSE pre-authentication encoding (PAE).
3. Build outputs additionally carry [SLSA Provenance v1](https://slsa.dev/spec/v1.2/build-provenance) using `predicateType` exactly `https://slsa.dev/provenance/v1`. The project profiles SLSA 1.2 behavior but uses the stable major-version predicate URI required by SLSA.

The versioned producer schemas are:

- `docs/spec/GENESIS_EVIDENCE_PREDICATE_v0.1.schema.json`
- `docs/spec/GENESIS_EVIDENCE_STATEMENT_v0.1.schema.json`
- `docs/spec/GENESIS_SLSA_BUILD_v1.schema.json`
- `docs/spec/GENESIS_EVIDENCE_BUNDLE_v0.1.schema.json`

The canonical authenticated vector is `docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json`. `scripts/check_genesis_evidence_profile.sh` validates it without changing retained files; `scripts/update_genesis_evidence_profile.sh` is the only refresh entrypoint. The vector's Ed25519 key is public test material, is not a trust root, and MUST NOT authorize production evidence.

Schemas pin exact major/profile versions. A producer MUST NOT follow a mutable `latest` schema. A compatible minor revision requires new vectors and a documented monotonicity analysis; incompatible semantics require a new predicate URI and schema filename. Verifiers MUST reject unsupported Genesis predicate versions. SLSA verifiers MUST follow SLSA's monotonic parsing rule and ignore unknown standard extension fields unless local policy assigns deny-only meaning to them.

### 0.2 Statement and predicate rules

The Genesis predicate URI is `https://genesiscode.dev/attestations/evidence/v0.1`. It records, without host-path leakage:

- source repository URI, Git revision, tree SHA-256, dirty bit, dirty policy, and a dirty-path-set digest when dirty input is explicitly permitted;
- tool name/version and SHA-256-addressed tool artifacts;
- environment profile, OS, architecture, optional container digest, and names of declared environment variables without secret values;
- deny-by-default network mode and every permitted network input by URI, SHA-256, and purpose;
- executed commands as argument vectors, repository-relative working directories, declared environment names, and exit codes, never ambiguous shell command strings;
- required negative controls, expected and observed outcomes, pass state, and optional content-addressed output;
- subject artifacts with normalized logical paths, SHA-256, byte sizes, and media types;
- integer duration nanoseconds, peak RSS bytes, signed disk delta bytes, and integer raw samples with explicit units;
- verifier name, version, and SHA-256-addressed implementation artifact;
- links to existing Genesis content-addressed acceptance, obligation, replay, bootstrap, or assurance artifacts.

Every in-toto subject MUST exactly match one Genesis predicate artifact by logical name and SHA-256. Arrays whose order has no runtime meaning are emitted in canonical key order and sorted by their declared identity. Duplicate JSON keys, floats, NaN/infinity, absolute/parent-aliased paths, drive-qualified paths, unnormalized separators, unsupported versions, undeclared network inputs, failed negative controls, and implicit dirty inputs fail closed.

Dirty source is never inferred as acceptable. `dirty=false` requires `dirtyPathsDigest=null`. `dirty=true` requires `dirtyPolicy=allow-declared` and a digest of the canonical sorted dirty path/material set. Release policy MAY prohibit dirty evidence regardless of this representation.

Measurements are authenticated observations, not semantic build inputs. Duration, RSS, disk delta, and raw samples are included in the signed statement but MUST NOT influence source tree identity, artifact bytes, replay semantics, cache keys, or reproducibility comparisons. A verifier compares them only when a policy explicitly names a measurement budget and compatible environment profile.

### 0.3 SLSA build boundary

A process that transforms declared source/dependencies into an output artifact emits a second Statement with the same subjects and SLSA Provenance v1:

- `buildDefinition.buildType` is `https://genesiscode.dev/buildtypes/roadmap-evidence/v0.1` for this profile;
- `externalParameters` identifies the Genesis evidence profile and command argument vectors;
- `internalParameters` identifies the environment and network profiles;
- `resolvedDependencies` binds source trees, toolchains, and fetched inputs by digest;
- `runDetails.builder` identifies the producer and its version/dependencies;
- `runDetails.byproducts` links the companion Genesis statement by SHA-256.

Source-only audits, replay checks, benchmarks, and obligation runs are not mislabeled as SLSA builds. They use the Genesis predicate. GenesisCode does not invent a `slsa.dev` source predicate: source-policy assertions use a separately versioned Genesis predicate until an applicable standardized SLSA Source-track attestation is explicitly profiled. Emitting the format does not claim SLSA Build L1/L2/L3; level claims require separate platform, provenance-generation, isolation, authenticity, and policy evidence.

The checked-in SLSA schema is a strict Genesis producer profile. Interoperable consumers honor the authoritative SLSA parsing rules, including monotonic unknown-field handling. Genesis extension fields use a `genesis_` prefix and MUST be deny-monotonic: removing or ignoring one cannot turn a denial into an allow.

### 0.4 Canonical bytes, DSSE, and trust

Genesis canonical statement JSON is UTF-8 with sorted object keys, compact separators, no insignificant whitespace, no duplicate keys, and integer-only numbers. DSSE `payload` is standard padded base64 of those exact bytes. PAE is:

`DSSEv1 SP len(payloadType) SP payloadType SP len(payload) SP payload`

where lengths are decimal byte counts and `SP` is one ASCII space. Bundle attestations retain both the decoded statement and DSSE envelope for auditability; validators MUST prove the envelope payload is byte-identical to canonical serialization of the retained statement before signature evaluation.

The authenticated profile uses Ed25519. `keyid` is `sha256:<lowercase hex SHA-256 of the raw 32-byte public key>`. Signatures are standard base64 of the 64-byte Ed25519 signature over PAE bytes. Signature validity proves control of a key, not authorization. Trust roots, thresholds, role separation, expiration/revocation, and identity policy are external verifier inputs and MUST NOT be accepted from the bundle being verified.

E1 and E2 bundles MAY omit envelopes when policy explicitly permits unsigned local/review evidence. E3 and E4 bundles require an envelope and at least one valid, authorized signature for every statement. E4 additionally requires an independently operated verifier and policy-defined signer/verifier separation; self-produced reports cannot promote themselves. R0.2.c owns the independent read-only cryptographic verifier and trust-policy implementation.

### 0.5 Relationship to existing artifacts

CoreForm evidence in `.genesis/store`, `genesis/acceptance-v0.2` records, acceptance signatures, replay logs, bootstrap witnesses, and assurance packs remain their domain authorities. The Genesis evidence predicate links their content identities and execution context; it does not reinterpret their semantics. Existing build files named `*.sig` that contain only a SHA-256 digest are integrity sidecars, not digital signatures. New code and documentation MUST call them digest sidecars, and migration code MUST NOT treat them as DSSE or authorization evidence.

### 0.6 Independent verifier profile

`tools/genesis-evidence-verifier` is a standalone Cargo workspace and MUST remain absent from the root Rust workspace and Genesis CLI dependency graphs. It has no Genesis crate dependencies, signing command, output-file option, network path, or authority-changing operation. Its only successful operation reads a bundle, an externally selected trust policy, a hash-tree manifest, and an artifact root; verifies them; and emits one deterministic JSON report on stdout. Verification failure emits one deterministic JSON diagnostic on stderr and exits nonzero.

The command contract is:

```text
genesis-evidence-verifier \
  --bundle <bundle.json> \
  --policy <trust-policy.json> \
  --policy-sha256 <out-of-band-lowercase-hex> \
  --artifact-tree <tree.json> \
  --artifact-root <directory>
```

The caller MUST obtain `--policy-sha256` through an authenticated channel independent of the bundle and supplied policy file. A policy cannot authorize itself: the verifier hashes the exact policy bytes before parsing or using any key, role, threshold, compatibility, or limit from it. Bundle-provided keys are never trust roots. Signature validity counts only when the externally pinned policy authorizes the key for the predicate role and the bundle-profile threshold is met.

The policy and test-vector contracts are:

- `docs/spec/GENESIS_EVIDENCE_TRUST_POLICY_v0.1.schema.json`
- `docs/spec/GENESIS_ARTIFACT_HASH_TREE_v0.1.schema.json`
- `docs/spec/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.schema.json`
- `policies/evidence_verifier_trust_v0.1.json`
- `docs/program/evidence/GENESIS_EVIDENCE_ARTIFACT_TREE_v0.1.json`
- `docs/program/evidence/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.json`

The v0.1 Merkle profile sorts normalized logical paths by UTF-8 byte order. For each file it computes:

`SHA-256("GenesisCodeHashTreeLeafv0.1\0" || u64be(path-byte-length) || path-utf8 || u64be(size-bytes) || artifact-sha256-bytes)`

It combines adjacent nodes as:

`SHA-256("GenesisCodeHashTreeNodev0.1\0" || left-32-bytes || right-32-bytes)`

An unpaired final node is promoted unchanged to the next level. Empty trees, duplicate/unsorted paths, absolute paths, `..`, drive-qualified paths, backslashes, repeated separators, symlink components, non-files, size changes, digest changes, tree-manifest substitution, and roots that escape the canonical artifact root fail closed. Artifacts are SHA-256 hashed in bounded streaming chunks rather than loaded into memory.

The verifier rejects duplicate JSON keys, floating-point numbers, trailing data, unknown Genesis fields, unsupported versions/predicates/profiles, noncanonical base64, DSSE payload mismatch, forged/untrusted/role-ineligible signatures, threshold failure, dirty source outside policy, stale source identity, undeclared network behavior, failed negative controls, incompatible environment/verifier/build/builder identities, divergent Genesis/SLSA subjects or commands, missing companion-statement linkage, artifact mismatch, and nonexact hash-tree coverage. Resource limits are externally pinned, with an implementation hard ceiling of 16 MiB per JSON input.

Source freshness is policy-relative and MUST NOT be inferred from a producer-signed claim. The out-of-band trust policy pins the expected repository URI, Git revision, and source-tree SHA-256. The verifier first validates their representation and then requires exact equality. A syntactically valid but different repository, revision, or tree is stale for that verification invocation and fails closed. When clean source is required, `dirty=false`, `dirtyPolicy=reject`, and `dirtyPathsDigest=null` are mandatory; dirty input without an explicit allowed policy and dirty-path digest is malformed rather than implicitly accepted.

`scripts/check_genesis_evidence_verifier.sh` builds and tests the verifier offline in an ignored target directory, proves dependency-graph separation and absence of production signing/write paths, runs the positive vector twice for byte-identical output, executes every published negative case at its expected diagnostic boundary, and proves retained evidence remains unchanged. `scripts/update_genesis_evidence_verifier_vectors.sh` is the explicit vector refresh path. This verifier is independent of the producer implementation and main CLI dependency graph, but its checked-in fixture key remains test-only and conveys no release authority.

### 0.6.1 Adversarial evidence and replay matrix

`docs/program/EVIDENCE_ADVERSARIAL_MATRIX_v0.1.json` binds R0.2.e's eight rejection requirements to eleven executable controls. Each control identifies one authority, fixture, command, and expected diagnostic. Verifier cases refer to the published 30-case negative-vector catalog; replay refers to `replay_adversarial_matrix_rejects_reordered_and_altered_facts`, which rejects 16 mutations spanning entry order/count/index, operation and request hashes, response body/hash, decision/capability metadata, and deterministic scheduler facts.

`scripts/check_evidence_adversarial_matrix.sh` rejects duplicate or incomplete matrix data, requires exact one-time control coverage, verifies catalog diagnostics have not drifted, and runs both authoritative tests offline. Adding a requirement without a control, orphaning a control, renaming a fixture, changing an expected boundary, or removing either runtime test fails the gate. This matrix is reviewed program metadata outside `docs/program/evidence/`; it does not alter or recursively enter the signed E2 conformance fixture tree.

### 0.7 Evidence storage and immutable publication

`policies/evidence_storage_classes_v0.1.json` and `docs/spec/EVIDENCE_STORAGE_CLASSES_v0.1.schema.json` define storage authority independently from the class written inside an evidence payload:

- E0 is a mutable local/CI observation under ignored `.genesis/`. It is never release authority and MUST NOT be tracked.
- E1 is a reviewed in-tree schema or example under `docs/spec/`. It defines representation, not a runtime or release claim.
- E2 is a reviewed in-tree golden under `docs/program/evidence/`. It may contain signed E3/E4-shaped conformance data, but remains test-only and has no release authority.
- E3 is authenticated, independently verified release evidence emitted only under ignored `.genesis/release-assets/evidence/E3/` and published through a create-new immutable release channel with at least one mirror.
- E4 is independently reproduced release evidence emitted only under ignored `.genesis/release-assets/evidence/E4/`; it requires at least two policy-authorized signatures, independent verification, at least two mirrors, and offline verification.

An evidence payload cannot promote itself. A claimed `E3` or `E4`, a valid signature, source-control presence, local packaging, or a self-produced verification report does not grant authority. Authority requires an externally pinned trust policy, the independent verifier, storage-class policy, immutable publication, and the class-specific mirror threshold. Generated assets remain `candidate-until-immutable-publication` until those external controls succeed. GenesisCode does not currently claim that its test-only E3 fixture is E3 release evidence or that an E4 release exists.

`docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json` exactly covers every file below `docs/program/evidence/`, records its SHA-256 and role, fixes its distribution class at E2, and sets `authority=false` and `testOnly=true`. Its explicit update command is `scripts/update_evidence_fixture_classification.sh`. Adding, removing, or changing a fixture without refreshing this classification fails closed.

The release asset profile is deterministic USTAR with sorted normalized logical paths, mtime/uid/gid set to zero, empty owner names, file mode `0644`, and no links, devices, absolute paths, parent aliases, repeated separators, drive-qualified paths, local `.genesis` material, or bundled trust roots. It contains:

- `release-manifest.json`, exactly covering every payload by path, SHA-256, and byte size;
- `MIRROR.json`, class-specific mirror and offline verification rules without circularly embedding the outer archive digest;
- `evidence/bundle.json` and `evidence/artifact-tree.json` in canonical retained encoding;
- subject artifacts under `evidence/artifact/`;
- `verification/result.json` from the standalone independent verifier.

The archive SHA-256 is embedded in its filename. A matching `.sha256` sidecar and external `.mirror.json` descriptor bind name, digest, size, media type, class, release ID, minimum mirror count, and argument-vector instructions. The trust policy and trust-policy digest are deliberately absent from the archive trust boundary and MUST arrive through an authenticated out-of-band channel.

Render an immutable candidate explicitly:

```text
bash scripts/update_evidence_release_asset.sh E3 <release-id>
```

For non-default E3/E4 inputs, pass bundle, tree, artifact root, trust policy, and out-of-band trust-policy SHA-256 as the remaining arguments. The renderer runs the independent verifier before packaging, requires the verified bundle profile to equal the requested storage class, enforces that class's signature threshold, and creates files with exclusive `create-new` semantics. It will not overwrite even byte-identical output.

Verify a downloaded release directory without extracting it:

```text
python3 scripts/lib/evidence_storage.py verify-release \
  --release-dir <directory>
```

This command validates packaging, content address, sidecar, descriptor, safe USTAR structure, normalized metadata, exact manifest coverage, class/profile binding, and the embedded verifier-result linkage. It does not replace cryptographic evidence verification. Follow the descriptor's `verify-offline` argument vector with the independently obtained trust policy after safely materializing the logical payloads.

Create a no-overwrite byte-identical mirror:

```text
python3 scripts/lib/evidence_storage.py mirror-release \
  --source-dir <verified-primary-directory> \
  --destination-dir <new-mirror-directory>
```

The source is verified before copying, the destination directory and files are created exclusively, the destination is verified afterward, and every byte is compared. Existing destinations are rejected rather than replaced. `scripts/check_evidence_storage_classes.sh` proves E0 ignore behavior, fixture coverage, deterministic E3 rendering, offline package validation, mirror parity, retained-tree immutability, and rejection of overwrite, E3-to-E4 escalation, non-release classes, archive mutation, duplicate policy keys, fixture authority escalation, and archive path traversal.

## 1. Requirements Trace Artifact

Artifact kind:
- `:type :vcs/evidence`
- `:kind :requirements-trace`

Required fields:
- `:status :verified`
- `:graph-h <hex64>` content hash of the requirement graph source
- `:release`
  - `:snapshot <hex64>` release snapshot hash (required)
  - `:commit <hex64>|nil` optional commit hash binding
  - `:policy <hex64>|nil` optional policy hash binding
- `:requirements` vector of requirement maps:
  - `:id <string>`
  - `:level :system|:hlr|:llr`
  - optional `:parents [<string> ...]`
  - optional `:hazards [<string> ...]`
  - `:links` map with at least one of:
    - `:modules [{:path <string> :exports [<qualified-sym> ...]} ...]`
    - `:obligations [<symbol|string> ...]`
    - `:evidence-kinds [<symbol|string> ...]`

Policy gate behavior:
- if `:release/:commit` is present, it MUST match the protected commit hash
- `:release/:snapshot` MUST match the protected commit result snapshot
- linked obligations/evidence-kinds MUST not dangle
- malformed artifacts fail closed

Pre-commit binding:
- `:release/:commit = nil` is allowed so trace evidence can be produced before commit finalization.
- This avoids unsatisfiable hash cycles between commit hash and evidence hash while preserving snapshot/policy anchoring.

## 2. Tool Qualification Artifact

Artifact kind:
- `:type :vcs/evidence`
- `:kind :tool-qualification`

Required fields:
- `:status :qualified`
- `:release`
  - `:commit <hex64>|nil` optional commit hash binding
  - `:snapshot <hex64>` required release snapshot binding
  - `:policy <hex64>|nil` optional policy hash binding
- `:requirements [<string> ...]` non-empty
- `:tools` non-empty vector of maps:
  - `:name <string>`
  - `:path <string>`
  - `:blake3 <hex64>`
  - `:size-bytes <int>`
- `:qualification-tests` non-empty vector of maps:
  - `:id <string>`
  - `:artifact <hex64>`
  - `:manifest <hex64>` run-manifest artifact hash
  - `:run-id <string>`
  - `:runner <string>`
  - `:profile <string>`
  - `:snapshot <hex64>` (must match `:release/:snapshot`)
  - `:policy <hex64>|nil`
  - `:result :pass`

Qualification run-manifest linkage requirements:
- Each `--test-artifact id=<run-manifest-hex64>` must resolve from local `.genesis/store`.
- Run manifest `:kind` must be `genesis/qualification-test-run-manifest-v0.1`.
- Run manifest must bind:
  - `:test-id` (must equal CLI `<id>`),
  - `:artifact` (must exist in local store),
  - `:result :pass`,
  - `:profile` (must equal qualification profile),
  - `:release/:snapshot` (must equal qualification `--snapshot`),
  - `:release/:policy` (must equal qualification `--policy` when provided),
  - optional `:release/:commit` (must equal qualification `--commit` when provided),
  - run metadata fields `:run-id` and `:runner`.
- Referenced test artifact payload must parse as CoreForm map and declare `:ok true`.

Policy gate behavior:
- if `:release/:commit` is present, it MUST match the protected commit hash
- `:release/:snapshot` MUST match the protected commit result snapshot
- if `:release/:policy` is present and policy gating provided a policy hash, it MUST match
- test entries with non-`:pass` results fail closed
- qualification-test lineage fields (`:manifest`, `:run-id`, `:runner`, `:snapshot`) are required and validated fail-closed

Pre-commit binding:
- `:release/:commit = nil` is allowed for pre-commit qualification evidence attachment.

## 2.5 High-Assurance Supplemental Artifacts

Object-equivalence artifact (`genesis/object-equivalence-v0.1`) required fields:
- `:kind "genesis/object-equivalence-v0.1"`
- `:ok true`
- `:trace-artifact <hex64>`
- `:qualification-artifact <hex64>`
- `:source-artifact <hex64>`
- `:object-artifact <hex64>`
- `:method <symbol|string>`

Independent verifier run artifact (`genesis/independent-verifier-run-v0.1`) required fields:
- `:kind "genesis/independent-verifier-run-v0.1"`
- `:ok true`
- `:assurance-profile <symbol|string>`
- `:trace-artifact <hex64>`
- `:qualification-artifact <hex64>`
- `:object-equivalence-artifact <hex64>`
- `:run-id <string>`
- `:runner <string>`
- `:roles [<symbol|string> ...]` (minimum 2 entries)
- `:result :pass`

## 3. Assurance Pack Artifact

Artifact kind:
- `:type :vcs/evidence`
- `:kind :assurance-pack`

Required fields:
- `:status :ready`
- `:target-profile :custom|:do178c-dal-a|:do178c-dal-b|:nasa-class-a|:nasa-class-b|:iec62304-class-c`
- `:release`
  - `:snapshot <hex64>` required
  - `:commit <hex64>|nil` optional commit hash binding
  - `:policy <hex64>|nil` optional policy hash binding
- `:trace-matrix` map:
  - `:artifact <hex64>` requirements-trace artifact hash
  - `:source <string>` source path used to load the trace artifact
  - `:requirements [<map> ...]` copied requirement trace payload
- `:qualified-tool-manifest` map:
  - `:artifact <hex64>` tool-qualification artifact hash
  - `:source <string>` source path used to load the qualification artifact
  - `:tools [<map> ...]`
  - `:qualification-tests [<map> ...]`
- `:coverage-exports` vector of maps:
  - `:artifact <hex64>`
  - `:profile <symbol>`
  - `:ok <bool>`
  - `:source <string>`
- `:object-equivalence` map|`nil`:
  - `:artifact <hex64>`
  - `:source <string>`
  - `:source-artifact <hex64>` low-level/source binary artifact hash
  - `:object-artifact <hex64>` emitted object/binary artifact hash
  - `:method <symbol>` deterministic equivalence method identifier
- `:independence-attestations` vector of maps:
  - `:kind :independence-attestation`
  - `:roles [<symbol> <symbol>]`
  - `:attestor <string>`
- `:independent-verifier-runs` vector of maps:
  - `:artifact <hex64>`
  - `:run-id <string>`
  - `:runner <string>`
  - `:roles [<symbol> ...]`
  - `:source <string>`
- `:external-control-bindings` map:
  - `:contract "genesis/assurance-external-control-bindings-v0.1"`
  - `:assurance-profile <symbol>`
  - `:standard-family <string>`
  - `:workflow-target <string>` deterministic external workflow lane hint
  - `:crosswalk` map:
    - `:kind "genesis/assurance-standards-crosswalk-v0.1"`
    - `:version "0.1"`
    - `:source "docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json"`
  - `:release-artifacts` map:
    - `:trace-artifact <hex64>`
    - `:qualification-artifact <hex64>`
    - `:coverage-artifacts [<hex64> ...]`
    - `:object-equivalence-artifact <hex64>|nil`
    - `:independent-verifier-run-artifacts [<hex64> ...]`
  - `:objective-bindings` vector of maps:
    - `:objective-id <string>`
    - `:status <string>`
    - `:summary <string>`
    - `:evidence-refs [<string> ...]`
    - `:evidence-artifacts [<hex64> ...]`
  - `:external-controls` vector of maps:
    - `:control-id <string>`
    - `:status <string>`
    - `:summary <string>`
    - `:owner <string>`
    - `:tracked-in <string>`
    - `:closure-bundle <string>|nil`
    - `:immutable-refs [<string> ...]`
  - `:unresolved-control-count <int>`
  - `:unresolved-open-count <int>`

Profile gate behavior:
- `:do178c-dal-a` and `:nasa-class-a` require at least one independence attestation and minimum `:mcdc` coverage rank.
- `:do178c-dal-b` and `:nasa-class-b` require at least one independence attestation and minimum `:decision` coverage rank.
- `:iec62304-class-c` requires minimum `:symbol` coverage rank.
- regulated profiles (`:do178c-dal-a`, `:do178c-dal-b`, `:nasa-class-a`, `:nasa-class-b`, `:iec62304-class-c`) require:
  - one valid object-equivalence artifact (`genesis/object-equivalence-v0.1`),
  - at least one independent verifier run artifact (`genesis/independent-verifier-run-v0.1`) with `:result :pass`, profile binding, and hash linkage to trace/qualification/object-equivalence artifacts.
- all profiles require `:external-control-bindings` and the crosswalk source kind/version contract.
- `:custom` has no additional profile constraints beyond valid trace/qualification artifacts.

Deterministic bundle mirror behavior:
- optional `--bundle-dir <dir>` materializes reproducible files:
  - `assurance_pack.gc`
  - `requirements_trace.gc`
  - `tool_qualification.gc`
  - `coverage/*.gc`
  - `object_equivalence.gc`
  - `independent_verifier/*.gc`
  - `bundle_manifest.gc`

## 4. Deterministic CLI Emitters

- `genesis gcpm trace` emits `genesis/pkg-requirements-trace-v0.1`.
- `genesis gcpm qualify` emits `genesis/pkg-tool-qualification-v0.1`.
- `genesis gcpm assurance-pack` emits `genesis/pkg-assurance-pack-v0.1`.

All three commands:
- produce canonical CoreForm evidence bytes
- support `--no-store` for deterministic file-only output
- import to local store when `--no-store` is not set
