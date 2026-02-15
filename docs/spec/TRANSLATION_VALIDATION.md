# Translation Validation Obligation v0.2

This document is normative for `core/obligation::translation-validation`.

## Goal

Translation validation provides a *tooling-grade* soundness check for the optimizer by comparing
observable outputs between:

- the original package modules
- the optimized package modules (as produced by `gc_opt`)

Refinement proofs are out of scope; translation validation is an evidence-producing obligation.

## Execution Model

Given a package `package.toml`:

1. Load and canonicalize package modules.
2. Optimize each module once using `gc_opt::optimize_module_with_report`.
3. Discover the package test ids (suite + case).
4. For each test id:
   - run test against original modules
   - run test against optimized modules
   - compare final value hashes
5. The obligation is `ok = true` iff all test ids have equal value hashes between original and
   optimized runs.

If any mismatch is observed, the obligation must be `ok = false` and include a human-readable error
entry describing the test id and the two hashes.

## Evidence Artifact Schema

On completion, the obligation must write a CoreForm term artifact in the evidence store with:

- `:kind` = `"genesis/translation-validation-v0.2"`
- `:package` = package name (string)
- `:ok` = bool
- `:modules` = vector of module records
- `:optimizer` = optimizer stats summary
- `:tests` = vector of per-test records (original + optimized hashes)
- `:errors` = vector of strings (may be empty)

### `:modules` entry

Each module record is a map:

- `:path` string (module path from manifest)
- `:orig-h` bytes(32) (module hash before optimization)
- `:opt-h` bytes(32) (module hash after optimization)
- `:changed` bool

### `:optimizer` record

The optimizer record is a map:

- `:egg-runs` int
- `:egg-iterations` int
- `:egg-eclasses` int
- `:egg-enodes` int
- `:egg-rewrites` vector of rewrite records

Each rewrite record is a map:

- `:name` string
- `:n` int

### `:tests` entry

Each per-test record is a map:

- `:suite` string
- `:test` string
- `:orig-h` bytes(32)
- `:opt-h` bytes(32)
- `:ok` bool

