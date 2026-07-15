> Bundle Entry: `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# CLI JSON Schemas v0.1 (Non-GCPM)

This document freezes the `--json` schema IDs for `genesis` commands outside the `pkg/gcpm` surface.

`pkg/gcpm` schema IDs remain in `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`.

## Shared Envelope

All commands use the global envelope from `docs/spec/CLI.md`:

- top-level `ok` boolean
- top-level `kind` schema ID (table below)
- top-level `data` object for success
- top-level `error` object for failures
- `diagnostics_schema = "genesis/diagnostics-schema-v1"`
- `diagnostic_catalog` identifies `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json`,
  its semantic version, and its SHA-256 content identity
- `diagnostics` array (empty on success, non-empty on failure)

Failure diagnostics (`diagnostics[*]`) include stable machine-actionable routing fields:
- `id` (`genesis/diagnostic/v1/<family>/<name>`) and exact `code`
- `catalog_version` and `catalog_identity_sha256`
- `severity` and lifecycle `phase`
- `primary_span` (span object or `null`) and `related_spans` (array)
- `parameters` (JSON object; prose is not a routing contract)
- non-empty `likely_causes`, `safe_repair_actions`, and `documentation`
- `error_class` (string)
- `candidate_fix` (string)
- `blocking_capability` (string or `null`)
- `next_safe_action` (string)
- `repair_plan` (`genesis/diagnostic-repair-plan-v0.1` object)

The generated catalog is closed by
`docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.schema.json`. Exact lookup is available as
`genesis --json agent-index --diagnostic <exact-code>`. Unknown emitted codes
fail closed as `diagnostic/catalog-miss`; the unrecognized token is retained as
`parameters.reported_code` for implementation debugging.

An absent `primary_span` means the current producer has not localized the
failure; it does not implicate the entire input. Safe repair metadata never
authorizes a prompt-derived policy change. Any repair with
`policyEffect = "review-required"` requires a separate, explicit policy diff;
agents must not silently broaden capabilities or suppress obligations.

`repair_plan.action` distinguishes inspection, verification commands, source
patches, effectful commands, and policy review. Its preconditions and
postconditions bind the current diagnostic/content identities, require the
original command to be rerun, and preserve all declared obligations. The
authorization object always sets `policy_change_allowed = false` and
`obligation_suppression_allowed = false`. A concrete denied capability may
produce a `genesis/capability-policy-diff-v0.1` proposal, but that proposal is
never applicable by itself: `requires_review = true`, `auto_apply = false`,
and the active policy remains unchanged until a separate reviewed diff is
explicitly accepted. Prompt text has no repair or policy authority.

Every failure envelope exposes `error.context.schema =
"genesis/failure-context-v0.1"`, closed by
`docs/spec/GC_FAILURE_CONTEXT_v0.1.schema.json`. Its `domain`, `kind`, and
`operation` fields are stable routing facts; `facts` contains typed
domain-specific values. Parser, typechecker, evaluator, package, policy,
replay, patch, build, and deployment authorities emit richer typed contexts at
their source boundary. The envelope normalizer fail-closes any remaining
legacy context into the same schema, preserves scrubbed legacy details under
`facts.legacy_context`, and never permits an absolute host path in that
fallback. Human messages remain concise renderings and are not the only
failure contract.

Without `--json`, every cataloged CLI failure is rendered from that same
normalized diagnostic rather than from separately maintained prose. The first
line identifies the exact diagnostic code, failed `operation`, and one safe
normalized subject. The following `cause` and `next` fields select one primary
cause from structured context (falling back to the producer message and then
the catalog) and exactly one catalog-authorized next action. Subject and cause
selection use a fixed key priority through nested legacy/cause objects; arrays
retain source order. Absolute paths reduce to safe basenames, control
characters become spaces, fields are bounded, and missing facts render explicit
`unknown` placeholders. Unknown codes first fail closed to
`diagnostic/catalog-miss` and cannot inject terminal controls.

Human output wraps deterministically to `COLUMNS`, clamped to 24-160 columns
with a 96-column default. ANSI styling is enabled only for a terminal or a
nonzero `CLICOLOR_FORCE`; the presence of `NO_COLOR` always wins. Styling
changes labels only, so removing ANSI sequences reproduces no-color output
exactly. MCP and `--json` remain protocol surfaces and do not consume this
terminal rendering contract.

Failures emitted before a command result use `kind = "genesis/error-v0.2"`.
Command-result envelopes may retain the command-specific `kind` with
`ok = false`, structured `error`, and non-empty `diagnostics`; this preserves
the schema identity of deterministic result data such as obligation reports.

The versioned diagnostic golden authority is
`tests/diagnostics/goldens/v0.1/diagnostics.json`. It freezes exact catalog-bound
failure projections for malformed syntax, type/effect mismatch, unhandled
effects, seal misuse, replay tampering, path normalization, exhausted budgets,
invalid packages, stale semantic patches, and incompatible runtime profiles.
`scripts/check_cli_diagnostics_contract.sh` rejects host-path leakage and
weakened repair authorization, then reproduces all ten vectors through the
production CLI. Intentional contract changes use the separate
`scripts/update_cli_diagnostic_goldens.sh` generator followed by review; normal
checks are read-only and never bless drift.

Diagnostic repair utility is measured by the versioned corpus at
`benchmarks/diagnostics/repair_utility/v0.1/workloads.json` and retained report
at `benchmarks/diagnostics/repair_utility/v0.1/report.json`. The corpus contains
18 sorted cases: 15 automatically repairable mutations across missing and
extra delimiters, integer operand types, primitive-name edits, and package
schema versions, plus three deny-by-default capability cases where the only
safe automatic outcome is abstention. Hidden `expected` bytes are available
only to the benchmark runner; pinned reference agents receive stable filenames,
current bytes, the original command, repair authorization, and either the
human message or the full structured diagnostic. The agent implementation is
source-hashed, runs through isolated stdio without filesystem, subprocess, or
network imports, and is rejected if it embeds case IDs or oracle fields.

`exactRecovery` requires exact expected bytes, a successful original-command
retry, and an identical structured result from an independent semantic replay
within two repair turns. `overRepair`, `policyBroadening`, and `regression` are
separate zero-tolerance metrics. Review-required capability cases must preserve
all source and policy bytes and emit `safeAbstention`; a non-applicable policy
proposal is never a repair. Token cost uses the exact, model-independent
`genesis/utf8-byte-token-v0.1` profile over canonical request and response JSON.
Every report publishes the agent model/runtime version, decoding mode, context,
seed, attempts, failure codes, and cost. The retained report is accepted only
after two production CLI executions are byte-identical and the independent
verifier recomputes all identities, counts, rates, comparisons, and safety
decisions. This deterministic reference result measures the utility of the
current API; it is not a substitute for the held-out multi-model evidence
required by later agent-corpus milestones.

The report is closed by
`docs/spec/GC_REPAIR_UTILITY_REPORT_v0.1.schema.json`. Normal diagnostics checks
verify it read-only. Intentional corpus, agent, or diagnostic changes require
`scripts/update_gc_repair_utility_report.sh` followed by review.

## Command -> Kind

### Core runtime commands

- `parse` -> `genesis/parse-v0.1`
- `fmt` -> `genesis/fmt-v0.2`
- `eval` -> `genesis/eval-v0.2`
- `explain` -> `genesis/explain-v0.2`
- `debug step` -> `genesis/debug-step-v0.1`
- `debug break` -> `genesis/debug-break-v0.1`
- `debug inspect` -> `genesis/debug-inspect-v0.1`
- `debug continue` -> `genesis/debug-continue-v0.1`
- `debug frames` -> `genesis/debug-frames-v0.1`
- `debug timeline` -> `genesis/debug-timeline-v0.1`
- `debug bisect` -> `genesis/debug-bisect-v0.1`
- `run` -> `genesis/run-v0.2`
- `replay` -> `genesis/replay-v0.2`
- `test` -> `genesis/test-v0.2`
- `pack` -> `genesis/pack-v0.2`
- `cli-schema` -> `genesis/cli-schema-v0.1`
- `agent-index` -> `genesis/agent-index-v0.1`
- `agent-index --search-symbol` -> `genesis/agent-symbol-search-v0.3`
- `agent-index --card` -> `genesis/agent-card-v0.3`
- `agent-plan` -> `genesis/agent-plan-v0.1`
- `bench *` -> `genesis/bench-v0.1` (subcommand result in `data.kind`)
- `warm` -> `genesis/warm-session-v0.2`

### Security / optimization / semantic tooling

- `keygen` -> `genesis/keygen-v0.2`
- `sign` -> `genesis/sign-v0.2`
- `transparency-verify` -> `genesis/transparency-verify-v0.2`
- `typecheck` -> `genesis/typecheck-v0.2`
- `optimize` -> `genesis/optimize-v0.2`
- `semantic-edit index` -> `genesis/semantic-edit-index-v0.1`
- `semantic-edit workspace-graph` -> `genesis/semantic-edit-workspace-graph-v0.1`
- `semantic-edit refactor-plan` -> `genesis/semantic-edit-refactor-plan-v0.1`
- `semantic-edit apply-plan` -> `genesis/semantic-edit-apply-plan-v0.1`
- `apply-patch` -> `genesis/apply-patch-v0.2`
- `session begin` -> `genesis/agent-session-begin-v0.1`
- `session status` -> `genesis/agent-session-status-v0.1`
- `session stage` -> `genesis/agent-session-stage-v0.1`
- `session test` -> `genesis/agent-session-test-v0.1`
- `session apply` -> `genesis/agent-session-apply-v0.1`
- `session abort` -> `genesis/agent-session-abort-v0.1`
- `verify` -> `genesis/verify-v0.2`

### Selfhost lifecycle

- `selfhost-artifact` -> `genesis/selfhost-artifact-v0.2`
- `selfhost-dashboard` -> `genesis/selfhost-dashboard-v0.2`

### Store / refs / commit

- `store put` -> `genesis/store-put-v0.2`
- `store get` -> `genesis/store-get-v0.2`
- `store has` -> `genesis/store-has-v0.2`
- `store verify` -> `genesis/store-verify-v0.2`
- `refs get` -> `genesis/refs-get-v0.1`
- `refs list` -> `genesis/refs-list-v0.1`
- `refs set` -> `genesis/refs-set-v0.1`
- `refs delete` -> `genesis/refs-delete-v0.1`
- `commit new` -> `genesis/commit-new-v0.1`
- `commit show` -> `genesis/commit-show-v0.1`

### Policy / sync / gc

- `policy list` -> `genesis/policy-list-v0.1`
- `policy show` -> `genesis/policy-show-v0.1`
- `policy set-default` -> `genesis/policy-set-default-v0.1`
- `sync pull` -> `genesis/sync-pull-v0.1`
- `sync push` -> `genesis/sync-push-v0.1`
- `gc plan` -> `genesis/gc-plan-v0.1`
- `gc run` -> `genesis/gc-run-v0.1`
- `gc pin` -> `genesis/gc-pin-v0.1`
- `gc unpin` -> `genesis/gc-unpin-v0.1`
- `gc purge` -> `genesis/gc-purge-v0.1`

### VCS

- `vcs hash` -> `genesis/vcs-hash-v0.2`
- `vcs diff` -> `genesis/vcs-diff-v0.1`
- `vcs apply` -> `genesis/vcs-apply-v0.1`
- `vcs log` -> `genesis/vcs-log-v0.1`
- `vcs blame` -> `genesis/vcs-blame-v0.1`
- `vcs why` -> `genesis/vcs-why-v0.1`
- `vcs merge3` -> `genesis/vcs-merge3-v0.1`
- `vcs resolve-conflict` -> `genesis/vcs-resolve-conflict-v0.1`

## Warm Protocol v0.2

`genesis warm` is a long-lived newline-delimited JSON transport. Every input
line and every output line is one UTF-8 JSON value closed by
`docs/spec/WARM_PROTOCOL_v0.2.schema.json`. The wire protocol is
`genesis/warm-protocol-v0.2`; responses use
`genesis/warm-response-v0.2`, typed errors use
`genesis/warm-protocol-error-v0.2`, and the final command envelope (when
observed by an embedding runtime) uses `genesis/warm-session-v0.2`.

### Lifecycle and ordering

1. `initialize` MUST be the first accepted method in generation zero and after
   every successful restart or worker-crash reset. It returns the exact server
   limits and capabilities clients may rely on.
2. Every request frame carries a 1..128 byte ASCII identifier. IDs are unique
   within a generation. Execute IDs intentionally identify both the immediate
   `accepted` response and exactly one terminal `completed` or typed-error
   response. `meta.sequence` totally orders emitted responses.
3. `execute` binds a stable workspace ID to one base-relative root and queues a
   parsed CLI command. Rebinding an active workspace ID, absolute paths, parent
   traversal, and symlink escape fail closed. A single serialized dispatcher
   prevents process-current-directory overlap between workspaces.
4. `cancel` terminalizes queued work immediately with a `not-started-v0.1`
   audit. In native Unix mode, each running command occupies a fresh process
   tree rooted at that group. Cancellation, request deadlines, session wall limits, disconnect
   drain expiry, and monitored resource violations send `SIGKILL` to that
   process tree and reap its leader before the terminal response is emitted.
   Initialization advertises `hard_termination = true` only for that profile.
5. `restart` succeeds only while idle, advances the generation, clears all
   workspace and ID bindings, and requires a new `initialize`. A contained
   worker panic performs the same generation reset, emits
   `warm/worker-crash`, and fails queued requests as
   `warm/worker-restarted` rather than replaying uncertain work.
6. `shutdown`, EOF, input failure, and disconnect stop admission. At most
   `max_drain_requests`, including the active request, remain eligible to run;
   excess accepted requests receive `warm/drain-bounded`. The retained set has
   one total `drain_timeout_ms` deadline. Expiry kills and reaps the active
   worker and terminalizes the rest as `warm/drain-timeout`. Every accepted ID
   therefore receives exactly one terminal response even when input closes.

### Bounds and isolation

- Frame allocation is bounded before UTF-8 or JSON decoding. Oversized lines
  are fully drained so the next frame remains parseable.
- Input transport capacity, execute queue depth, workspace count, argv count,
  argv entry size, deadline, session frame count, workspace idle lifetime,
  disconnect drain set, and disconnect drain time are finite and reported by
  `initialize`.
- `docs/spec/AGENT_SESSION_RESOURCES_v0.1.schema.json` closes the resource and
  audit shapes. Each native command has finite wall, aggregate CPU, kernel
  steps, aggregate process-group resident memory, combined output, effect-op,
  process-count, and workspace-growth ceilings. Clients cannot override these
  session-owned limits in request argv.
- Native output pipes continue draining after their capture ceiling, while the
  process group is killed. Disk enforcement combines inherited per-file OS
  limits with periodic and final base-relative workspace growth accounting.
  CPU, resident memory, and process count are sampled recursively across the
  complete process tree, including host-bridge descendants that create their
  own process groups.
- `genesis/agent-session-audit-v0.1` records the limit-set BLAKE3 identity,
  worker profile, observed wall/CPU/output/effects/disk/peak-memory/peak-process
  values, enforcement mechanisms, termination mode, and exceeded dimension.
  Native terminal successes place it at `data.audit`; terminal protocol errors
  place it at `error.details.audit`. Host absolute paths are forbidden.
- Valid, malformed, oversized, and invalid-UTF-8 frames all consume the finite
  session frame budget. EOF and transport failures do not.
- Workspace roots resolve beneath `--workspace-root`; response metadata and
  typed errors do not expose the configured absolute root. Idle eviction never
  removes a queued or running request's workspace.
- Native macOS/Linux mode accepts cancellation and control frames while one
  isolated worker runs. Other native targets fail closed at worker launch.
  WASI mode advertises `concurrent_control = false`,
  `hard_termination = false`, and `wasi-inline-v0.1`; it enforces logical
  step/shape/effect/output bounds but explicitly reports unavailable native OS
  CPU/process hard isolation. Clients MUST negotiate the worker profile and
  MUST NOT infer native hard-cancellation parity from wire-version parity.

### Closed frame examples

```json
{"protocol":"genesis/warm-protocol-v0.2","id":"init-1","method":"initialize","client":{"name":"agent","version":"1.0"}}
{"protocol":"genesis/warm-protocol-v0.2","id":"eval-1","method":"execute","workspace":{"id":"repo","root":"."},"argv":["--json","eval","main.gc"],"deadline_ms":5000}
{"protocol":"genesis/warm-protocol-v0.2","id":"cancel-1","method":"cancel","target_id":"eval-1"}
{"protocol":"genesis/warm-protocol-v0.2","id":"stop-1","method":"shutdown"}
```

Unknown fields, methods, protocol versions, duplicate IDs, uninitialized use,
invalid bounds, nested `warm`, workspace escape, queue overflow, stale cancel
targets, busy restart, and session exhaustion always produce a typed protocol
error. Schema IDs are immutable; incompatible framing requires a new protocol
version rather than permissive parsing.

## Determinism / versioning

- Schema IDs are immutable contracts for agent workflows.
- Backward-incompatible output changes require a version bump in `kind`.
- Command aliases MUST preserve `kind` for equivalent behavior.

## CLI Schema Contract (`genesis/cli-schema-v0.1`)

`genesis cli-schema` provides a machine-readable command/option schema for
agent planning.

### Envelope

- `kind = "genesis/cli-schema-v0.1"`
- Standard CLI JSON envelope from `docs/spec/CLI.md`.

### `data` payload

```json
{
  "schema": "genesis/cli-schema-v0.1",
  "runtime_profile": "production|parity-harness",
  "command": {
    "name": "genesis",
    "path": ["genesis"],
    "about": "optional string",
    "options": [
      {
        "name": "coreform_frontend",
        "long": "coreform-frontend",
        "short": null,
        "help": "optional string",
        "required": false,
        "global": true,
        "positional": false,
        "value_names": ["COREFORM_FRONTEND"],
        "default_values": [],
        "allowed_values": ["selfhost"],
        "action": "set",
        "value_type": "string",
        "multiple": false,
        "min_values": 1,
        "max_values": 1
      }
    ],
    "subcommands": [
      {
        "name": "fmt",
        "path": ["genesis", "fmt"],
        "about": "optional string",
        "options": [],
        "subcommands": []
      }
    ]
  },
  "mcp_interface": {
    "schema": "genesis/mcp-interface-v0.1",
    "protocolVersion": "2025-11-25",
    "identitySha256": "64 lowercase hexadecimal characters"
  }
}
```

### Profile-specific allowed values

- `runtime_profile = production`:
  - `engine` and `coreform-frontend` allowed values are `["selfhost"]`.
- `runtime_profile = parity-harness`:
  - `engine` and `coreform-frontend` allowed values are `["selfhost", "rust"]`.

### Determinism rules

- Option and subcommand entries are emitted in deterministic sorted order.
- Backward-incompatible schema changes require a `kind` version bump.

## Generated MCP Interface v0.1

`genesis mcp` implements the Model Context Protocol revision `2025-11-25`
over stdio. The transport is newline-delimited UTF-8 JSON-RPC 2.0. Stdout is
reserved exclusively for protocol frames; diagnostics and process failures use
stderr. Every input and output frame, queued call, root set, session frame count,
and argument vector has a finite configured bound.

### Lifecycle and capabilities

- `initialize` is the first request. The server selects protocol version
  `2025-11-25`, returns server identity and capabilities, and waits for
  `notifications/initialized` before serving normal requests.
- The core profile advertises `tools` and `resources`. It does not advertise
  prompts, logging, completion, elicitation, sampling, or Tasks.
- MCP Tasks are experimental in revision `2025-11-25`. Every core tool declares
  `execution.taskSupport = "forbidden"`; task methods and task-augmented calls
  fail unless a future separately versioned extension is explicitly negotiated.
- EOF/disconnect stops admission and applies the same finite drain-set and
  drain-time contract as warm mode. Every accepted call receives a terminal
  JSON-RPC result/error when stdout remains writable. There is no private
  shutdown method.

### Generated tool authority

The reviewed exposure-policy table selects canonical CLI command paths. Tool
names, descriptions, required arguments, allowed values, Draft 2020-12 input
schemas, and argv spellings are derived from the same Clap command tree emitted
by `genesis cli-schema`. Startup fails closed if a selected command or argument
disappears, a required CLI argument is omitted, or an exposed argument is
duplicated. Tool calls re-enter normal CLI dispatch and therefore cannot acquire
private semantics.

The exact core tools are `parse`, `format`, `check`, `run`, `test`, `explain`,
`search-symbol`, `get-card`, `diff`, `apply-patch`, `verify`, `replay`, `package`,
`build`, `session-begin`, `session-status`, `session-stage`, `session-test`,
`session-apply`, and `session-abort`. Each returns the canonical CLI JSON envelope as both text content
and `structuredContent`; command failures use `isError = true`. Unknown fields,
wrong JSON types, missing required fields, nested server commands, and unsupported
pagination fail with JSON-RPC errors.

### Roots and resources

When the client advertises roots, the server requests `roots/list` after
initialization. Only bounded local `file://` directory URIs are accepted. Each
root is canonicalized and must remain beneath the configured `--workspace-root`;
parent, absolute argument, symlink, non-file, duplicate, inaccessible, and
unadvertised-root escapes fail closed. Calls must select an exact returned URI
when multiple roots exist. Clients without the roots capability receive only the
configured boundary as their implicit root.

`resources/list` and `resources/read` expose the generated CLI schema, generated
MCP profile, core card, agent profile, task-card registry, symbol index, and
diagnostic catalog under `genesis://` URIs. Resource templates are an empty,
valid list. Embedded JSON is parsed before use; malformed authorities fail closed.

### Cancellation, progress, and errors

- A valid `_meta.progressToken` produces strictly increasing progress values
  `0` then `1`; no progress is emitted after completion or cancellation.
- `notifications/cancelled` terminalizes queued calls with JSON-RPC `-32800` and
  a not-started audit. For an active native call it kills and reaps the complete
  isolated process tree before emitting `-32800`; late progress/result frames
  are impossible. Disconnect drain cancellation uses `-32005` or `-32006`.
- Initialization advertises `experimental.genesis/sessionResources` with the
  closed limits and their identity. Successful tool results carry the audit in
  `_meta["genesis/sessionAudit"]`; cancellation, resource, worker, and drain
  errors carry it in `error.data.audit`. An oversized transport fallback
  preserves this audit.
- Parse, request-shape, method, parameter, initialization, root, queue, frame,
  output, worker, and resource failures use bounded JSON-RPC errors. CLI semantic
  failures remain typed Genesis tool results rather than transport failures.
- Request cancellation never applies to `initialize`, because initialization is
  complete before its response can be targeted as an active tool request.

The implementation authorities are `crates/gc_cli_driver/src/mcp/`,
`crates/gc_cli_driver/src/cli_schema.rs`, and the canonical command handlers.
`scripts/check_warm_protocol_contract.sh` verifies schema generation, lifecycle,
roots, resources, progress, cancellation controls, task rejection, malformed
frames, output purity, and production CLI execution.

## Transactional Agent Sessions v0.1

`genesis session` is the mutating agent boundary. Its durable state uses
`genesis/agent-transaction-v0.1`, snapshots use
`genesis/workspace-snapshot-v0.1`, and both are closed by
`docs/spec/AGENT_TRANSACTION_v0.1.schema.json`; command results use the six
stable kinds listed above. A transaction has one client-selected 1..64 byte
ASCII identifier, one immutable base snapshot, one current snapshot, an ordered
semantic-patch chain, one exact-snapshot verification record, and one of the
states `open`, `applied`, or `aborted`.

### Snapshot and isolation contract

1. `session begin` recursively captures the selected package manifest, declared
   modules, declared capability policy, and base-relative local dependency
   closure. Inputs must be regular files beneath the package root, and no path
   component may be a symlink. Retained path material is valid UTF-8 with
   nonempty `/`-separated segments; absolute, `.`, `..`, empty, backslash, and
   control-character forms are rejected both on capture and on load.
2. File blobs and sorted snapshot manifests use domain-separated BLAKE3
   identities. Every new or reused object is rehashed and length-checked before
   materialization and again before a live write. State, responses, and errors
   contain only relative names, client IDs, counts, and content identities,
   never absolute host paths.
3. The current snapshot is materialized below `.genesis/agent-sessions/`.
   `session stage` accepts one canonical Genesis semantic patch, applies it to a
   fresh candidate materialization, and runs package obligations there. The live
   package is not mutated. Failed patch parsing or application never activates
   the candidate.
4. `session test` rehashes the isolated workspace before running obligations and
   binds the acceptance artifact to that exact current snapshot. Capability
   policies must already belong to the captured snapshot; external policy paths
   cannot silently broaden authority.
5. `session apply` is explicit and fail-closed. It acquires the package-local
   apply lock, requires a successful verification for the exact current
   snapshot, rehashes both isolated and live inputs, rejects any stale live base,
   writes only snapshot-managed files, verifies the result, and retains the
   transaction record as `applied`. A failed managed-file write, post-write
   identity check, or state commit restores and rehashes the captured base or
   reports a distinct rollback failure.
6. `session abort` closes an open transaction without modifying the live
   package. Closed transactions cannot be staged, tested, applied, or aborted
   again, but remain inspectable through `session status`.

Snapshot admission is bounded to 4096 files and 256 MiB. Session IDs, object
locations, snapshot membership, stale-base comparison, verification identity,
and lifecycle state all fail closed. Arbitrary byte editing is intentionally not
a session authority: writes enter through semantic patches. O(1)-logical
copy-on-write forks and multi-candidate ranking remain the separately measured
R1.6 contract.
