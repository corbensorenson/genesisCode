# Assurance Artifacts v0.1

This document defines deterministic assurance evidence artifacts used by regulated-release policy gates.

Objective-level standards posture mapping is tracked in:
- `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
- `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`

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

## 3. Assurance Pack Artifact

Artifact kind:
- `:type :vcs/evidence`
- `:kind :assurance-pack`

Required fields:
- `:status :ready`
- `:target-profile :custom|:do178c-dal-a|:do178c-dal-b|:nasa-class-a|:nasa-class-b|:iec62304-class-c`
- `:release`
  - `:snapshot <hex64>` required
  - `:commit <hex64>|nil` optional commit hash binding
  - `:policy <hex64>|nil` optional policy hash binding
- `:trace-matrix` map:
  - `:artifact <hex64>` requirements-trace artifact hash
  - `:source <string>` source path used to load the trace artifact
  - `:requirements [<map> ...]` copied requirement trace payload
- `:qualified-tool-manifest` map:
  - `:artifact <hex64>` tool-qualification artifact hash
  - `:source <string>` source path used to load the qualification artifact
  - `:tools [<map> ...]`
  - `:qualification-tests [<map> ...]`
- `:coverage-exports` vector of maps:
  - `:artifact <hex64>`
  - `:profile <symbol>`
  - `:ok <bool>`
  - `:source <string>`
- `:independence-attestations` vector of maps:
  - `:kind :independence-attestation`
  - `:roles [<symbol> <symbol>]`
  - `:attestor <string>`

Profile gate behavior:
- `:do178c-dal-a` and `:nasa-class-a` require at least one independence attestation and minimum `:mcdc` coverage rank.
- `:do178c-dal-b` and `:nasa-class-b` require at least one independence attestation and minimum `:decision` coverage rank.
- `:iec62304-class-c` requires minimum `:symbol` coverage rank.
- `:custom` has no additional profile constraints beyond valid trace/qualification artifacts.

Deterministic bundle mirror behavior:
- optional `--bundle-dir <dir>` materializes reproducible files:
  - `assurance_pack.gc`
  - `requirements_trace.gc`
  - `tool_qualification.gc`
  - `coverage/*.gc`
  - `bundle_manifest.gc`

## 4. Deterministic CLI Emitters

- `genesis gcpm trace` emits `genesis/pkg-requirements-trace-v0.1`.
- `genesis gcpm qualify` emits `genesis/pkg-tool-qualification-v0.1`.
- `genesis gcpm assurance-pack` emits `genesis/pkg-assurance-pack-v0.1`.

All three commands:
- produce canonical CoreForm evidence bytes
- support `--no-store` for deterministic file-only output
- import to local store when `--no-store` is not set
