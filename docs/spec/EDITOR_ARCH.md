# Genesis Editor Architecture (Draft v0.1)

This defines the first production path for a GenesisCode-native GUI editor.

## Scope

- Editor core is implemented in GenesisCode.
- Host bridge provides rendering/window/input/filesystem/store/refs/sync effects.
- All mutating operations emit effect logs and semantic patch artifacts.

## Core subsystems

- Document model:
  - rope text buffer
  - AST/cache index for CoreForm forms
  - stable symbol index for navigation/refactoring
- Language pipeline:
  - parse/canonicalize/format
  - typecheck/effect-row reports
  - optimizer + translation-validation reports
- VCS + package integration:
  - GenesisGraph commit/log/blame/why
  - GenesisPkg lock/install/update/publish/verify
- UI:
  - multi-pane layout (editor + evidence + refs/policy + terminal)
  - deterministic command palette actions (effect-logged)

## Plugin/agent model

- Plugins are contracts with capability-scoped effects.
- Agent actions produce semantic patches + obligation evidence.
- Acceptance is policy-gated (`refs set` / publish rules).

### GenesisCode API surface (implemented in Prelude)

- Plugin contracts and capability policies:
  - `core/editor/plugin::make`
  - `core/editor/plugin::call`
  - `core/editor/plugin::command`
  - `core/editor/plugin::caps-allowed?`
  - `core/editor/plugin::perform` (deny-by-default capability wrapper)
- Agent session model (deterministic, replay/audit friendly):
  - `core/editor/agent::session-empty`
  - `core/editor/agent::{session-add-event,session-add-patch,session-add-evidence}`
  - `core/editor/agent::session-hash`
  - `core/editor/agent::session-log-artifact`
  - `core/editor/agent::store-session-log`
- Agent patch/acceptance flows:
  - `core/editor/action::agent-propose-patch`
  - `core/editor/action::agent-apply-patch-with-obligations`
  - `core/editor/agent::acceptance-report`

### Deterministic agent session artifact

- Session artifacts are canonical CoreForm maps:
  - `:kind` = `genesis/editor-agent-session-v0.2`
  - `:v` = `1`
  - `:session` = full session map (`:events`, `:patches`, `:evidence`)
  - `:session-h` = canonical hash of `:session`
- Session artifacts are stored through `core/store::put`, so they inherit
  deterministic content addressing and replay coverage from effect logs.

## Capability needs

- `io/fs::*` for workspace access (sandboxed)
- `core/store::*`, `core/refs::*`, `core/sync::*`
- `gfx/*` for rendering and input
- `sys/time::now` only through replayable effect log

## Milestones

1. Read-only viewer: syntax + AST + symbol navigation
2. Editing + canonical format + diagnostics pane
3. Patch workflow: propose/apply + obligations + evidence
4. Full package + publish workflow with policy visualization
