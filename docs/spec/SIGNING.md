# Acceptance Signing (v0.2)

This document specifies the normative behavior of package acceptance signing in GenesisCode v0.2.

## Key Format

`genesis keygen --out <path>` writes a TOML file with:

- `alg = "ed25519"`
- `sk_b64 = "..."` (base64-encoded 32-byte Ed25519 secret key seed)
- `pk_b64 = "..."` (base64-encoded 32-byte Ed25519 public key)

The tool MUST create the secret file without overwriting an existing path, MUST reject symlink and non-regular key inputs, MUST reject mismatched secret/public material, and on Unix MUST deny group and other permissions before secret bytes are written. Unix readers MUST open the final path with no-follow semantics and validate the opened descriptor's identity, regular-file type, and permissions before reading secret bytes; a pathname-only precheck is insufficient.

Production keypair admission, signed-message construction, signature artifact construction, signature-set canonicalization, and transparency-entry construction are owned by the artifact-loaded `core/security::signing-authority` contract in `SELFHOST_SIGNING_AUTHORITY_v0.1.md`. Rust performs only the bounded cryptographic, secret-custody, codec, storage, and pointer-write mechanisms named there and MUST NOT retain a production semantic fallback.

## Message To Sign

Signing is performed over the acceptance artifact hash with domain separation:

- message bytes: `b"GCv0.2\\0acceptance\\0" || acceptance_hash_bytes`

where `acceptance_hash_bytes` is the 32-byte value represented by the 64-hex acceptance artifact hash.

Ed25519 signatures are deterministic for a given message and key.

## Signature Artifact

`genesis sign --pkg package.toml --key <key.toml>` MUST:

1. Determine the acceptance artifact hash:
   - from `--acceptance <hex>` if provided, else from `.genesis/last_acceptance`.
2. Produce a signature artifact and store it in `.genesis/store/` as canonical CoreForm:

```
{
  :kind "genesis/acceptance-signature-v0.2"
  :alg "ed25519"
  :acceptance-h b"...32 bytes..."
  :pk b"...32 bytes..."
  :sig b"...64 bytes..."
}
```

3. Write `.genesis/last_signature` containing the signature artifact hash (one line).
4. Update the signature set file (default `.genesis/signatures.gc`) by inserting the signature artifact hash and writing a canonical CoreForm vector of 64-hex strings (sorted, deduplicated).

Malformed existing signature-set or transparency-head state MUST fail closed; it MUST NOT be interpreted as an empty set or absent head.

## Verification

Verification is policy-gated (see `docs/spec/REGISTRY_POLICY.md`) and every production route must consume `core/security::evidence-verify-authority` under `SELFHOST_EVIDENCE_VERIFY_AUTHORITY_v0.1.md`. The current package phase fails closed but is not yet H1/H2: Rust still computes acceptance/signature schema, key-policy admission, and cryptographic facts before GenesisCode performs the final conjunction and threshold comparison.
