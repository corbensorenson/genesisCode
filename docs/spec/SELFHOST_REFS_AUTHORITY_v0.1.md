# Self-hosted reference authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/refs::authority` binding is the production semantic authority for direct
`core/refs::{get,list,set,delete}` database decisions. It owns exact-name lookup, prefix filtering,
canonical entry order, expected-old comparison, conflict versus write selection, single-name update
or deletion, and the logical response content.

Rust remains the bounded mechanism for capability transport, reference database parsing, exclusive
locking, crash-safe replacement, and retry after a concurrent snapshot change. The existing Rust
policy/evidence/signature admission gate runs before set or delete. Bulk updates used by sync and GPK
flows remain host-authoritative. These residuals keep `SD-REFS` at H0.

## Bootstrap

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly`. A missing artifact, missing
`core/refs::authority` binding, evaluator failure, sealed `ERROR`, resource exhaustion, or invalid
result is a hard error. There is no Rust semantic fallback.

The evaluator limits are:

- 20,000,000 evaluation steps per request.
- 80,000,000 logical allocation units.
- 4 MiB bytes and string values.
- 65,536 map and vector entries.
- 16 optimistic write retries.

## Request

Every request is the exact map:

```text
{
  :kind "genesis/refs-authority-request-v0.1"
  :op   :get | :list | :set
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
- A malformed request returns `:ok false`, `:action :error`, and a closed code/message pair.

The Rust decoder rejects open fields, wrong request hashes, invalid identities, wrong action shapes,
lookup/list substitution, false conflict/write claims, and replacement snapshots that alter any ref
other than the requested name. These checks constrain a corrupted authority result; they do not
select the production decision.

## Atomic write protocol

For set or delete, the host snapshots the database, evaluates GenesisCode, and attempts to replace
the database only if the locked current snapshot is byte-semantically equal to the evaluated
snapshot. A mismatch causes a fresh snapshot and a fresh authority evaluation. Exhausting the retry
bound fails closed. Rust never converts a changed snapshot into its own conflict or write verdict.

## Nonclaims

This contract does not claim H2 for `SD-REFS`, bulk sync/GPK reference authority, policy/evidence/
signature admission authority, registry reference authority, `R4.2.e` or SH-C closure, bootstrap
fixpoint, or release qualification.
