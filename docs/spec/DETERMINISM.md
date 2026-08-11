# Determinism and Portability (v0.2)

This document summarizes v0.2 determinism goals and the concrete measures implemented in the toolchain to make artifacts portable across machines and operating systems.

## Content-Addressed Artifacts

All content-addressed artifacts are derived from canonical CoreForm bytes (or raw bytes for byte artifacts) and BLAKE3 hashing. Tooling must avoid incorporating machine-local paths or platform-specific formatting into hashed artifacts.

Key properties:
- Module hashes are computed from canonical printed CoreForm (newlines are `\n`).
- Package artifacts (`genesis/package-v0.2`) do not include filesystem paths (e.g. no `:manifest-path`).

## Effect Logs

Effect logs are deterministic for replay:
- Requests are hashed via `hash(op, payload-hash, continuation-hash)` (see `docs/spec/VALUE_EFFECT_HASH.md`).
- Responses (including errors) are captured in the log; replay validates request/response hashes.

To prevent log nondeterminism and path leakage:
- `.gclog` does not record filesystem paths (such as capability `base_dir`) in `:cap`.
- Filesystem requests use Unicode 17 NFC, base-relative paths with `/` separators. Absolute paths,
  backslashes, drive prefixes, empty components, `.`, and `..` components are rejected before host
  access; `.` alone names the capability root.
- IO error payloads record only NFC base-relative paths or the nondisclosing sentinels
  `<outside-base>` and `<invalid-path>`, never an absolute host prefix or OS error string.
- Filesystem response names must be valid UTF-8 and are normalized to NFC. Non-UTF-8 names and NFC
  collisions become explicit sealed errors rather than lossy or silently merged data.

## Path and Newline Normalization

- `package.toml` path fields (`modules[].path`, `dependencies[].path`, `caps_policy`) are required to:
  - be relative
  - use `/` separators
  - not contain `.` or `..`
- Canonical printing always uses `\n` line endings; inputs with `\r\n` parse equivalently and normalize on output.
- Core `Str` identity is exact UTF-8 scalar identity and is never implicitly normalized or
  locale-folded. Unicode 17 NFC and extended-grapheme operations are explicit APIs frozen by
  `docs/spec/TEXT_PATH_PROFILE_v0.1.md`.

## Known OS-Dependent Behavior

Some outcomes are inherently OS-dependent at *run time*, but do not compromise replay determinism:
- stable IO error kinds may differ when host permissions or filesystem behavior differ
- filesystem semantics differ (permissions, case enforcement, symlink handling, races, etc.)

These differences do not affect replay because replay consumes recorded responses rather than re-executing capabilities.
