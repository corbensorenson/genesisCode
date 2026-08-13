# Self-hosted package resolution identity authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::resolution-identity-authority` binding is the exclusive production
semantic producer for package requirement environment fingerprints created by
`core/pkg-low::lock`, `core/pkg-low::update`, and install-time dependency hydration. It owns the
closed seven-field requirement identity, canonical CoreForm term printing, the trailing newline,
and the raw BLAKE3 digest over those exact bytes.

This authority runs only after host-side resolution has selected a snapshot and optional commit.
It does not parse selectors, infer strategies, solve dependency graphs, select refs or tags,
access registries, read locks, validate artifacts, or decide update admission. Those decisions
remain outside this partial authority.

## Limits and bootstrap

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly` and fail closed if the artifact
or binding is absent. Each evaluation is bounded to 2,000,000 steps, 4,000,000 logical allocation
units, 64 KiB bytes, 16 KiB strings, and 64 map or vector entries. Rust may construct the typed
request, load the artifact, enforce those limits, decode the closed result, and seal a stable
`core/pkg/authority-error`; it must not recompute or substitute a production fingerprint.

## Request

Every request is the exact ten-field map:

```text
{
  :commit <string-or-nil>
  :kind "genesis/pkg-resolution-identity-request-v0.1"
  :op :requirement-fingerprint
  :registry <string-or-nil>
  :selector <string>
  :snapshot <string-or-nil>
  :strategy <:pinned|:track-ref|:tag-policy>
  :tag-policy <string-or-nil>
  :update-policy <:manual|:auto>
  :v 1
}
```

The authority rejects open maps, wrong kinds, operations, or versions, mistyped optional fields,
and values outside the closed strategy and update-policy enums.

## Identity

The authority constructs exactly this seven-field map:

```text
{
  :commit request.:commit
  :registry request.:registry
  :selector request.:selector
  :snapshot request.:snapshot
  :strategy request.:strategy
  :tag-policy request.:tag-policy
  :update-policy request.:update-policy
}
```

It prints the map using `selfhost/printer::print-term`, encodes that string as UTF-8, appends one
byte `0x0a`, applies raw BLAKE3, and returns lowercase hexadecimal. This intentionally preserves the
v0.2 lock compatibility identity; it is not the domain-separated canonical term hash.

## Result

Every result is the exact map:

```text
[:code :fingerprint :kind :message :ok :request-h :v]
```

`:kind` is `genesis/pkg-resolution-identity-result-v0.1`, `:v` is 1, and `:request-h` is the
canonical term hash of the complete request. Success contains nil code and message plus one
lowercase 64-hex fingerprint. Rejection contains a closed code/message pair and nil fingerprint.
The Rust adapter rejects open, mistyped, request-unbound, or malformed results before package state
is changed.

## Compatibility oracle

The former Rust fingerprint implementation is compiled only for tests or the explicit
`parity-oracle` feature. Production has no silent Rust hash fallback. Differential tests establish
v0.2 identity continuity but do not grant the Rust implementation production authority.

## Nonclaims

This contract does not claim selector or graph-resolution authority, registry authority, complete
package or lock authority, H2 package resolution, `R4.2.e` or SH-C closure, bootstrap fixpoint,
workspace authority, or release qualification.
