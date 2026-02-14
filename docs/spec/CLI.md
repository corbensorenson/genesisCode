# Genesis CLI v0.2 (Exit Codes + JSON)

This document is normative for the `genesis` CLI behavior in GenesisCode v0.2.

## Global Flags

- `--json`: emit exactly one JSON object on stdout for all subcommands.
  - In JSON mode, stderr is reserved for unexpected process-level failures (it should usually be empty).

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

