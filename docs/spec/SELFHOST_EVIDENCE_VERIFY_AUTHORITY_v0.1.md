# Self-hosted Evidence Verification Authority v0.1

## Status and scope

This specification is the normative H0 inventory and partial cutover contract for `SD-EVIDENCE-VERIFY`. The artifact-loaded binding `core/security::evidence-verify-authority` is the production semantic producer for transparency-chain traversal/admission and the final GenesisBench DSSE conjunction. It is required on package-verification routes, but the package phase is not H1 or H2 while Rust still computes dependency integrity, acceptance/signature schema, store-integrity, and key-policy facts before invocation.

The host may reject malformed transport before invocation. It may read bounded files, decode TOML, JSON, base64, and CoreForm, compute BLAKE3 or SHA-256, execute Ed25519 verification, and transport those exact mechanism observations. Production routes MUST NOT return success, repair an observation, synthesize an alternative verdict, or bypass the GenesisCode result when artifact admission, request evaluation, or result decoding fails. This fail-closed route custody does not promote package semantics while callback facts can still choose the result.

## Closed protocol

Requests have kind `genesis/evidence-verification-authority-request-v0.1`, version `1`, one closed phase-specific field set, and one of these phases:

- `:package` receives bounded facts. Every fact has exactly `:class`, `:code`, `:mechanism-ok`, `:observed`, and `:required`. Admitted classes are `:presence`, `:identity`, `:schema`, `:crypto`, and `:at-least`. GenesisCode performs equality or threshold comparison and the final conjunction. A failed mechanism cannot pass regardless of transported values.
- `:transparency` receives the exact optional head hash, a distinct optional head-read error, and at most 16,384 ordered entry observations. GenesisCode binds traversal to the head, validates every exact six-field entry schema, follows `:prev-h`, rejects cycles, truncation, trailing entries, content-store failure, load failure, malformed links, and limit exhaustion, and computes the consumed entry count and verdict.
- `:dsse` receives the expected and observed key and payload identities, exact shape facts, decoded payload hash, signature cardinality, key-admission fact, and Ed25519 mechanism fact. GenesisCode requires every closed identity and mechanism predicate and computes the consumed verdict.

Results have kind `genesis/evidence-verification-authority-result-v0.1`, version `1`, exactly `:code`, `:data`, `:kind`, `:message`, `:ok`, `:request-h`, and `:v`, and bind to the canonical hash of the exact request. Protocol rejection uses `:ok false`; a well-formed semantic denial uses `:ok true` with `:data/:verified false` and nonempty deterministic diagnostic codes. Rust rejects open, unbound, mistyped, or internally inconsistent results.

## Fail-closed state rules

A missing pointer is distinct from an unreadable or malformed pointer. In particular, malformed `.genesis/last_acceptance` and `.genesis/transparency_head` files MUST NOT be treated as absent. Transparency loading is finite even for adversarial graphs, and both host collection and GenesisCode authority reject cycles and over-limit chains.

The independent `tools/genesis-evidence-verifier` remains a separately implemented, read-only corroborator for release evidence envelopes. It does not authorize production command results, share implementation code with this authority, replace the authority after failure, or promote its own reports.

## Resource and custody bounds

Authority evaluation is artifact-only and bounded to 20,000,000 steps, 64,000,000 allocation units, 16 MiB byte/string payloads, 16,384 vector entries, and 32 map entries. Secret key bytes are never protocol inputs. Production entrypoints are `genesis` and `genesis_wasi`; no environment, debug, parity, error, timeout, recovery, or source fallback may produce an accepted result.

`scripts/lib/selfhost_evidence_verify_authority.py` independently verifies source and artifact custody, exact protocol phases, result binding, production route reachability, absence of an unconditional success path, malformed-pointer and cycle controls, standalone-verifier separation, truthful H0 ledger alignment, and mutation rejection. The profile explicitly records its required host semantic oracle. It does not claim H1/H2 package authority, aggregate R4.2.d or SH-C closure, H3/H4 bootstrap closure, release qualification, registry or benchmark publication readiness, or replacement of the independent verifier.
