# Self-hosted package lock operations authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::lock-ops-authority` binding is the production semantic authority for
direct `core/pkg-low::init`, `core/pkg-low::add`, and `core/pkg-low::list` operations and for the
conditional lock mutation performed by `core/pkg-low::bridge` when a string `:lock` is supplied. It
owns empty lock construction, workspace and optional policy/default-registry normalization,
requirement mutation and metadata normalization, complete internal lock normalization before
mutation or projection, canonical lock TOML bytes and identity for init/add/bridge-lock mutation,
the closed list projection, and bridge dependency artifact-key construction.

Rust remains the bounded mechanism for capability-policy admission, artifact bootstrap, generic TOML
syntax decoding, UTF-8 transport, sandboxed path resolution, bounded file reads, strict result
decoding, and atomic persistence of authorized bytes. The authority uses the pure deterministic
Prelude CoreForm printer/parser pair once to freeze its validated runtime-map model into data-map
form before calling the already custodied canonical lock writer. Bridge provenance, conversion,
snapshot, attestation, and commit object construction remain Rust-authoritative mechanisms outside
this contract. Graph solving, semver mechanics, registry transport, publish behavior, workspace
scaffolding, and other package operations also remain outside this authority. These residuals keep
`SD-PACKAGE-RESOLUTION` at H0.

## Bootstrap and limits

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly`. A missing artifact or binding,
evaluator failure, sealed `ERROR`, resource exhaustion, open or malformed result, wrong request
identity, unknown rejection code, or bytes/hash contradiction is a hard authority error. There is no
production Rust semantic fallback. For lock-bearing bridge requests, authority availability is
checked after payload-shape validation and before any bridge object is constructed or stored.

Each request is bounded to 20,000,000 evaluation steps, 80,000,000 logical allocation units, 4 MiB
bytes or string values, and 65,536 map or vector entries. Add, list, and bridge-lock file reads are
independently bounded to 4 MiB before TOML syntax decoding or authority evaluation. The runtime-map
freeze is inside those same limits and does not create a second package codec or grant host
authority.

## Request

Every request is the exact map:

```text
{
  :document <generic TOML term>
  :kind "genesis/pkg-lock-ops-authority-request-v0.1"
  :op :init | :add | :list | :bridge-lock
  :payload <original capability payload or closed bridge facts>
  :v 1
}
```

The request hash covers the complete envelope, generic document, and original payload. The envelope
field set is closed. The payload remains the capability ABI map: host-only `:lock` is ignored by
semantic decisions but remains request-bound.

For init, `:document` is nil. The authority requires a string `:workspace`, defaults a missing or
non-string optional `:policy` to `policy:default-v0.1`, and includes a `default` registry only when
`:registry-default` is a string. It constructs a version-2 lock with empty requirements, locked
entries, and artifacts, then serializes it canonically. This exactly preserves the prior direct-init
behavior while moving all non-path decisions out of Rust.

For add, the generic TOML document first passes the complete internal model authority's version,
workspace, policy, registry, requirement, locked-entry, and artifact validation. Name and selector
must be strings. Update policy defaults to manual. Strategy accepts the closed pinned, track-ref, and
tag-policy forms or is inferred from selector class. Tag-policy strategy defaults its tag policy to
`exact`; non-tag strategies clear it. The named requirement is replaced without altering unrelated
lock facts, and the complete model is serialized canonically.

For list, the same complete normalized model is projected into canonically ordered requirement and
locked vectors. Requirement entries have exact fields `[:name :registry :selector :strategy
:tag-policy :update-policy]`; locked entries have exact fields `[:commit
:environment-fingerprint :name :snapshot]`.

For bridge-lock, the generic TOML document passes the same complete normalization. The payload is
the exact map `[:attestation :commit :conversion-evidence :dep :provenance-root :registry
:snapshot]`. Dependency must be a non-empty string, registry must be nil or a string, and all five
artifact identities must be lowercase hex64. The authority writes a pinned/manual `commit:<hash>`
requirement, its corresponding locked entry, and four bridge artifact identities. Artifact keys use
the prior compatibility rule: retain ASCII alphanumeric, hyphen, and underscore dependency
characters, replace each other Unicode scalar with one underscore, then append an underscore and
the first eight lowercase BLAKE3 hex characters of the UTF-8 dependency name. This rule is owned by
GenesisCode and parity-tested against the retired Rust construction.

## Result

Every result is the exact map:

```text
[:bytes :code :kind :lock-h :message :ok :request-h :v :value]
```

`:kind` is `genesis/pkg-lock-ops-authority-result-v0.1`, `:v` is 1, and `:request-h` is the canonical
GenesisCode term hash of the complete request. Init/add/bridge-lock success has UTF-8 canonical TOML bytes, the
lowercase BLAKE3 hex64 of those exact bytes, nil value/code/message. List success has the closed value
projection and nil bytes/hash/code/message. Rejection uses only `core/pkg/bad-authority-request`,
`core/pkg/bad-lock`, or `core/pkg/bad-payload`, with nil bytes/hash/value.

The Rust decoder independently rejects open fields, request substitution, malformed operation/result
combinations, ill-typed or open list entries, unknown symbols or rejection codes, non-UTF-8 bytes,
invalid hashes, and bytes/hash substitution. It never chooses a semantic default or rewrites bytes.

## Host mechanism and parity boundary

Rust extracts the lock path only for sandbox mechanics. Add/list/bridge-lock read at most 4 MiB and
generically decode TOML; init/add/bridge-lock persist the exact authorized bytes atomically; list
only attaches the already authorized host lock-path string. Bridge-lock facts are hashes emitted by
the separately scoped bridge object mechanisms and are request-bound, strictly typed inputs rather
than host-selected lock semantics. The former init `GenesisLock::empty` and add/list
`GenesisLock::{load,set_requirement_with_metadata}` plus `to_toml_canonical` routes are compile-time
reachable only under `test` or `parity-oracle`; bridge-lock parity exists only in authority tests.

## Nonclaims

This contract does not claim a self-hosted TOML codec, graph or semver mechanism authority, bridge
object or conversion authority, registry transport or publish authority, workspace scaffolding
authority, H2 package resolution, `R4.2.e` or SH-C closure, bootstrap fixpoint, or release
qualification.
