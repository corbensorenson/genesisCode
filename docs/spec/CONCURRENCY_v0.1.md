# Concurrency Spec (v0.1)

Status: normative for `core/task::*` ABI shape and deterministic replay behavior.

## Goals
- Keep kernel purity unchanged (no threads in kernel semantics).
- Express concurrency only as capability effects.
- Preserve deterministic replay and auditability.

## Operation Surface
- `core/task::spawn`
- `core/task::await`
- `core/task::cancel`
- `core/task::status`
- `core/task::scope`

All operations are deny-by-default under capability policy.

## Payload and Result Schemas

Canonical task data contracts:
- Task handle:
```coreform
{:task-id <symbol|str>}
```
- Successful task completion:
```coreform
{:task-id <symbol|str> :state :done :result <term> :error nil}
```
- Failed task completion:
```coreform
{:task-id <symbol|str> :state :failed :result nil :error <term>}
```
- Cancelled task:
```coreform
{:task-id <symbol|str> :state :cancelled :result nil :error nil}
```

### `core/task::spawn`
Request payload:
```coreform
{:scope <str|symbol|nil> :label <str|symbol|nil> :payload <term>}
```
Optional executable payload forms inside `:payload`:
```coreform
{:task/eval <coreform-expr-term>}
{:task/eval <coreform-expr-term> :task/arg <term>}
{:task/eval <coreform-expr-term> :task/args [<term> ...]}
```
Semantics:
- The runner evaluates `:task/eval` in a fresh prelude environment.
- If `:task/arg`/`:task/args` are present, the evaluated value must be callable and is applied left-to-right.
- If evaluation yields an effect program, it is executed under the same capability policy as the parent run.
- Final non-datum results are rejected as `core/task/program-error`.

Result payload:
```coreform
{:task-id <symbol|str> :state :running|:done|:failed|:cancelled}
```

### `core/task::await`
Request payload:
```coreform
{:task-id <symbol|str>}
```
Result payload:
```coreform
{:task-id <symbol|str> :state :done|:failed|:cancelled :result <term|nil> :error <term|nil>}
```

### `core/task::cancel`
Request payload:
```coreform
{:task-id <symbol|str>}
```
Result payload:
```coreform
{:task-id <symbol|str> :state :cancelled|:done|:failed}
```

### `core/task::status`
Request payload:
```coreform
{:task-id <symbol|str>}
```
Result payload:
```coreform
{:task-id <symbol|str> :state :running|:done|:failed|:cancelled}
```

### `core/task::scope`
Request payload:
```coreform
{:scope <str|symbol|nil>}
```
Result payload:
```coreform
{:scope <str|symbol|nil> :state :entered}
```

## Determinism Rules
- Task identifiers must be deterministic within a run (monotonic logical IDs are recommended).
- Runner-visible schedule decisions must be recorded in effect logs.
- Replay must fail on:
  - missing/extra task events
  - task-id mismatch
  - state transition mismatch
  - response hash mismatch

## Replay Contract
- `spawn`/`await`/`cancel`/`status`/`scope` responses are replayed from log, never recomputed from wall-clock scheduling.
- Any host runtime that cannot satisfy deterministic replay for these ops must return sealed `core/caps/not-supported`.

## Policy Hooks
Capability policy should support:
- `max_tasks`
- `max_workers`
- `max_queue`
- per-op timeout and cancellation controls

Absent policy entries default to deny.
