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

