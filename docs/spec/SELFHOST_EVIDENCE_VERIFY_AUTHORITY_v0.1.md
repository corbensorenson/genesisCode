# Self-hosted Evidence Verification Authority v0.1

## Status and scope

This specification is the normative H2 contract for `SD-EVIDENCE-VERIFY`. The artifact-loaded binding `core/security::evidence-verify-authority`, composed from `selfhost/evidence_verify_package_v1.gc` and `selfhost/evidence_verify_authority_v1.gc`, is the sole production semantic producer for package integrity, acceptance and signature admission, registry signature policy, transparency traversal, and GenesisBench DSSE verification.

The host may reject malformed transport before invocation. It may read bounded files, decode TOML, JSON, base64, and CoreForm, compute BLAKE3 or SHA-256, execute Ed25519 verification, and transport those exact mechanism observations. Any content-store term MUST be decoded from the exact captured bytes whose computed hash accompanies that term; hashing one path read and decoding a second path read is forbidden. The host MUST transport declared and observed hashes separately, raw acceptance/signature/signature-set terms, decoded policy-key observations including failures, exact DSSE field inventories, and cryptographic mechanism outcomes. It MUST NOT compute schema admission, reference closure, store equality, policy-key admission, valid-signature count, signature threshold, or a substitute final verdict. Production routes fail closed when artifact admission, request evaluation, or result decoding fails.

## Closed protocol

Requests have kind `genesis/evidence-verification-authority-request-v0.1`, version `1`, one closed phase-specific field set, and one of these phases:

- `:package` receives bounded generic module/dependency observations, the optional raw acceptance term and hash, content-store observations, an optional decoded policy observation, the raw signature-set term, and raw signature observations. GenesisCode validates all closed acceptance, obligation, policy, key, signature-set, and signature schemas; derives and requires every acceptance reference; compares every required and observed store hash; admits keys; rejects duplicate or omitted signature observations; counts valid signatures; applies the minimum threshold; and computes the final conjunction. A failed transport or cryptographic mechanism cannot pass.
- `:transparency` receives the exact optional head hash, a distinct optional head-read error, and at most 16,384 ordered entry observations with required and computed hashes kept separate. GenesisCode binds traversal to the head, compares content identity, validates every exact six-field entry schema, follows `:prev-h`, rejects cycles, truncation, trailing entries, load failure, malformed links, and limit exhaustion, and computes the consumed entry count and verdict.
- `:dsse` receives expected and observed key and payload identities, sorted raw envelope/signature field-name inventories, decoded payload hash, signature cardinality, public-key mechanism outcome, and Ed25519 mechanism outcome. GenesisCode independently requires exact field inventories, cardinality, identities, and mechanism predicates and computes the consumed verdict.

Results have kind `genesis/evidence-verification-authority-result-v0.1`, version `1`, exactly `:code`, `:data`, `:kind`, `:message`, `:ok`, `:request-h`, and `:v`, and bind to the canonical hash of the exact request. Protocol rejection uses `:ok false`; a well-formed semantic denial uses `:ok true` with `:data/:verified false` and nonempty deterministic diagnostic codes. Rust rejects open, unbound, mistyped, or internally inconsistent results.

## Fail-closed state rules

A missing pointer is distinct from an unreadable or malformed pointer. In particular, malformed `.genesis/last_acceptance` and `.genesis/transparency_head` files MUST NOT be treated as absent. Transparency loading is finite even for adversarial graphs, and both host collection and GenesisCode authority reject cycles and over-limit chains.

The independent `tools/genesis-evidence-verifier` remains a separately implemented, read-only corroborator for release evidence envelopes. It does not authorize production command results, share implementation code with this authority, replace the authority after failure, or promote its own reports.

## Resource and custody bounds

Authority evaluation is artifact-only and bounded to 20,000,000 steps, 64,000,000 allocation units, 16 MiB byte/string payloads, 16,384 vector entries, and 32 map entries. Secret key bytes are never protocol inputs. Production entrypoints are `genesis` and `genesis_wasi`; no environment, debug, parity, error, timeout, recovery, or source fallback may produce an accepted result.

`scripts/lib/selfhost_evidence_verify_authority.py` independently verifies both source identities and artifact custody, exact protocol phases, result binding, production route reachability, absence of the retired host-semantic functions, single-read hash/term binding, malformed-pointer and cycle controls, standalone-verifier separation, truthful H2 ledger alignment, and mutations of store binding, acceptance-reference closure, key admission, signature-set closure, threshold enforcement, DSSE inventory transport, and route custody. The profile records that no host semantic oracle remains for this decision domain. It does not claim SH-C closure, H3/H4 bootstrap closure, release qualification, registry or benchmark publication readiness, package lock/resolution authority, or replacement of the independent verifier.
