# Acceptance Signing (v0.2)

This document specifies the normative behavior of package acceptance signing in GenesisCode v0.2.

## Key Format

`genesis keygen --out <path>` writes a TOML file with:

- `alg = "ed25519"`
- `sk_b64 = "..."` (base64-encoded 32-byte Ed25519 secret key seed)
- `pk_b64 = "..."` (base64-encoded 32-byte Ed25519 public key)

The tool SHOULD set restrictive file permissions on Unix (best-effort).

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

## Verification

Verification is policy-gated (see `docs/spec/REGISTRY_POLICY.md`).

