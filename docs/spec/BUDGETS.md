# Budgets Obligation (v0.2)

This document defines the `core/obligation::budgets` obligation, which enforces deterministic, per-test resource budgets and emits evidence artifacts.

Budgets are intended to keep packages within operational envelopes for CI and for downstream consumers. They are not a semantic feature of the language.

## Configuration (`package.toml`)

Budgets are configured under `[budgets]`:

- `max_steps_per_test` (integer, optional): maximum kernel evaluation steps for each unit test.
- `max_effect_entries_per_test` (integer, optional): maximum effect log entries for each effectful test.
- `max_effect_log_bytes_per_test` (integer, optional): maximum canonical `.gclog` byte length for each effectful test.

If a field is omitted, that particular check is not enforced (but the evidence artifact still records measured values).

## Semantics

For each executed unit test:

1. Measure `steps`: the final `EvalCtx.steps` after evaluating the test body and (if applicable) running effects.
2. If the test produced an effect log:
   - measure `effect_entries = len(entries)`
   - measure `effect_log_bytes = len(canonical_log_string_bytes)`

The obligation fails if any configured budget is exceeded.

## Evidence Artifact

The obligation must write a report artifact to the package evidence store:

- `:kind = "genesis/budgets-v0.2"`
- `:package` (string): package name
- `:ok` (bool)
- `:limits` (map): the configured budget values (only keys that are present in `package.toml`)
- `:tests` (vector): per-test entries, each a map:
  - `:suite` (symbol)
  - `:name` (string)
  - `:ok` (bool): whether this test satisfies all configured budgets
  - `:steps` (int)
  - `:effect-entries` (int)
  - `:effect-log-bytes` (int)
- `:errors` (vector of strings) if `:ok = false`

