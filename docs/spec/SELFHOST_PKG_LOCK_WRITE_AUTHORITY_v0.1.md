# Self-hosted package lock write authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::lock-write-authority` binding is the production semantic authority
for `core/pkg-low::save-lock`. It owns payload normalization, update-policy and resolution-strategy
normalization or inference, legacy-version upgrade, canonical lock TOML bytes, and the BLAKE3
`lock-h` of those exact bytes.

Rust remains the bounded mechanism for capability-policy admission, artifact bootstrap, strict result
decoding, sandboxed path resolution, directory policy, and atomic persistence. Lock parsing, package
graph resolution, registry behavior, workspace scaffolding, and all package operations other than the
`save-lock` serialization decision remain outside this authority. These residuals keep
`SD-PACKAGE-RESOLUTION` at H0.

## Bootstrap and limits

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly`. A missing artifact, missing
binding, evaluator failure, sealed `ERROR`, resource exhaustion, open or malformed result, wrong
request identity, or bytes/hash contradiction is a hard error. There is no production Rust semantic
fallback.

Each request is bounded to 20,000,000 evaluation steps, 80,000,000 logical allocation units, 4 MiB
bytes or string values, and 65,536 map or vector entries. The surrounding policy authority remains
bounded to 32,000,000 allocation units; this ceiling was ratcheted from 20,000,000 after the
90-module artifact measured 21,169,109 units for the package-init policy fixture.

## Request

Every request is the exact map:

```text
{
  :kind "genesis/pkg-lock-write-authority-request-v0.1"
  :op :write
  :payload <core/pkg-low::save-lock payload>
  :v 1
}
```

The payload accepts the lock fields `:version`, `:workspace`, `:policy`, `:registries`,
`:requirements`, `:locked`, and `:artifacts`; the host-only `:lock` path is ignored by semantic
serialization. Maps use canonical term order. Requirement and locked package names are quoted TOML
keys. Registry and artifact keys preserve the v0.2 writer's bare-key compatibility behavior.

Symbol strategies accept `:pinned`, `:track-ref`, and `:tag-policy`; string strategies accept only
their unprefixed forms. Missing strategies are inferred from the selector. Missing update policy is
`manual`, missing registry serializes as `default`, missing lock policy is
`policy:default-v0.1`, and absent or non-u64 integer versions normalize to version 2. Version 0, or
a legacy version containing v2 requirement facts, upgrades to version 2.

## Result

Every result is the exact map with fields:

```text
[:bytes :code :kind :lock-h :message :ok :request-h :v]
```

`:kind` is `genesis/pkg-lock-write-authority-result-v0.1`, `:v` is `1`, and `:request-h` is the
canonical GenesisCode term hash of the complete request. Success has UTF-8 TOML `:bytes`, a
lowercase BLAKE3 hex64 `:lock-h`, and nil code/message. Rejection has a closed code/message pair and
nil bytes/hash.

The Rust decoder rejects open fields, wrong request hashes, non-UTF-8 bytes, malformed success or
rejection shapes, invalid hashes, and bytes/hash substitution. These checks constrain a corrupted
authority result; they do not select serialization semantics.

## Persistence protocol

After authority success, Rust resolves the payload's `:lock` against the effective policy base,
rejects path escape, applies `create_dirs`, and atomically writes the exact authorized bytes. The
effect response returns the authorized `lock-h`; Rust does not reserialize or modify the bytes.

## Nonclaims

This contract does not claim H2 package resolution, lock-read parsing, graph resolution, registry or
workspace authority, `R4.2.e` or SH-C closure, bootstrap fixpoint, or release qualification.
