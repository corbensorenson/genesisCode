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
- IO error payloads record base-relative paths using `/` separators rather than absolute paths.

## Path and Newline Normalization

- `package.toml` path fields (`modules[].path`, `dependencies[].path`, `caps_policy`) are required to:
  - be relative
  - use `/` separators
  - not contain `.` or `..`
- Canonical printing always uses `\n` line endings; inputs with `\r\n` parse equivalently and normalize on output.

## Known OS-Dependent Behavior

Some values are inherently OS-dependent at *run time*, but do not compromise replay determinism:
- capability error messages (`io::Error` strings) may differ across OS versions
- filesystem semantics differ (permissions, symlink handling, etc.)

These differences do not affect replay because replay consumes recorded responses rather than re-executing capabilities.

