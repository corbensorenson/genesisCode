# Assurance Artifacts v0.1

This document defines deterministic assurance evidence artifacts used by regulated-release policy gates.

## 1. Requirements Trace Artifact

Artifact kind:
- `:type :vcs/evidence`
- `:kind :requirements-trace`

Required fields:
- `:status :verified`
- `:graph-h <hex64>` content hash of the requirement graph source
- `:release`
  - `:snapshot <hex64>` release snapshot hash (required)
  - `:commit <hex64>|nil` optional commit hash binding
  - `:policy <hex64>|nil` optional policy hash binding
- `:requirements` vector of requirement maps:
  - `:id <string>`
  - `:level :system|:hlr|:llr`
  - optional `:parents [<string> ...]`
  - optional `:hazards [<string> ...]`
  - `:links` map with at least one of:
    - `:modules [{:path <string> :exports [<qualified-sym> ...]} ...]`
    - `:obligations [<symbol|string> ...]`
    - `:evidence-kinds [<symbol|string> ...]`

Policy gate behavior:
- if `:release/:commit` is present, it MUST match the protected commit hash
- `:release/:snapshot` MUST match the protected commit result snapshot
- linked obligations/evidence-kinds MUST not dangle
- malformed artifacts fail closed

Pre-commit binding:
- `:release/:commit = nil` is allowed so trace evidence can be produced before commit finalization.
- This avoids unsatisfiable hash cycles between commit hash and evidence hash while preserving snapshot/policy anchoring.

## 2. Tool Qualification Artifact

Artifact kind:
- `:type :vcs/evidence`
- `:kind :tool-qualification`

Required fields:
- `:status :qualified`
- `:release`
  - `:commit <hex64>|nil` optional commit hash binding
  - `:policy <hex64>|nil` optional policy hash binding
- `:requirements [<string> ...]` non-empty
- `:tools` non-empty vector of maps:
  - `:name <string>`
  - `:path <string>`
  - `:blake3 <hex64>`
  - `:size-bytes <int>`
- `:qualification-tests` non-empty vector of maps:
  - `:id <string>`
  - `:artifact <hex64>`
  - `:result :pass`

Policy gate behavior:
- if `:release/:commit` is present, it MUST match the protected commit hash
- if `:release/:policy` is present and policy gating provided a policy hash, it MUST match
- test entries with non-`:pass` results fail closed

Pre-commit binding:
- `:release/:commit = nil` is allowed for pre-commit qualification evidence attachment.

## 3. Deterministic CLI Emitters

- `genesis gcpm trace` emits `genesis/pkg-requirements-trace-v0.1`.
- `genesis gcpm qualify` emits `genesis/pkg-tool-qualification-v0.1`.

Both commands:
- produce canonical CoreForm evidence bytes
- support `--no-store` for deterministic file-only output
- import to local store when `--no-store` is not set

