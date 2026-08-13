# Self-host signing authority v0.1

Status: normative H2 contract for `SD-SIGNING` under R4.2.d.

## Authority

`core/security::signing-authority`, loaded from the exact artifact-only self-host toolchain, owns every production signing decision and every byte or semantic artifact covered by `SD-SIGNING`. This includes package key-generation admission, acceptance signature message and artifact construction, signature-set ordering and deduplication, transparency entry construction, and GenesisBench DSSE PAE and envelope facts. Production hosts fail closed when the binding is absent, evaluation fails, a request or result is open or malformed, the result is not bound to the exact request, or a cryptographic mechanism reports failure.

No production Rust implementation may construct, repair, approve, or override these semantic results. Legacy constructors may exist only behind the explicit parity-oracle feature. Evidence, signature, transparency, package, and run *verification* remain the separate `SD-EVIDENCE-VERIFY` migration and are not claimed here.

## Host mechanisms

Rust remains responsible for bounded mechanisms that cannot run in the pure kernel:

- obtain entropy from the operating-system CSPRNG;
- derive Ed25519 public keys and execute Ed25519 signing and verification;
- execute SHA-256 and content-addressed BLAKE3 storage;
- decode and encode fixed Base64, TOML, CoreForm, and JSON transports;
- enforce regular, non-symlinked, owner-only secret-key files and create-only secret writes;
- load bounded package state, persist exact authority-produced terms, and update state pointers.

Mechanism outcomes are explicit closed request facts. A false keypair or signature fact is rejected by GenesisCode. Secret key material never enters the authority request or result. The host may reject a mechanism failure but cannot infer a semantic signing result.

## Protocol

Every request has kind `genesis/signing-authority-request-v0.1`, version `1`, a symbol `:phase`, and the exact phase field set:

- `:keygen`: `:public-key` (32 bytes) and `:keypair-valid` (bool). It admits only a valid Ed25519 pair and returns the fixed algorithm and public key.
- `:acceptance-plan`: `:acceptance-h` (32 bytes). It returns exactly `b"GCv0.2\x00acceptance\x00" || acceptance-h`.
- `:acceptance-finalize`: acceptance hash, 32-byte public key, 64-byte signature, and `:signature-valid`. It emits the canonical `genesis/acceptance-signature-v0.2` term only after mechanism acceptance.
- `:commit`: canonical package, acceptance, and signature artifact hashes; public-key Base64; a closed prior signature vector; and an optional 32-byte prior transparency head. It returns a strictly sorted, unique signature set and the exact `genesis/transparency-entry-v0.2` term.
- `:dsse-plan`: bounded payload bytes and nonempty payload type. It returns exact DSSE v1 pre-authentication bytes using UTF-8 byte lengths.
- `:dsse-finalize`: payload and payload type, SHA-256 payload/key facts, public key, signature, and `:signature-valid`. It emits all facts for `genesis/genesisbench-dsse-signature-v0.1` only after mechanism acceptance.

Results have kind `genesis/signing-authority-result-v0.1`, version `1`, and exactly `:code`, `:data`, `:kind`, `:message`, `:ok`, `:request-h`, and `:v`. `:request-h` is the lowercase canonical CoreForm hash of the exact request. Acceptance requires nil code/message and closed phase data. Rejection requires nil data and nonempty code/message.

## Custody and bounds

Production uses artifact-only bootstrap. Each authority evaluator receives 20,000,000 post-bootstrap steps and 64,000,000 logical allocation units, with a 16 MiB payload ceiling plus fixed protocol overhead. Bootstrap counters reset before request evaluation. Inputs and outputs are closed, artifact hashes are canonical lowercase 32-byte hexadecimal, secret files are create-only and owner-only on Unix, mismatched keypairs are rejected, and malformed signature-set or transparency-head state is never treated as empty.

Package signing stores the immutable signature and transparency terms before publishing their exact state pointers. GenesisBench signing permits only the separately specified bounded payload types and canonical payload transport. These host transports do not gain semantic authority.

## Verification and nonclaims

`scripts/lib/selfhost_signing_authority.py` independently verifies source/artifact custody, all six protocol phases, package and GenesisBench production routes, closed result decoding, artifact-only bounds, secret custody, parity-only legacy constructors, adversarial controls, and semantic-ownership ledger alignment.

This profile does not claim `SD-EVIDENCE-VERIFY`, H3/H4, bootstrap fixpoint, aggregate R4.2.d or SH-C closure, release qualification, registry trust, or benchmark publication readiness.
