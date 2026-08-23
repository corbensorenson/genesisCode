# Self-hosted reference authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/refs::authority` binding is the production semantic authority for direct
`core/refs::{get,list,set,delete}` database decisions and local bulk mutations requested by
`core/gpk-low::import` and `core/sync::pull`. It owns exact-name lookup, prefix filtering, canonical
entry order, expected-old comparison, conflict versus write selection, single-name update or
deletion, bulk mode/input admission, strict bulk order and uniqueness, first-conflict attribution,
complete replacement snapshots, and the logical response content. The same authority supplies every
local ref lookup or listing consumed by GPK export, package publish, package ref and semver
resolution, and VCS log/blame/why history discovery.

The artifact-loaded `core/refs::policy-authority` binding is the production semantic authority for
the local VCS policy gate used by direct set/delete, GPK import ref updates, sync-push ref updates,
and package publication's accepted sync plan. It owns policy schema and class selection, frozen-ref
admission, obligation/evidence/assurance admission, requirements-trace and tool-qualification
admission, signer threshold, required-role, per-role minimum, and role-independence decisions. It
reuses the same GenesisCode package-publish policy implementation for commit-bearing updates and
owns the deletion-only class/frozen decision without fabricating a commit.

Rust remains the bounded mechanism for capability transport, stable content-addressed artifact
observation, BLAKE3 contradiction checks, Ed25519 verification, reference database parsing,
exclusive locking, crash-safe replacement, sorted transport construction, and retry after a
concurrent snapshot change. Registry reference state and transport remain host mechanisms. Sync
pull's existing `:refs` contract carries names rather than local policy hashes and therefore remains
an explicitly ungated bulk-import mode; changing that public contract is outside this migration.
Internal ref reads in GC remain a neutral observation governed by the separate H2 GC contract.
Registry references, direct legacy persistence APIs, the sync-pull policy contract, and the explicit
parity oracle have not migrated. These residuals keep `SD-REFS` at H0.

## Bootstrap

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly`. A missing artifact, missing
`core/refs::authority` binding, evaluator failure, sealed `ERROR`, resource exhaustion, or invalid
result is a hard error. There is no Rust semantic fallback.

The database evaluator limits are:

- 20,000,000 evaluation steps per request.
- 80,000,000 logical allocation units.
- 4 MiB bytes and string values.
- 65,536 map and vector entries.
- 16 optimistic write retries.

The policy evaluator uses the same 80,000,000-allocation, 4 MiB bytes/string, and 65,536
map/vector bootstrap envelope, then resets counters and applies a 50,000,000-step limit per phase.
Host observation is bounded to 4 MiB per artifact, 64 MiB total, and 4,096 requested
evidence/attestation objects. Every observed object's bytes must match its requested lowercase
BLAKE3 identity before UTF-8 or CoreForm decoding.

## Request

Every request is the exact map:

```text
{
  :kind "genesis/refs-authority-request-v0.1"
  :op   :get | :list | :set | :set-many
  :payload <operation payload>
  :v 1
}
```

The database snapshot is a map from string ref names to lowercase 64-hex artifact identities.

- `:get` payload: exact `{:name <string> :refs <snapshot>}`.
- `:list` payload: exact `{:prefix <string-or-nil> :refs <snapshot>}`.
- `:set` payload: exact `{:expected-old <hash-or-nil> :expected-old-present <bool>
  :name <string> :new-hash <hash-or-nil> :refs <snapshot>}`. Delete is represented by
  `:new-hash nil`; absence of CAS is distinct from an expected absent ref.
- `:set-many` payload: exact `{:mode <mode> :ops <ordered-ops> :refs <snapshot>}`. Each operation is
  the exact map `{:expected-old <hash-or-nil> :expected-old-present <bool> :name <string>
  :new-hash <hash-or-nil>}`. Operations MUST be strictly ordered by UTF-8 ref-name bytes, unique,
  and bounded to 4096 entries. `:cas` applies each operation's explicit expected-old contract;
  `:same-or-absent` permits only absent refs or refs already equal to their non-nil new hash; and
  `:unconditional` permits replacement regardless of the observed value.

## Result

Every result is the exact map with fields:

```text
[:action :code :current :entries :kind :message :ok :refs :request-h :v :value]
```

`:kind` is `genesis/refs-authority-result-v0.1`, `:v` is `1`, and `:request-h` is the canonical
GenesisCode term hash of the complete request.

- `:get` succeeds with `:action :read` and `:value` set to the selected hash or nil.
- `:list` succeeds with `:action :list` and `:entries` set to ordered exact
  `{:hash <hash> :name <name>}` maps.
- A permitted transition succeeds with `:action :write`, the observed `:current`, and the complete
  replacement snapshot in `:refs`.
- A failed expected-old comparison succeeds with `:action :conflict`, the observed `:current`, and
  no replacement snapshot.
- A bulk conflict also places exactly one `{:hash <current-or-nil> :name <name>}` record in
  `:entries`, identifying the first conflicting operation in canonical order. A permitted bulk
  transition returns the complete all-operations replacement snapshot and no entries.
- A malformed request returns `:ok false`, `:action :error`, and a closed code/message pair.

The Rust decoder rejects open fields, wrong request hashes, invalid identities, wrong action shapes,
lookup/list substitution, false conflict/write claims, wrong bulk conflict attribution, unsorted or
duplicate bulk inputs, and replacement snapshots that smuggle unrelated changes. These checks
constrain a corrupted authority result; they do not select the production decision.

## Policy request and result

Every policy request is the exact map:

```text
{
  :facts <exact bound policy/commit/ref facts>
  :kind "genesis/refs-policy-authority-request-v0.1"
  :mechanism <nil or exact phase observation>
  :phase :inspect | :prepare | :finalize | :delete
  :v 1
}
```

The facts map is exactly `[:commit :commit-h :depth :expected-old :policy :policy-h :ref
:remote]`. The host fixes `:depth 0`, `:expected-old nil`, and `:remote "local"`; it supplies the
exact policy term/hash and either the exact commit term/hash or two nils for deletion. The policy
and commit bytes are each read once under the artifact bounds and independently hash-checked.

For commit-bearing updates, `:inspect` selects the policy class and returns exact ordered evidence
and attestation hash requests plus a self-hash. `:prepare` consumes exact term/byte/hash envelopes
for only those requested objects, performs all non-cryptographic admission, and emits closed
Ed25519 verification requests plus a self-hash. Rust decodes those requests, derives the exact VCS
commit signing hash, performs only the requested Ed25519 mechanism checks, and returns
request-hash-bound booleans. `:finalize` revalidates all prior hashes and facts, applies signer and
role policy, and returns exact admission facts. `:delete` accepts no mechanism observation and
performs only complete policy parsing, frozen-prefix admission, and class selection.

Every phase result is the exact request-bound map
`[:code :kind :message :ok :request-h :v :value]`, with kind
`genesis/refs-policy-authority-result-v0.1`. Rejections use only declared `core/refs/*` policy
diagnostics. Accepted final/delete values are exactly `[:admit :commit-h :policy-h :ref]`; Rust
rejects any open field or contradiction. Missing, malformed, oversized, corrupt, unrequested,
request-substituted, self-hash-substituted, or identity-contradictory input fails closed before the
ref or remote mutation.

## Atomic write protocol

For set, delete, or set-many, the host snapshots the database, evaluates GenesisCode, and attempts
to replace the database only if the locked current snapshot is byte-semantically equal to the
evaluated snapshot. A mismatch causes a fresh snapshot and a fresh authority evaluation. Exhausting
the retry bound fails closed. GPK import submits one policy-admitted `:cas` batch. Sync pull fetches
all requested closures before submitting one `:same-or-absent` or `:unconditional` batch, so a
later conflict cannot leave an earlier ref updated. Direct and GPK mutations pass the policy
authority before local persistence. Sync push passes every requested remote ref update before
artifact upload or remote mutation, including package publish's recursive sync call. Rust never
converts a changed snapshot into its own conflict, write, policy, evidence, or signer verdict.

## Internal consumer routing

Production GPK export, package publish, package ref and semver resolution, and VCS root/history
lookup MUST call the same `RefsAuthority` adapter used by direct `core/refs` effects. The runner
loads that authority lazily for only the operations that can consume local refs. A missing
configuration or binding fails closed when a local ref is actually requested; hash-only and
commit-override paths do not invent a ref decision. Direct `RefsDb::{get,list}` fallback for these
consumers is compiled only under the explicit `parity-oracle` feature, never generic `cfg(test)` or
a production profile. Remote registry lookups remain registry observations and do not inherit this
local refs authority claim.

## Nonclaims

This contract does not claim H2 for `SD-REFS`, complete internal-ref consumer authority beyond the
named local routes, complete GPK/sync policy or transport authority, a new policy-bearing sync-pull
contract, registry reference authority, `R4.2.e` or SH-C closure, bootstrap fixpoint, or release
qualification.
