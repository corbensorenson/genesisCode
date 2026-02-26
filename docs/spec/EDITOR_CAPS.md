# Genesis Editor Capability Surface (v0.1)

This document defines editor-specific host capabilities required beyond existing
`gfx/*`, `io/fs::*`, `core/store::*`, `core/refs::*`, and `core/sync::*`.

All operations are deny-by-default, effect-logged, and replay-checked where responses are deterministic.

## Capability families

- `editor/clipboard::*`
- `editor/dialog::*`
- `editor/plugin::*`
- `editor/task::*`
- `editor/watch::*`

## `editor/clipboard::*`

- `editor/clipboard::get`
  - payload: `{}`
  - response: `{ :ok true :mime "text/plain" :data <bytes-or-str> } | ERROR`
- `editor/clipboard::set`
  - payload: `{ :mime "text/plain" :data <bytes-or-str> }`
  - response: `{ :ok true } | ERROR`

Determinism:
- replay uses logged response; host clipboard is never queried during replay.

## `editor/dialog::*`

- `editor/dialog::open`
  - payload: `{ :title str :multi bool :filters [ ... ] :start-dir str|nil }`
  - response: `{ :ok true :paths [str ...] } | { :ok false } | ERROR`
- `editor/dialog::save`
  - payload: `{ :title str :default-name str|nil :filters [ ... ] :start-dir str|nil }`
  - response: `{ :ok true :path str } | { :ok false } | ERROR`

Determinism:
- replay consumes logged selected paths.
- headless hosts may return `{ :ok false }` when no dialog bridge is configured.

## `editor/plugin::*`

- `editor/plugin::command`
  - payload: `{ :plugin str|sym :command str|sym :payload term :request-schema-id? str|sym :response-schema-id? str|sym }`
  - response: bridge-defined CoreForm term | ERROR
  - policy: wrapper over generic host extension ABI (`host/plugin::command`) and must use per-op
    allowlists (`allow_plugins`, optional `allow_commands`, and `allow_schema_ids` when typed schemas are present) plus bridge policy.

Determinism:
- replay consumes logged bridge responses.
- production hosts must execute a configured bridge command; synthetic echo responses are forbidden.

## `editor/task::*`

Background worker tasks (lint/typecheck/optimize/indexing + workflow orchestration).

- `editor/task::spawn`
  - payload: `{ :task-kind sym :input term :budget-ms int|nil }`
  - response: `{ :ok true :task-id str :task-kind str :state :running|:done|:failed :task-contract map :partial-count int } | ERROR`
- `editor/task::poll`
  - payload: `{ :task-id str }`
  - response: `{ :state :running|:done|:failed :result term|nil :task-contract map :partial term|nil :partial-emitted bool :partial-seq int :partial-total int } | ERROR`
- `editor/task::cancel`
  - payload: `{ :task-id str }`
  - response: `{ :ok true } | ERROR`

Schema-driven first-party task kinds:
- `editor/task::parse-module`
- `editor/task::fmt-coreform`
- `editor/task::lint-module`
- `editor/task::optimize-module`
- `editor/task::typecheck-pkg`
- `editor/task::test-pkg`
- `editor/task::build-pkg`
- `editor/task::run-pkg`
- `editor/task::debug-pkg`
- `editor/task::refactor-module`
- `editor/task::index-workspace`

Each task exposes a deterministic `:task-contract` map:
- `:task-kind` (symbol)
- `:schema-version` (currently `1`)
- `:schema/required` (vector of required input keys)
- `:schema/optional` (vector of optional input keys)
- `:schema/output-keys` (vector of stable output keys)

For long-running workflows (`build/run/debug/refactor/index`), `editor/task::poll`
emits structured `:partial` payloads with deterministic phase progress metadata.

Determinism:
- all poll transitions are replayed from logs.
- task execution must not access ambient capabilities outside task policy.

## `editor/watch::*`

Filesystem/workspace change notifications.

- `editor/watch::subscribe`
  - payload: `{ :root str :globs [str ...] }`
  - response: `{ :ok true :watch-id str } | ERROR`
- `editor/watch::poll`
  - payload: `{ :watch-id str }`
  - response: `{ :events [ {:kind sym :path str :stamp int} ... ] } | ERROR`
- `editor/watch::unsubscribe`
  - payload: `{ :watch-id str }`
  - response: `{ :ok true } | ERROR`

Determinism:
- editor reactions are deterministic from logged watch event batches.
- host polls must return filesystem-derived deltas (`:create/:modify/:delete`) and must not emit synthetic heartbeat events.

## Bridge command contract (host runtime)

For editor ops that require host integration (`editor/plugin::command`, optionally
`editor/dialog::*`, and bridge-routed task kinds), the runtime reads per-op
`caps.toml` fields:

- `bridge_cmd` (string, required): executable path under the op `base_dir`.
- `bridge_args` (array<string>, optional): fixed args prepended before the op symbol.

Invocation contract:

- process cwd = op `base_dir`
- argv = `<bridge_cmd> <bridge_args...> <op-symbol>`
- env:
  - `GENESIS_HOST_BRIDGE_OP=<op-symbol>`
  - `GENESIS_HOST_BRIDGE_FAMILY=editor`
- stdin:
  - framed payload `<len>\n<payload>` where `payload` is canonical CoreForm
- stdout:
  - empty => `{ :ok true }`
  - framed response `<len>\n<payload>` where `payload` is one CoreForm term
- non-zero exit, invalid UTF-8, or invalid CoreForm => sealed ERROR

Normative framing details are specified in `docs/spec/HOST_BRIDGE_PROTOCOL.md`.

## Policy requirements

Recommended defaults:
- dev editor sessions: allow clipboard/dialog/task/watch with workspace sandboxing.
- CI/headless: deny clipboard/dialog, allow task/watch as needed.

Policies must constrain:
- dialog path roots
- task kinds permitted
- watch roots/globs

## Security boundaries

- Editor capabilities are not kernel primitives.
- Editor plugins receive least-privilege capability subsets.
- Task worker payloads are treated as untrusted user input and must return protocol ERROR on invalid shapes.

## Plugin/agent enforcement model (Prelude-level)

- Plugin capability enforcement is deny-by-default in GenesisCode:
  - `core/editor/plugin::caps-allowed?` evaluates `{:allow [...], :deny [...]}` policies.
  - `core/editor/plugin::perform` only emits host effects when policy allows the op.
  - denied plugin requests return deterministic sealed `editor/plugin/cap-denied` errors.
- Agent actions are patch-first and obligation-gated:
  - `core/editor/action::agent-propose-patch` wraps `core/vcs::diff`.
  - `core/editor/action::agent-apply-patch-with-obligations` wraps
    `core/vcs::apply` + `core/pkg::verify` and returns an acceptance report.
- Agent session logs are first-class deterministic artifacts:
  - `core/editor/agent::store-session-log` stores
    `genesis/editor-agent-session-v0.2` artifacts through `core/store::put`.
