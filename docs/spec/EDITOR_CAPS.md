# Genesis Editor Capability Surface (v0.1)

This document defines editor-specific host capabilities required beyond existing
`gfx/*`, `io/fs::*`, `core/store::*`, `core/refs::*`, and `core/sync::*`.

All operations are deny-by-default, effect-logged, and replay-checked where responses are deterministic.

## Capability families

- `editor/clipboard::*`
- `editor/dialog::*`
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

## `editor/task::*`

Background worker tasks (lint/typecheck/optimize/indexing).

- `editor/task::spawn`
  - payload: `{ :task-kind sym :input term :budget-ms int|nil }`
  - response: `{ :ok true :task-id str } | ERROR`
- `editor/task::poll`
  - payload: `{ :task-id str }`
  - response: `{ :state :pending|:running|:done|:failed :result term|nil } | ERROR`
- `editor/task::cancel`
  - payload: `{ :task-id str }`
  - response: `{ :ok true } | ERROR`

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
