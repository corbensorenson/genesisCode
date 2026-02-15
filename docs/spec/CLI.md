# Genesis CLI v0.2 (Exit Codes + JSON)

This document is normative for the `genesis` CLI behavior in GenesisCode v0.2.

## Global Flags

- `--json`: emit exactly one JSON object on stdout for all subcommands.
  - In JSON mode, stderr is reserved for unexpected process-level failures (it should usually be empty).
- `--step-limit <N>`: set the kernel evaluation step limit for commands that evaluate CoreForm.
  - Applies to: `eval`, `explain`, `run`, `replay`, `test`, `apply-patch`.
  - The step limit also applies to prelude initialization for that command.
  - The v0.2 toolchain default is `5_000_000` steps.
- `--no-step-limit`: disable the kernel evaluation step limit (for trusted inputs only).
  - For package commands (`test`, `apply-patch`), `package.toml` may reject this via `[limits].allow_unlimited = false` (default).
- `--max-pair-cells <N>`: maximum total number of `pair/cons` cells allocated during evaluation.
- `--max-vec-len <N>`: maximum observed vector length (vector literals and `vec/push`).
- `--max-map-len <N>`: maximum observed map length (map literals, `map/put`, `map/merge`).
- `--max-bytes-len <N>`: maximum observed bytes length (bytes literals and `bytes/concat`).
- `--max-string-len <N>`: maximum observed string length in UTF-8 bytes (string literals and `str/concat`).

## Subcommands (Signing + Policy)

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
