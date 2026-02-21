> Bundle Entry: `docs/spec/TESTING_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# Unit Test Suite Schema v0.2

GenesisCode v0.2 unit tests are discovered by evaluating suites listed in `package.toml` under `tests = [...]`.

## Suite Value

Each suite symbol must evaluate to a *map* whose keys are test names (strings) and values are test specs.

Example:
```lisp
(def pkg/basic::tests
  {
    "adds"
      {:body (fn (_) (core/effect::pure (prim int/add 1 2)))
       :expect 3}})
```

## Test Spec

A test spec is either:
- A callable (closure/native fn): treated as the body.
- A map with:
  - `:body` (required): callable. Called with one argument (currently `nil`).
  - `:expect` (optional): datum. If present, the test must return an equal datum.

The body may return:
- A pure datum/value (pass-through)
- An effect program (executed via the capability runner when allowed)

## Normative Behavior

- Any non-callable test spec is an error.
- If `:expect` is present, equality is structural over data.
- If a test uses effects, its run must emit a `.gclog` and `core/obligation::replayable-tests` must replay it successfully.

