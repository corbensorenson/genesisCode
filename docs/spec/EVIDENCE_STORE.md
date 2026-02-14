# Evidence Store v0.2 (Durability + Integrity)

GenesisCode v0.2 uses a content-addressed evidence store at:

`<package-root>/.genesis/store/<blake3-hex>`

This document is normative for the store’s integrity and best-effort durability semantics.

## Integrity

- Artifact filenames are the lowercase hex BLAKE3 digest of the artifact bytes.
- The store is write-once: a given `<hash>` path MUST never be overwritten.
- If an artifact path already exists, tooling MUST verify that the file contents hash to the filename.
  - If the hash does not match, the store is considered corrupted and tooling MUST fail.

## Concurrent Writers

Tooling MUST tolerate concurrent writers for the same artifact hash:

- Writers create a unique temp file under `.genesis/store/` with `create_new`.
- Writers write the full contents, `fsync` the temp file, then `rename` into the final `<hash>` path.
- If another writer wins the race (`rename` sees `AlreadyExists`), the loser discards its temp file and verifies the existing artifact contents.

## Durability (Best-Effort)

On platforms that support it, tooling performs best-effort `fsync` to reduce the risk of data loss after a crash:

- `fsync(temp_file)` after writing bytes.
- After the final `rename`, `fsync(.genesis/store/)` (directory fsync) on Unix platforms.

These steps do not guarantee durability across all filesystems and OS configurations, but they meaningfully reduce common crash-loss scenarios.

