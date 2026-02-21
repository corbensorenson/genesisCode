# Coverage Obligation (v0.2)

This document specifies the normative behavior of coverage obligations:

- `core/obligation::coverage` (symbol profile)
- `core/obligation::coverage-decision` (statement+decision profile)
- `core/obligation::coverage-mcdc` (statement+decision+MC/DC profile)

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

Coverage obligations MUST:

1. Compute `tracked_symbols` as the union of all module `:exports` symbols, minus `tests` and `property_tests` suite symbols from the manifest.
2. If the manifest has no unit tests (`tests` is empty) and either tracked exports are non-empty or the selected profile requires structural gates, fail.
4. Execute the package unit tests and measure hits for `tracked_symbols`.
   - For effectful tests, the obligation MUST NOT re-run host capabilities; it MUST use effect log replay (`genesis replay` semantics) to execute continuations deterministically.
5. Collect deterministic structural coverage using stable site identities:
   - statement sites (`:stmt`) and decision sites (`:decision`) from compiled module structure.
   - decision samples that include observed boolean condition bindings and outcome.
6. Profile gates:
   - `symbol`: fail if any tracked export has zero hits.
   - `decision`: `symbol` gates + fail if any expected statement site has zero hits, or any expected decision site misses true/false branch coverage.
   - `mcdc`: `decision` gates + fail when condition independence is not demonstrated for every condition at each expected decision site.

## Evidence Artifact

The obligation MUST write a report artifact to the evidence store with:

- `:kind` = `"genesis/coverage-v0.2"`
- `:package` = package name (string)
- `:ok` = boolean
- `:profile` = `:symbol | :decision | :mcdc`
- `:definition` = string describing the tracked set rule
- `:exports` = vector of `{ :sym <symbol> :hits <int> }` for each tracked symbol
- `:missing` = vector of missing tracked symbols (those with `:hits = 0`)
- `:structural` map containing:
  - aggregate decision counters
  - expected site counts
  - per-site statement and decision coverage
  - per-site MC/DC condition status
  - vectors of missing statement/decision/MC/DC requirements
- `:tests` = per-test vector including symbol hits plus per-test statement/decision site coverage slices
- `:errors` = (optional) vector of strings

All terms MUST be in canonical CoreForm form.
