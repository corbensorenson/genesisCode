# Self-hosted package publish authority v0.1

Status: normative implementation contract for `R4.2.e`; the production authority switch and
independent evidence are pending.

## Purpose and authority

Artifact-loaded `core/pkg::publish-authority` MUST become the exclusive production semantic
authority for `core/pkg-low::publish`. It owns policy decoding, frozen-ref admission, class
selection, commit and evidence schema admission, required-obligation and evidence-kind closure,
requirements-trace and tool-qualification admission, signature-policy and role admission,
publication provenance, and the exact `core/sync::push` plan. A GenesisCode rejection is final.

Rust MAY retain only bounded payload, ref, and artifact transport; artifact-only authority
bootstrap and evaluation; canonical term/byte/hash contradiction checks; Ed25519 verification;
capability, timeout, and byte-budget enforcement; and exact execution of an authority-returned
sync plan. Rust MUST NOT parse a policy into a decision structure, select a policy class, validate
commit/evidence semantics, count accepted signers or roles, construct provenance, or retain a
production semantic fallback.

This contract does not switch production authority by itself. Until the source module, strict
adapter, authentic route, profile, independent verifier, and published toolchain artifact pass
their required controls, the current Rust route remains truthfully host-authoritative and
`SD-PACKAGE-RESOLUTION` remains H0.

## Closed three-phase protocol

The protocol has `:inspect`, `:prepare`, and `:finalize` phases. Every phase recomputes all prior
semantic state from raw inputs rather than trusting a host projection. The host may fetch only the
objects requested by `:inspect`, may verify only the cryptographic requests returned by
`:prepare`, and may perform a remote mutation only after an accepted `:finalize` result.

Every request is the exact map:

```text
{
  :facts {
    :commit <raw :vcs/commit term>
    :commit-h <lowercase hex64>
    :depth <nonnegative integer>
    :expected-old nil | <lowercase hex64>
    :policy <raw :vcs/policy term>
    :policy-h <lowercase hex64>
    :ref <nonempty portable VCS ref string>
    :remote <nonempty string>
  }
  :kind "genesis/pkg-publish-authority-request-v0.1"
  :mechanism nil | <phase-specific exact mechanism map>
  :phase :inspect | :prepare | :finalize
  :v 1
}
```

The authority recomputes the raw BLAKE3 identity of canonical policy and commit bytes and requires
the supplied identities to agree. The complete request hash is the canonical GenesisCode term
hash of the request envelope. A malformed, open, mismatched, resource-exhausted, or opaque request
is an authority failure or a closed rejection; it never authorizes host recovery.

### Inspect

`:inspect` requires `:mechanism nil`. GenesisCode validates the complete policy and commit,
rejects a frozen or unmatched ref, deterministically selects `:tags`, then `:main`, then `:dev`,
and validates required commit obligations before returning:

```text
{
  :attestation-hashes [<lowercase hex64> ...]
  :evidence-hashes [<lowercase hex64> ...]
  :inspect-h <lowercase hex64>
}
```

`:inspect-h` is the canonical term hash of the complete inspect value with `:inspect-h` omitted.
Vectors preserve the exact order in the commit. Duplicate hashes remain visible and MUST NOT be
silently normalized by the host. Rust may now load exactly those requested artifacts. It supplies
each as `{:bytes <bytes> :h <hash> :term <raw term>}` so GenesisCode and Rust can independently
reject term/bytes/hash substitution.

### Prepare

`:prepare` uses the same facts and this exact mechanism map:

```text
{
  :attestations [{:bytes <bytes> :h <hash> :term <raw attestation>} ...]
  :evidence [{:bytes <bytes> :h <hash> :term <raw evidence>} ...]
  :inspect-h <lowercase hex64>
}
```

The authority recomputes inspect, requires exact ordered object coverage, validates every object
envelope and evidence term, derives the complete required evidence-kind set, and validates every
required requirements-trace and tool-qualification artifact against the exact commit, snapshot,
policy, obligation, and observed-kind context.

If signatures are required, GenesisCode computes the VCS signing hash by replacing the commit's
`:attestations` with `[]` and hashing:

```text
BLAKE3("GCv0.2\0" || "vcs\0commit-signing-hash\0" || canonical_unsigned_commit_bytes)
```

It then returns one ordered cryptographic request per attestation:

```text
{
  :allowed-public-keys [<base64 string> ...]
  :alg <string>
  :attestation-h <lowercase hex64>
  :pk <32 bytes>
  :request-h <lowercase hex64>
  :sig <64 bytes>
  :sign-message <bytes>
  :signing-h <32 bytes>
}
```

`:sign-message` is exactly `"GCv0.2\0" || "vcs\0commit-sign\0" || signing-h`.
`:request-h` binds the complete cryptographic request with `:request-h` omitted. The successful
prepare value is exactly `{:crypto-requests [...] :prepare-h <hex64>}`; `:prepare-h` binds the
complete value with that field omitted. When signatures are not required, the request vector is
empty even if the commit carries attestations.

Rust decodes each allowed key, verifies key length and validity, requires `ed25519`, checks all
signing-domain fields for contradiction, and performs strict Ed25519 verification. This is a
mechanism observation, not a policy decision. It returns exactly one ordered fact per request:

```text
{:request-h <lowercase hex64> :signature-valid <boolean>}
```

Rust MUST report false rather than omit an invalid key, disallowed signer, malformed signature,
unsupported algorithm, or failed signature. It MUST reject duplicate, missing, reordered, open,
or request-unbound facts before finalization.

### Finalize

`:finalize` uses the same facts and this exact mechanism map:

```text
{
  :attestations [<same ordered object envelopes> ...]
  :crypto-facts [<same ordered facts> ...]
  :evidence [<same ordered object envelopes> ...]
  :inspect-h <lowercase hex64>
  :prepare-h <lowercase hex64>
}
```

The authority recomputes inspect and prepare, requires exact fact coverage, and alone decides:

- distinct accepted signer count against `:min-signatures`;
- normalized required role presence;
- per-role distinct signer minima;
- pairwise role independence;
- the final publication provenance; and
- the exact remote, roots, depth, and compare-and-set ref update sent to `core/sync::push`.

The successful value is exactly:

```text
{
  :commit <lowercase hex64>
  :provenance {
    :attestations [<lowercase hex64> ...]
    :base nil | <lowercase hex64>
    :evidence [<lowercase hex64> ...]
    :obligations [<string-or-symbol> ...]
    :parents [<lowercase hex64> ...]
    :patch <lowercase hex64>
    :result <lowercase hex64>
  }
  :ref <ref string>
  :sync {
    :depth <nonnegative integer, omitted when zero>
    :remote <remote string>
    :roots [<commit hash> <policy hash>]
    :set-refs [{:expected-old <hash, optional> :hash <commit hash> :name <ref> :policy <policy hash>}]
  }
}
```

Rust strictly decodes this closed value, requires it to agree with the bound facts, and invokes
`core/sync::push` without semantic repair. It may append the exact authority-returned `:commit`,
`:ref`, and `:provenance` fields to a successful sync mechanism result. No store, ref, network, or
remote mutation is permitted after a rejection or before final acceptance.

## Normative policy semantics

The accepted v1 policy term remains `:type :vcs/policy`, `:v 1`, optional string-or-nil `:name`,
optional `:refs {:frozen-prefixes [string ...]}`, and required `:classes` map. Only `:tags`,
`:main`, and `:dev` class keys are semantic. A present class is a map with a nonempty string
`:patterns` vector and the existing optional fields `:exclude`, `:required-obligations`,
`:required-evidence-kinds`, `:obligation-evidence-kinds`, `:require-signatures`,
`:min-signatures`, `:allowed-public-keys`, `:required-attestation-roles`,
`:role-min-signatures`, and `:independent-role-pairs`. Wrong types, negative minima, empty required
roles, same-role independence pairs, role constraints without signature enforcement, and positive
signature minima without allowed keys fail closed.

Evidence kinds and roles are ASCII-trimmed and receive one leading `:` when absent. Deduplicated
semantic sets use canonical term order; commit vectors and object requests preserve source order.
Obligation-specific evidence requirements apply only when the exact obligation is on the commit.

Policy patterns use the v1 portable-ref glob grammar over UTF-8 bytes: UTF-8 literal byte
sequences, backslash escaping, `?` for exactly one byte, `*` or `**` for zero or more bytes
including `/`, bracket byte classes and ranges with leading `!` negation, and brace expansion.
A component-boundary `**/` additionally matches zero complete path components, so
`refs/**/main` matches `refs/main`. Brace groups may contain nested or singleton alternatives;
empty alternatives are ignored when any nonempty alternative exists, while an all-empty group
matches the empty byte sequence. Matching is case-sensitive and anchored to the complete ref.
Invalid escapes, unclosed classes or braces, and descending byte ranges make the policy invalid.
This freezes the existing default `globset` behavior as a language-level contract; production
GenesisCode MUST parse and match it directly. A Rust glob result is not an admissible mechanism
fact.

Frozen prefixes use exact Unicode scalar prefix comparison. Excludes override patterns within a
class. Class precedence is always tags, main, dev and is not map iteration order.

## Evidence semantics

Every evidence term must be `:type :vcs/evidence`, `:v 1`, have a symbol `:kind`, and have valid
optional content-addressed `:inputs`, `:outputs`, and string-hash `:data` reachability pointers.
Inline `:data` remains permitted and does not become a reachability pointer.

Requirements-trace evidence must be `:status :verified`, bind a valid graph hash and the exact
release snapshot plus optional exact commit/policy, contain at least one requirement, admit only
`:system`, `:hlr`, or `:llr` levels, reject empty IDs and links, and reject dangling obligation or
evidence-kind links. Module links require a nonempty path and nonempty export vector.

Tool-qualification evidence must be `:status :qualified`, bind the exact release snapshot plus
optional exact commit/policy, contain nonempty requirement IDs and tools with valid BLAKE3 hashes,
and contain nonempty qualification tests. Every test must have nonempty ID, run ID, runner, and
profile; valid artifact, manifest, and snapshot hashes; the exact release snapshot and optional
policy; and result `:pass`.

## Closed result and diagnostic inventory

Every phase result is the exact outer map `[:code :kind :message :ok :request-h :v :value]`.
`:kind` is `genesis/pkg-publish-authority-result-v0.1`, `:v` is 1, and `:request-h` binds the
complete phase request. Success has nil code/message. Rejection has nil value and one of:

- `core/pkg/bad-authority-request`
- `core/pkg/bad-payload`
- `core/pkg/bad-policy`
- `core/pkg/ref-frozen`
- `core/pkg/no-policy-class`
- `core/pkg/bad-commit`
- `core/pkg/missing-obligation`
- `core/pkg/missing-evidence`
- `core/pkg/bad-evidence`
- `core/pkg/missing-evidence-kind`
- `core/pkg/missing-requirements-trace`
- `core/pkg/invalid-requirements-trace`
- `core/pkg/missing-tool-qualification`
- `core/pkg/invalid-tool-qualification`
- `core/pkg/bad-attestation`
- `core/pkg/missing-signatures`
- `core/pkg/missing-attestation-role`
- `core/pkg/missing-attestation-role-signatures`
- `core/pkg/role-independence-violation`

Artifact absence, ref transport failure, malformed authority output, evaluator failure, sealed
`ERROR`, resource exhaustion, term/byte/hash contradiction, cryptographic contradiction, and sync
mechanism failure remain distinct mechanism errors and MUST NOT be translated into a semantic
admission.

## Failure ordering and verification

The production route MUST establish artifact-only authority availability before local ref lookup,
artifact reads, cryptographic work, or sync transport. It may validate the closed user payload
shape first only if doing so has no filesystem or network side effect. Each phase must complete and
be strictly decoded before the next phase's side effects begin. Final acceptance must precede the
first remote mutation.

The implementation transaction MUST add focused authority tests for every policy class and
diagnostic family, advanced glob parity, malformed/open/substituted envelopes, duplicate and
reordered objects and facts, false cryptographic facts, signer deduplication, role minima and
independence, requirements-trace and tool-qualification binding, and exact sync/provenance output.
Authentic effect tests MUST prove missing authority and every semantic rejection occur before ref,
store, crypto, or remote mutation as applicable. An independent verifier MUST mutate protocol,
source, artifact, route, decoder, ordering, test, ledger, identity, and nonclaim controls.

## Nonclaims

This contract does not claim the production switch, artifact publication, independent evidence,
registry transport authority, ref transport authority, cryptographic mechanism authority, graph or
semver authority, workspace authority, H2 package resolution, `R4.2.e` closure, SH-C closure,
bootstrap fixpoint, or release qualification.
