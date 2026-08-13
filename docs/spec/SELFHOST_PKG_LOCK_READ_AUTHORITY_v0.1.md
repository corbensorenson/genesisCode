# Self-hosted package lock read authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::lock-read-authority` binding is the production semantic authority for
the normalized result of `core/pkg-low::load-lock`. It owns supported lock versions, required
workspace admission, policy defaults, requirement update-policy defaults, resolution-strategy
normalization and tag-policy compatibility, optional field types, and the closed public lock term.

Rust retains capability and sandbox admission, bounded file reads, UTF-8 validation, generic TOML
decoding, artifact bootstrap, strict result decoding, and diagnostic sealing. Generic TOML decoding
is still a production-required host oracle, so this slice remains H0. The complete internal model
used by selected resolution routes is governed separately by
`SELFHOST_PKG_LOCK_MODEL_AUTHORITY_v0.1.md` and remains outside this public projection.

## Limits and bootstrap

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly` and fail closed if the artifact
or binding is absent. Lock input is capped at 4 MiB before UTF-8 or TOML decoding. Authority
evaluation is bounded to 20,000,000 steps, 80,000,000 logical allocation units, 4 MiB strings or
bytes, and 65,536 map or vector entries.

## Request and host document

Every authority request is the exact map:

```text
{
  :document <generic TOML term>
  :kind "genesis/pkg-lock-read-authority-request-v0.1"
  :op :read
  :v 1
}
```

Rust converts TOML tables to string-keyed maps, arrays to vectors, strings, integers, and booleans to
the corresponding terms, and floats or datetimes to closed tagged transport maps. This conversion
does not select governed lock fields. Unknown root or nested TOML fields remain ignorable for v0.2
compatibility.

GenesisCode accepts lock versions 1 and 2, requires a string workspace, defaults absent policy to
`policy:default-v0.1`, and defaults absent or unknown update policies to `:manual` as the legacy
reader did. Missing or unknown strategies are inferred from selectors. An effective `tag-policy`
strategy requires `refs/tags/*`, `ref:refs/tags/*`, or a non-empty `semver:` selector. Present
update-policy, strategy, tag-policy, and environment-fingerprint values must be strings, matching
the legacy typed decoder. An absent or empty source selector normalizes to public `nil`.

## Result

Every result is the exact map:

```text
[:code :kind :lock :message :ok :request-h :v]
```

`:kind` is `genesis/pkg-lock-read-authority-result-v0.1`, `:v` is 1, and `:request-h` is the canonical
term hash of the complete request. Success contains the closed normalized lock fields
`[:artifacts :locked :policy :registries :requirements :workspace]`; rejection contains a closed
code/message pair and nil lock. The Rust decoder rejects open, mistyped, request-unbound, or
substituted results before the effect response is formed.

Rust adds only the capability transport fields `:ok true` and the admitted `:lock` path to a
successful normalized result. It does not reinterpret the normalized lock.

## Compatibility oracle

The former typed `GenesisLock::load` route for this operation is compiled only for tests or the
explicit `parity-oracle` feature. Production has no silent typed-parser fallback. The generic Rust
TOML codec remains reachable and must be removed or independently refined before H2 can be claimed.

## Nonclaims

This contract does not claim self-hosted TOML decoding, H2 package resolution, internal lock-model
authority, graph solving, update/install/registry/workspace authority, `R4.2.e` or SH-C closure,
bootstrap fixpoint, or release qualification.
