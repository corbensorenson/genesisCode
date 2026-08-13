# Self-hosted package lock model authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::lock-model-authority` binding is the production semantic authority for
the complete typed lock model consumed by `core/pkg-low::{info,lock,update,install,verify}`. It owns
supported lock versions, required workspace admission, policy defaults, requirement update-policy
and resolution-strategy normalization, tag-policy compatibility, locked-entry normalization, and
retention of every field needed by package resolution.

The public `core/pkg-low::load-lock` projection remains governed separately by
`SELFHOST_PKG_LOCK_READ_AUTHORITY_v0.1.md`. Rust retains capability and sandbox admission, bounded
file transport, UTF-8 validation, generic TOML syntax decoding, artifact bootstrap, strict result
decoding, typed reification, graph and semver mechanisms, persistence, and diagnostic sealing.
Generic TOML decoding is production-required, so this slice remains H0.

## Limits and bootstrap

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly` and fail closed if the artifact
or binding is absent. Input is capped at 4 MiB before UTF-8 or TOML decoding. The authority shares a
context bounded to 20,000,000 steps, 80,000,000 logical allocation units, 4 MiB strings or bytes,
and 65,536 map or vector entries.

## Request

Every request is the exact map:

```text
{
  :document <generic TOML term>
  :kind "genesis/pkg-lock-model-authority-request-v0.1"
  :op :read-model
  :v 1
}
```

The exact field set is checked by key, not cardinality alone. The generic TOML transport is defined
by the lock-read authority contract. Unknown root and nested TOML fields remain ignorable for v0.2
compatibility.

## Model

Success returns the exact model fields:

```text
[:artifacts :locked :policy :registries :requirements :version :workspace]
```

Each requirement is exactly
`[:registry :selector :strategy :tag-policy :update-policy]`. Strategy and update policy are closed
symbols. Each locked entry is exactly
`[:commit :environment-fingerprint :exports-hash :registry :resolved-ref :snapshot
:source-selector]`. An absent source selector becomes the empty string, while other optional fields
remain nil. This model therefore preserves resolution strategy, tag policy, source selector, and
environment fingerprint rather than reconstructing them in Rust.

## Result and contradiction checks

Every result is the exact map
`[:code :kind :message :model :ok :request-h :v]`. Kind and version are fixed, and request hash is the
canonical hash of the complete request. Rejection codes are closed to `core/pkg/bad-lock` and
`core/pkg/bad-authority-request`. Rust rejects open, mistyped, request-unbound, noncanonical, or
unknown-code results and converts the accepted closed model without selecting defaults.

Boundary failures are sealed as `core/pkg/authority-error`; user lock failures are sealed with the
authority's closed code. Production has no typed-parser fallback. `GenesisLock::load` is reachable
for these routes only under tests or the explicit `parity-oracle` feature.

## Nonclaims

This contract does not claim self-hosted TOML decoding, authority over `init`, `add`, `list`,
`load-lock`, `save-lock`, snapshot, publish, or bridge, graph-solving or semver mechanisms, registry
or workspace authority, H2 package resolution, `R4.2.e` or SH-C closure, bootstrap fixpoint, or
release qualification.
