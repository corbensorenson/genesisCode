# Registry Policy (Local) (v0.2)

This document specifies a minimal, local “registry policy” format and verification behavior for GenesisCode v0.2.

The intent is to let CI and release tooling enforce supply-chain rules without requiring a network registry service.

## Policy File (`policy.toml`)

The policy file is TOML with:

- `version = 1` (required)
- `min_signatures = <int>` (optional, default `0`)
- `allowed_public_keys = ["<base64-32-bytes>", ...]` (optional, default `[]`)

If `min_signatures > 0`, `allowed_public_keys` MUST be non-empty.

## Signature Set File

The signature set file is a CoreForm term stored on disk (default `.genesis/signatures.gc`) containing:

- a vector of 64-hex signature artifact hashes, e.g. `["<hex>" "<hex2>" ...]`

The set is treated as order-insensitive; tooling SHOULD sort and deduplicate when writing.

## Verification (`genesis verify --policy`)

When invoked with `--policy <policy.toml>`, `genesis verify` MUST:

1. Perform standard package verification (module hashes, dependency hashes, acceptance artifact integrity).
2. If `min_signatures > 0`:
   - require an acceptance artifact hash (from `--acceptance` or `.genesis/last_acceptance`)
   - load the signature set file (from `--signatures` or `.genesis/signatures.gc`)
   - for each signature artifact hash in the set:
     - verify the artifact exists in `.genesis/store/` and its name matches its content hash
     - parse it as `genesis/acceptance-signature-v0.2`
     - require `:acceptance-h` to match the acceptance artifact hash
     - require `:pk` to be in `allowed_public_keys`
     - verify the Ed25519 signature over the message specified in `docs/spec/SIGNING.md`
3. Fail verification if the number of valid signatures is less than `min_signatures`.

