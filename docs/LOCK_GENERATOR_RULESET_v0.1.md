# Default `genesis.lock` Generator Ruleset v0.1

What gets written when, how locking works, and update invariants.

## 1.0 Goals

- `genesis.lock` is the deterministic record of dependency resolution.
- Supports tracking refs while pinning exact commits.
- Stable ordering: same inputs -> same file bytes.

## 2.0 Canonical ordering

Writer must emit keys in this order:

1. `version`
2. `workspace`
3. `policy`
4. `[registries]` sorted
5. `[requirements]` sorted
6. `[locked]` sorted
7. `[artifacts]` optional

## 3.0 Command semantics

### 3.1 `genesis pkg init`

Writes empty lock file with workspace identity, default policy, and registries.

### 3.2 `genesis pkg add`

Adds or updates a requirement only (does not resolve unless explicitly asked).

### 3.3 `genesis pkg lock`

Resolves requirements to exact commits and snapshots. May pull refs/artifacts if configured.

Never writes timestamps or nondeterministic fields.

### 3.4 `genesis pkg install`

Installs according to `[locked]`. Does not rewrite the lock file by default.

### 3.5 `genesis pkg update`

For deps with `update_policy=auto` and ref selectors, pulls latest and advances the locked commit/snapshot.

## 4.0 Selectors

- `commit:<hash>` exact pin
- `snapshot:<hash>` snapshot-only pin (discouraged)
- `ref:<refname>` track a ref (or shorthand `<refname>`)

## 5.0 Resolution algorithm (deterministic)

For each requirement:

1. determine registry remote
2. resolve selector
3. fetch commit and result snapshot
4. perform basic verification
5. write `[locked]` entry

## 6.0 Invariants

`pkg install --frozen` refuses if:

- any requirement is missing from locked
- any locked artifact is missing and cannot be fetched
- lock version mismatch
