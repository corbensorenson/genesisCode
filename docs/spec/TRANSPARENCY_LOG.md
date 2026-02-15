# Transparency Log (Local) (v0.2)

This document specifies a minimal, local transparency log mechanism for GenesisCode v0.2.

The transparency log is an append-only hash chain stored in the package’s evidence store, anchored by a head pointer file.

## Head Pointer

- `.genesis/transparency_head` contains the hex hash (BLAKE3) of the latest transparency entry artifact, or may be absent if no entries exist.

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

