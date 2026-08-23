# Self-hosted package lock model authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::lock-model-authority` binding is the production semantic authority for
the complete typed lock model consumed by `core/pkg-low::{info,lock,update,install,verify}`, the
lock-root projection consumed by `core/gc-low::{plan,run}`, the input and post-write lock models
for `genesis gcpm remove`, and the lock model consumed by `genesis gcpm env` environment planning
and optional hydration. It owns supported lock versions,
required workspace admission, policy defaults, requirement update-policy and resolution-strategy
normalization, tag-policy compatibility, locked-entry normalization, artifact-root normalization,
and retention of every field needed by package resolution and GC reachability planning.

The public `core/pkg-low::load-lock` projection remains governed separately by
`SELFHOST_PKG_LOCK_READ_AUTHORITY_v0.1.md`. Rust retains capability and sandbox admission, bounded
file transport, UTF-8 validation, generic TOML syntax decoding, artifact bootstrap, strict result
decoding, typed reification, graph and semver mechanisms, persistence, and diagnostic sealing.
Generic TOML decoding is production-required, so this slice remains H0. GC retains store traversal,
dead-set calculation, locking, quarantine, and deletion as bounded host mechanisms; this contract
authorizes only interpretation of lock-derived roots, not mutation of the store.

## Limits and bootstrap

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly` and fail closed if the artifact
or binding is absent. On Unix the host opens input nonblocking, then admits only an opened descriptor
whose metadata proves it is a regular file; the descriptor size is capped at 4 MiB before host
allocation, UTF-8 validation, or TOML decoding. This removes the path-check/open FIFO race while
retaining compatibility with a symlink that resolves to a regular file. Non-regular input fails
before reading, authority evaluation, hydration, or persistence. The authority shares a
context bounded to 20,000,000 steps, 80,000,000 logical allocation units, 4 MiB strings or bytes,
and 65,536 map or vector entries. GC rejects invalid, escaping, or otherwise inadmissible lock paths
as sealed `core/gc/bad-lock` errors before root planning, dead-set construction, or store mutation;
an admissible but absent lock remains an empty lock-root source.

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

Package boundary failures are sealed as `core/pkg/authority-error`; GC boundary failures use
`core/gc/lock-authority-error`, and user lock failures remain closed diagnostics. GC `plan` and `run`
fail as `core/gc/lock-authority-unavailable` before root or dead-set planning when lock roots are
enabled and the artifact authority is unavailable. Production has no typed-parser fallback for the
declared operations. `GenesisLock::load` is reachable for the package-low routes only under tests or
the explicit `parity-oracle` feature and is absent from the production GC lock-root and `gcpm remove`
routes. Remove re-authorizes the exact emitted bytes and requires their closed model to equal the
authorized mutation before persistence. Environment planning reads the lock once, renders canonical
bytes through the independently governed lock writer, re-authorizes those bytes against the input
model, and uses only that accepted model for hydration and environment observations. Invalid input or
a render/model contradiction fails before store hydration or environment persistence.

## Nonclaims

This contract does not claim self-hosted TOML decoding, authority over `init`, `add`, `list`,
`load-lock`, `save-lock`, snapshot, publish, or bridge, graph-solving or semver mechanisms, registry
or workspace authority, H2 package resolution, `R4.2.e` or SH-C closure, bootstrap fixpoint, or
release qualification.
