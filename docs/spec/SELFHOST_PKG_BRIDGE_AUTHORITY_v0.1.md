# Self-hosted package bridge authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::bridge-authority` binding is the exclusive production semantic
authority for the object graph created by `core/pkg-low::bridge`. It owns the canonical external
provenance, conversion-data, equivalence-evidence, empty patch, package snapshot, unsigned commit,
attestation, and final commit terms; the exact printed bytes and raw BLAKE3 identity of each stored
object; the VCS commit signing hash and domain-separated attestation message; and the closed,
request-bound two-phase plan/finalize protocol.

Rust remains a bounded mechanism for capability admission, payload and UTF-8 transport, artifact
bootstrap, exact authorized-byte storage, invocation of `core/crypto::sign`, verification of the
returned Ed25519 signature against the supplied public key, strict result contradiction checking,
and optional lock persistence through the separately custodied package lock-operations authority.
It may reject a contradiction but may not construct, normalize, repair, or substitute a bridge
object. Graph solving, semver, registry transport and policy, workspace scaffolding, and publish
decisions remain outside this contract, so `SD-PACKAGE-RESOLUTION` remains H0.

## Bootstrap and limits

Production evaluation MUST use `SelfhostBootstrapMode::ArtifactOnly`. A missing artifact or binding,
evaluator failure, sealed `ERROR`, resource exhaustion, open or malformed result, wrong request or
plan identity, unknown rejection code, object term/bytes/hash contradiction, VCS signing
contradiction, false cryptographic mechanism fact, or invalid final attestation/commit is a hard
authority error or a closed sealed bridge rejection. There is no production Rust semantic fallback.

The authority shares the package authority evaluator limits: 20,000,000 steps, 80,000,000 logical
allocation units, 4 MiB bytes or string values, and 65,536 map or vector entries. Authority
availability is checked after closed payload-shape validation and lock/dependency pairing, but
before any object is stored or any signing capability is invoked, for every bridge request whether
or not it mutates a lock.

## Request

Every request is the exact map:

```text
{
  :facts {
    :ecosystem <nonempty string>
    :name <nonempty string>
    :source <nonempty string>
    :source-hash <lowercase hex64>
    :version <nonempty string>
  }
  :kind "genesis/pkg-bridge-authority-request-v0.1"
  :mechanism nil | {
    :plan-h <lowercase hex64>
    :public-key <32 bytes>
    :signature <64 bytes>
    :signature-valid <boolean>
  }
  :op :plan | :finalize
  :v 1
}
```

The request hash is the canonical GenesisCode term hash of the complete envelope. `:plan` requires
nil mechanism data. `:finalize` recomputes the complete plan from the request facts and accepts only
an exact mechanism map whose plan identity matches that recomputation. The host supplies the
cryptographic observation `:signature-valid`; GenesisCode decides whether that observation admits
attestation construction. A false observation is rejected and cannot be promoted by the host.

## Plan result

The successful plan value is the exact map `[:plan :plan-h]`. `:plan-h` is the canonical term hash
of the complete plan. The plan contains exact object envelopes `[:bytes :h :term]` for provenance,
conversion data, conversion evidence, patch, and snapshot, plus the unsigned commit, 32-byte
`:signing-h`, and byte-vector `:sign-message`. Every object envelope binds its canonical printed
term bytes to their raw BLAKE3 lowercase hex identity.

The unsigned commit signing hash is:

```text
BLAKE3("GCv0.2\0" || "vcs\0commit-signing-hash\0" || canonical_commit_bytes)
```

The Ed25519 message is:

```text
"GCv0.2\0" || "vcs\0commit-sign\0" || signing_hash
```

This corrects the retired bridge mechanism, which sent the bare 32-byte signing hash to the signing
capability even though VCS attestation verification requires the domain-separated message. The
authority owns the corrected message; Rust independently recomputes it only to reject contradiction.

## Final result

The successful finalize value is the exact map `[:attestation :commit :plan-h]`. The attestation
binds algorithm `ed25519`, role `:mirror-converter`, the supplied public key and signature, and the
recomputed signing hash. The final commit differs from the unsigned commit only by the single
attestation object identity. Rust independently verifies the attestation and requires the final
commit to retain the plan signing hash and exactly that one attestation before storing either object.

Every outer result is the exact map:

```text
[:code :kind :message :ok :request-h :v :value]
```

`:kind` is `genesis/pkg-bridge-authority-result-v0.1`, `:v` is 1, and `:request-h` binds the complete
request. Success has nil code/message. Rejection has nil value and only
`core/pkg/bad-authority-request`, `core/pkg/bad-payload`, or `core/pkg/bridge-signature`.

## Storage and failure ordering

Rust stores only authority-supplied bytes and rejects any store identity that differs from the
authority identity. Plan objects are stored in dependency order before signing. Attestation and
commit objects are stored only after host signature verification and authority finalization both
succeed. Optional lock mutation occurs only after final commit storage and delegates all lock
semantics to `core/pkg::lock-ops-authority`.

## Nonclaims

This contract does not claim payload-transport or cryptographic-mechanism authority, a self-hosted
TOML codec, graph or semver mechanism authority, registry transport or publish authority, workspace
authority, H2 package resolution, `R4.2.e` or SH-C closure, bootstrap fixpoint, or release
qualification.
