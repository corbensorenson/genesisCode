> Bundle Entry: `docs/spec/GCPM_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# GCPM Build Targets v0.1

This document defines deterministic deployment bundle output for:

- `genesis gcpm build --pkg <package.toml> --target <web|desktop|service> [--out-dir <path>]`

## Goals

- Provide a first-class, machine-readable deployment build target for `gcpm`.
- Keep bundle output reproducible and immutable across reruns with identical inputs.
- Emit explicit provenance metadata suitable for AI-agent planning and CI attestation.

## Inputs

- `package.toml` path (`--pkg`, default `package.toml`)
- `target` contract token (`web|desktop|service`)
- output root (`--out-dir`, default `.genesis/build`)

## Bundle Layout

For `target=<target>` and manifest hash `<bundle-h>`:

- `<out-dir>/<target>/<bundle-h>/build_manifest.gc`
- `<out-dir>/<target>/<bundle-h>/provenance.gc`
- `<out-dir>/<target>/<bundle-h>/package.toml`
- `<out-dir>/<target>/<bundle-h>/package_artifact.txt`

`<bundle-h>` is BLAKE3 over canonical `build_manifest.gc` bytes.

## Build Manifest Schema

`build_manifest.gc` is a CoreForm map:

- `:type = :gcpm/build-manifest`
- `:v = 1`
- `:target = "<web|desktop|service>"`
- `:target-profile` map:
  - `:runtime` (for example `wasm32-unknown-unknown` or `native`)
  - `:host-profile` (`browser|desktop|headless`)
  - `:artifact-format` (bundle encoding contract token)
- `:package` map:
  - `:name`
  - `:version`
  - `:package-h` (BLAKE3 over `package.toml` bytes)
  - `:package-artifact` (deterministic package artifact hash)

## Provenance Schema

`provenance.gc` is a CoreForm map:

- `:type = :gcpm/build-provenance`
- `:v = 1`
- `:target`
- `:bundle-h`
- `:build-manifest-h` (same hash as `:bundle-h`)
- `:package-artifact`
- `:generated-by` (CLI version string)

## Immutability Rule

- Existing identical artifacts are accepted (idempotent reruns).
- Existing artifacts with byte differences hard-fail; bundle files are immutable once materialized.

## JSON Kind

- `gcpm build` emits `kind = "genesis/pkg-build-v0.1"`.

