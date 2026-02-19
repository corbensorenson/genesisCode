# Genesis CLI v0.2 (Exit Codes + JSON)

This document is normative for the `genesis` CLI behavior in GenesisCode v0.2.

## Global Flags

- `--json`: emit exactly one JSON object on stdout for all subcommands.
  - In JSON mode, stderr is reserved for unexpected process-level failures (it should usually be empty).
- `--step-limit <N>`: set the kernel evaluation step limit for commands that evaluate CoreForm.
  - Applies to: `eval`, `explain`, `run`, `replay`, `test`, `apply-patch`, `semantic-edit index`, `fmt --engine selfhost`, and `eval --engine selfhost`.
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
- `--selfhost-only`: enforce strict selfhost frontend mode.
  - Also enabled when `GENESIS_SELFHOST_ONLY=1|true|yes|on`.
  - In this mode:
    - commands with `--engine` must use `--engine selfhost`
    - `--selfhost-bootstrap` must be `artifact-only`
    - commands not yet routed through selfhost frontend return exit code `50`.
  - Current routed set:
    - native: `fmt`, `eval`, `explain`, `run`, `replay`, `optimize`, `typecheck`, `test`, `apply-patch`, `semantic-edit`, `pack`, `store/*`, `refs/*`, `pkg/*` (alias: `gcpm/*`), `policy/*`, `sync/*`, `gc/*`, `vcs/*`, `selfhost-dashboard`.
    - WASI: `fmt`, `eval`, `run`, `replay`, `test`, `pack`, `store/*`, `refs/*`, `pkg/*` (alias: `gcpm/*`), `policy/*`, `sync/*`, `gc/*`, `vcs/*`.
- Runtime commands that resolve `--engine selfhost` must use an explicit pinned artifact identity
  (`--selfhost-artifact` or `GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT`), and
  `--selfhost-bootstrap artifact-only`; implicit filesystem fallback discovery is rejected.
  - Applies to: `fmt`, `eval`, `explain`, `run`, `replay`, `optimize`, and `vcs hash`.
- Package/frontend commands without an explicit engine (`typecheck`, `test`, `apply-patch`, `pack`)
  default to the selfhost frontend.
  - Toolchain artifact resolution still follows:
    - `--selfhost-artifact <path>`, or
    - `GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT=<path>`, or
    - `./.genesis/selfhost/toolchain.gc` if present, or
    - workspace fallback `selfhost/toolchain.gc` if present.
- Kernel module evaluation uses the compiled evaluator by default and fails closed on compilation errors.
- Tree-walk evaluation is reserved for explicit parity harnesses and is not a mainline CLI execution mode.

## Rust Engine Compatibility Mode (Historical Comparisons Only)

`--engine rust` and `--coreform-frontend rust` exist solely to support deterministic parity checks
against prior Rust semantics during the selfhost cutover.

They are disabled by default and require explicit opt-in in development/debug profiles:
- use dedicated parity binaries:
  - `target/debug/genesis_parity`
  - `target/debug/genesis_wasi_parity`

Release builds reject `--engine rust` and `--coreform-frontend rust` unconditionally.

CI must keep production binaries (`genesis`, `genesis_wasi`) rust-engine-free and run
parity/golden comparisons through dedicated parity harnesses (e.g.
`scripts/selfhost_strict_smoke.sh` / `scripts/selfhost_strict_golden.sh`).

Dedicated compatibility harness entrypoints:
- debug parity harness binaries:
  - `target/debug/genesis_parity`
  - `target/debug/genesis_wasi_parity`
- release-profile rejection gate:
  - `scripts/selfhost_release_profile_guard.sh`
  - This script must pass while release binaries still reject rust engine/frontend paths.

## Subcommands (Signing + Policy)

- `genesis fmt <file> [--check] [--engine rust|selfhost]`
  - when `--engine` is omitted, engine defaults to `selfhost`.
  - `--engine rust` remains available for parity/comparison workflows.
  - `--engine selfhost` runs the self-hosted CoreForm toolchain inside the kernel and therefore honors `--step-limit/--no-step-limit`.
  - JSON output includes `data.selfhost_artifact` (`null` for rust engine, otherwise `{path,hash,source}`).
- `genesis eval <file> [--engine rust|selfhost] [--stage1-pipeline] [--stage1-gate] [--stage2-gate]`
  - when `--engine` is omitted, engine defaults to `selfhost` (same rule as `fmt`).
  - `--engine selfhost` runs self-hosted parse+canonicalize in-kernel before evaluation.
  - `--stage1-pipeline` runs Stage-1 CoreForm->CoreForm transforms before evaluation.
  - `--stage1-gate` enforces `core/obligation::stage1-validation` for the eval input.
  - `--stage2-gate` enforces `core/obligation::translation-validation` in fail-closed mode:
    unsupported modules fail the gate, and supported modules must validate successfully.
  - For Stage-2 gating, validation input is Stage-1 transformed CoreForm (matching package translation-validation flow), even when `--stage1-pipeline` is not requested.
  - JSON output includes `data.selfhost_artifact` (`null` for rust engine, otherwise `{path,hash,source}`).
  - JSON output includes `data.kernel_eval_backend` (`"compiled"`).
- `genesis explain <file> --contract <expr-or-symbol> --msg <coreform> [--engine rust|selfhost]`
  - when `--engine` is omitted, engine defaults to `selfhost`.
  - `--engine selfhost` runs self-hosted parse/canonicalize for the input module and self-hosted parse for `--contract`/`--msg`.
  - JSON output includes `data.kernel_eval_backend` (`"compiled"`).
- `genesis run <file> --caps <policy.toml> [--log <out.gclog>] [--engine rust|selfhost]`
  - when `--engine` is omitted, engine defaults to `selfhost`.
  - `--engine selfhost` runs self-hosted parse/canonicalize before evaluating the effect program.
  - JSON output includes `data.kernel_eval_backend` (`"compiled"`).
- `genesis replay <file> --log <log.gclog> [--store <dir>] [--engine rust|selfhost]`
  - when `--engine` is omitted, engine defaults to `selfhost`.
  - `--engine selfhost` runs self-hosted parse/canonicalize before replaying against the deterministic log.
  - JSON output includes `data.kernel_eval_backend` (`"compiled"`).
- `genesis selfhost-artifact --out <file> [--min-stage2-supported-modules <N>] [--min-stage2-validated-modules <N>]`
  - emits a canonical self-host toolchain artifact used by `--engine selfhost` bootstrap.
  - runs Stage-1 + Stage-2 validation for each embedded selfhost module and records per-module gate metadata.
  - exits with code `30` when validation fails or configured Stage-2 minimum thresholds are not met.
- `genesis selfhost-dashboard [--markdown <file>] [--store <dir>]`
  - emits a cutover dashboard artifact (`genesis/selfhost-cutover-dashboard-v0.2`) into a content-addressed store path.
  - writes a markdown mirror (default: `docs/status/SELFHOST_CUTOVER.md`) with routed/default selfhost coverage.

CI strict selfhost gates:
- `scripts/selfhost_strict_smoke.sh`: fast strict routing health check.
- `scripts/selfhost_strict_golden.sh`: strict golden sweep across `tests/spec/coreform/*` and `tests/spec/pkg_*` fixtures, including WASI strict checks for available routed commands.
- `genesis keygen --out <key.toml>`: generate an Ed25519 signing key (see `docs/spec/SIGNING.md`).
- `genesis sign --pkg <package.toml> --key <key.toml> [--acceptance <hex>] [--signatures <file>]`:
  - sign the acceptance artifact hash and write a signature artifact into the evidence store
  - update `.genesis/last_signature` and the signature set file (default `.genesis/signatures.gc`)
- `genesis verify --pkg <package.toml> [--policy <policy.toml>] [--signatures <file>]`:
  - when `--policy` is provided, enforce signature policy (see `docs/spec/REGISTRY_POLICY.md`)
- `genesis transparency-verify --pkg <package.toml>`: verify the local transparency log chain (see `docs/spec/TRANSPARENCY_LOG.md`)
- `genesis optimize <file> [--engine rust|selfhost] ...`
  - when `--engine` is omitted, engine defaults to `selfhost` (same rule as `fmt`).
- `genesis semantic-edit index --pkg <package.toml> --module-path <path>`
  - emits a deterministic canonical AST node index with stable semantic node IDs.
  - output kind: `genesis/semantic-edit-index-v0.1`.
- `genesis vcs hash --in <file> [--engine rust|selfhost]`
  - when `--engine` is omitted, engine defaults to `selfhost` (same rule as `fmt`).
  - JSON output includes `data.selfhost_artifact` (`null` for rust engine, otherwise `{path,hash,source}`).
- `genesis gcpm ...` is a first-class alias to `genesis pkg ...` and must preserve identical JSON `kind` contracts.
  - See `docs/spec/GCPM_CLI_CONTRACT_v0.1.md`.
  - Command schema IDs are enumerated in `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`.
- Workspace lifecycle commands:
  - `genesis gcpm new` initializes `genesis.workspace.toml` + `genesis.lock`.
  - `genesis gcpm remove <dep>` removes dependency requirements deterministically.
  - `genesis gcpm migrate --pkg package.toml` migrates package-only repos to workspace+lock form.
  - `genesis gcpm abi --pkg <package.toml>` exports a deterministic contract ABI/introspection index including contract op tables, type/effect signatures, required capabilities, and manifest obligations.
  - `genesis gcpm test --pkg <package.toml>` is a gcpm alias for package obligation execution.
  - `genesis gcpm run <task>` executes canonical workspace tasks from `genesis.workspace.toml` (no shell glue).
  - `genesis gcpm env --profile <dev|ci|release>` realizes deterministic profile artifacts under `.genesis/env/<profile-hash>/`.
  - `genesis gcpm self-optimize --pkg <package.toml> [--dry-run]` runs a closed-loop propose/optimize/validate/apply flow and only promotes rewrites when `core/obligation::translation-validation` and package obligations succeed.
  - ABI/introspection schema: `docs/spec/GCPM_ABI_INDEX_v0.1.md`.
  - Workspace descriptor schema: `docs/spec/GCPM_WORKSPACE_v0.1.md`.
  - Environment realization schema: `docs/spec/GCPM_ENV_v0.1.md`.
  - JSON output for `test` includes `data.kernel_eval_backend_default = "compiled"`.
- `genesis gcpm lock|update|publish --json` emit deterministic AI workflow reports under `data.report`.
  - See `docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md`.
- `genesis gcpm --json` emits prompt-safe deterministic telemetry under `data.telemetry`.
  - See `docs/spec/GCPM_TELEMETRY_v0.1.md`.
- `genesis gcpm doctor --caps <caps.toml> [--lock genesis.lock]`
  - emits `kind = "genesis/pkg-doctor-v0.1"` with deterministic `data.doctor` diagnostics.
  - diagnostic schema is defined in `docs/spec/GCPM_DIAGNOSTICS_v0.1.md`.

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
  "error": null,
  "diagnostics_schema": "genesis/diagnostics-schema-v1",
  "diagnostics": []
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
  },
  "diagnostics_schema": "genesis/diagnostics-schema-v1",
  "diagnostics": [
    {
      "version": "v1",
      "severity": "error",
      "code": "parse/coreform",
      "message": "…",
      "exit_code": 10,
      "suggested_fix": "verify syntax and canonicalize with `genesis fmt --check <file>`."
    }
  ]
}
```

`error.context` is optional and may be omitted or `null`.
`diagnostics` is always present in JSON output:
- success cases: `[]`
- failure cases: at least one typed diagnostic entry with stable `code` and `exit_code`
