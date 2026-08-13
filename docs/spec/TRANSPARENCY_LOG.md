# Transparency Log (Local) (v0.2)

This document specifies a minimal, local transparency log mechanism for GenesisCode v0.2.

The transparency log is an append-only hash chain stored in the package’s evidence store, anchored by a head pointer file.

The artifact-loaded `core/security::signing-authority` defined by `SELFHOST_SIGNING_AUTHORITY_v0.1.md` exclusively constructs each production entry term and the canonical signature-set update. The host may load bounded prior state, store the exact returned term content-addressably, and write exact pointers; it MUST NOT reconstruct, repair, or replace authority output.

## Head Pointer

- `.genesis/transparency_head` contains the hex hash (BLAKE3) of the latest transparency entry artifact, or may be absent if no entries exist.
- A present malformed head is an error, not an empty log.

## Entry Artifact

Each transparency entry is stored as a content-addressed CoreForm term in `.genesis/store/<hex>`.

Entry schema:

```
{
  :kind "genesis/transparency-entry-v0.2"
  :prev-h b"...32 bytes..." | nil
  :package-artifact "<hex>"
  :acceptance-artifact "<hex>"
  :signature-artifact "<hex>"
  :signer-pk-b64 "<base64>"
}
```

Where `:prev-h` is the previous entry’s artifact hash bytes (or `nil` for the first entry).

## Append Behavior

`genesis sign` MUST append a transparency entry after successfully writing the signature artifact and updating the signature set.

## Verification

`genesis transparency-verify --pkg package.toml` MUST:

1. Read `.genesis/transparency_head` (if absent, treat as an empty log and succeed).
2. Walk the entry chain backwards following `:prev-h`.
3. For each entry:
   - verify the artifact exists and its name matches its content hash
   - verify `:kind` and `:prev-h` types and sizes
4. Report the number of traversed entries and fail on any mismatch.

The consumed traversal, schema, cycle, bound, and final admission verdict is exclusively produced by artifact-loaded `core/security::evidence-verify-authority` under `SELFHOST_EVIDENCE_VERIFY_AUTHORITY_v0.1.md`. Host traversal is an untrusted bounded observation proposal: GenesisCode rebinds it to the exact head and validates every link. Cycles, more than 16,384 entries, truncation, trailing entries, malformed present heads, and read failures fail closed.
