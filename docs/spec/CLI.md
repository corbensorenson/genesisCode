# Genesis CLI v0.2 (Exit Codes + JSON)

This document is normative for the `genesis` CLI behavior in GenesisCode v0.2.

## Global Flags

- `--json`: emit exactly one JSON object on stdout for all subcommands.
  - In JSON mode, stderr is reserved for unexpected process-level failures (it should usually be empty).
- `--step-limit <N>`: set the kernel evaluation step limit for commands that evaluate CoreForm.
  - Applies to: `eval`, `explain`, `run`, `replay`, `test`, `apply-patch`, `fmt --engine selfhost`, and `eval --engine selfhost`.
  - The step limit also applies to prelude initialization for that command.
  - Exception: for `fmt --engine selfhost` and `eval --engine selfhost`, toolchain bootstrap load is not charged against the step limit.
  - The v0.2 toolchain default is `5_000_000` steps.
- `--no-step-limit`: disable the kernel evaluation step limit (for trusted inputs only).
  - For package commands (`test`, `apply-patch`), `package.toml` may reject this via `[limits].allow_unlimited = false` (default).
- `--max-pair-cells <N>`: maximum total number of `pair/cons` cells allocated during evaluation.
- `--max-vec-len <N>`: maximum observed vector length (vector literals and `vec/push`).
- `--max-map-len <N>`: maximum observed map length (map literals, `map/put`, `map/merge`).
- `--max-bytes-len <N>`: maximum observed bytes length (bytes literals and `bytes/concat`).
- `--max-string-len <N>`: maximum observed string length in UTF-8 bytes (string literals and `str/concat`).

## Subcommands (Signing + Policy)

- `genesis fmt <file> [--check] [--engine rust|selfhost]`
  - `--engine rust` is the default.
  - `--engine selfhost` runs the self-hosted CoreForm toolchain inside the kernel and therefore honors `--step-limit/--no-step-limit`.
- `genesis eval <file> [--engine rust|selfhost] [--stage1-pipeline] [--stage1-gate] [--stage2-gate]`
  - `--engine rust` is the default.
  - `--engine selfhost` runs self-hosted parse+canonicalize in-kernel before evaluation.
  - `--stage1-pipeline` runs Stage-1 CoreForm->CoreForm transforms before evaluation.
  - `--stage1-gate` enforces `core/obligation::stage1-validation` for the eval input.
  - `--stage2-gate` enforces `core/obligation::translation-validation` only when the module is Stage-2 supported.
  - For Stage-2 gating, validation input is Stage-1 transformed CoreForm (matching package translation-validation flow), even when `--stage1-pipeline` is not requested.
- `genesis selfhost-artifact --out <file> [--min-stage2-supported-modules <N>] [--min-stage2-validated-modules <N>]`
  - emits a canonical self-host toolchain artifact used by `--engine selfhost` bootstrap.
  - runs Stage-1 + Stage-2 validation for each embedded selfhost module and records per-module gate metadata.
  - exits with code `30` when validation fails or configured Stage-2 minimum thresholds are not met.
- `genesis keygen --out <key.toml>`: generate an Ed25519 signing key (see `docs/spec/SIGNING.md`).
- `genesis sign --pkg <package.toml> --key <key.toml> [--acceptance <hex>] [--signatures <file>]`:
  - sign the acceptance artifact hash and write a signature artifact into the evidence store
  - update `.genesis/last_signature` and the signature set file (default `.genesis/signatures.gc`)
- `genesis verify --pkg <package.toml> [--policy <policy.toml>] [--signatures <file>]`:
  - when `--policy` is provided, enforce signature policy (see `docs/spec/REGISTRY_POLICY.md`)
- `genesis transparency-verify --pkg <package.toml>`: verify the local transparency log chain (see `docs/spec/TRANSPARENCY_LOG.md`)

## Exit Codes (Stable)

The CLI uses stable exit codes for automation and CI.

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Internal error (bug/unexpected) |
| 10 | Parse/canonicalization error (CoreForm, TOML, patch/log schema) |
| 11 | Formatting check failed (`fmt --check`) |
| 20 | Evaluation / kernel error |
| 30 | Obligation or checker failure (`test`, `typecheck`, `apply-patch` when obligations fail) |
| 40 | Replay mismatch (`replay`) |
| 41 | Capability denied during `run` (at least one log entry has decision `deny`) |
| 50 | Verification failed (`verify`) |
| 70 | I/O error |

Notes:
- `clap` argument/usage errors are handled by `clap` itself and may exit with its own code.
- `run` exits `41` if any capability request is denied, even if the program later handles the error as data.
- `replay` may require `--store <dir>` if the `.gclog` externalizes large responses into an artifact store.

## JSON Output (Stable Envelope)

All `--json` outputs use the same top-level envelope shape:

```json
{
  "ok": true,
  "kind": "genesis/<command>-v0.2",
  "data": { },
  "error": null
}
```

On failure:

```json
{
  "ok": false,
  "kind": "genesis/error-v0.2",
  "data": null,
  "error": {
    "code": "parse/coreform",
    "message": "…",
    "context": null
  }
}
```

`error.context` is optional and may be omitted or `null`.
