# Coverage Obligation (v0.2)

This document specifies the normative behavior of the `core/obligation::coverage` obligation.

## Goal

Ensure that the package’s public, non-test API is exercised by unit tests.

## Definitions

- **Exported symbol**: Any symbol listed in a module’s static `::meta` map at key `:exports`.
- **Test suite symbol**: Any symbol listed in `package.toml` under `tests` or `property_tests`.
- **Non-test export (tracked symbol)**: `exported_symbols − test_suite_symbols`.
- **Hit**: A tracked symbol is considered “hit” when it is evaluated in *variable position* during test execution (i.e., the evaluator evaluates a `Term::Symbol` and attempts environment lookup for that symbol).
  - Quoted symbols (e.g. `'(foo)`) do not count as hits.
  - Symbols used only as data (e.g. map keys, message op symbols) do not count as hits unless they are evaluated as variables.
- **Decision sample**: every evaluated `if` condition increments deterministic structural counters:
  - `:total` (all evaluated `if` conditions),
  - `:taken-true`,
  - `:taken-false`.

## Obligation Behavior

`core/obligation::coverage` MUST:

1. Compute `tracked_symbols` as the union of all module `:exports` symbols, minus `tests` and `property_tests` suite symbols from the manifest.
2. If `tracked_symbols` is empty, succeed and emit a coverage report noting there are no non-test exports.
3. If the manifest has no unit tests (`tests` is empty) and `tracked_symbols` is non-empty, fail.
4. Execute the package unit tests and measure hits for `tracked_symbols`.
   - For effectful tests, the obligation MUST NOT re-run host capabilities; it MUST use effect log replay (`genesis replay` semantics) to execute continuations deterministically.
5. Fail if any `tracked_symbol` has total hit count `0` across all unit tests.
6. Collect deterministic decision counters (`:total`, `:taken-true`, `:taken-false`) for each executed test and aggregate totals across the obligation run.

## Evidence Artifact

The obligation MUST write a report artifact to the evidence store with:

- `:kind` = `"genesis/coverage-v0.2"`
- `:package` = package name (string)
- `:ok` = boolean
- `:definition` = string describing the tracked set rule
- `:exports` = vector of `{ :sym <symbol> :hits <int> }` for each tracked symbol
- `:missing` = vector of missing tracked symbols (those with `:hits = 0`)
- `:structural` = `{ :decision { :total <int> :taken-true <int> :taken-false <int> } }`
- `:tests` = per-test vector of `{ :suite <symbol> :name <string> :hits <vector> :decision <map> }`
- `:errors` = (optional) vector of strings

All terms MUST be in canonical CoreForm form.
