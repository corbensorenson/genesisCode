# Default `genesis.lock` Generator Ruleset v0.1

What gets written when, how locking works, and update invariants.

## 1.0 Goals

- `genesis.lock` is the authoritative, deterministic record of dependency resolution for a workspace.
- It supports tracking refs (branches/tags) while pinning exact commits for reproducibility.
- It is stable: same inputs -> same file bytes (modulo ordering/format).

## 2.0 Lock file format (baseline)

TOML, with stable ordering.

### 2.1 Canonical ordering

Generator must always write keys in stable order:

1. `version`
2. `workspace`
3. `policy`
4. `[registries]` (sorted by name)
5. `[requirements]` (sorted by pkg name)
6. `[locked]` (sorted by pkg name)
7. `[artifacts]` (optional, stable key order)

## 3.0 Lock generator state machine (commands)

### 3.1 `genesis pkg init`

Writes:

- `version = 1`
- `workspace = <name>`
- `policy = <default policy alias>` (e.g. `policy:default-v0.1`)
- `[registries] default = <remote spec or empty>`
- empty `[requirements]`
- empty `[locked]`

### 3.2 `genesis pkg add <pkg>@<selector>`

Adds/updates a requirement:

`[requirements."<pkg>"] = { selector = "...", update_policy = "...", registry = "..." }`

Does not modify `[locked]` unless `--lock` is passed.

### 3.3 `genesis pkg lock`

Resolves requirements into pinned commits/snapshots and writes `[locked]`.

May pull missing refs/artifacts from registries (effect logged).

### 3.4 `genesis pkg install`

Installs according to `[locked]`. Does not rewrite lock by default.

### 3.5 `genesis pkg update [pkg...]`

For deps where `update_policy=auto` and selector is a ref:

- pull latest ref head
- update `[locked]` to new commit/snapshot
- re-verify policy-required obligations (at least basic)

## 4.0 Selectors (requirement syntax)

Requirement selector is one of:

### 4.1 Commit pin

`selector = "commit:<hash>"`

### 4.2 Snapshot pin (discouraged)

`selector = "snapshot:<hash>"`

### 4.3 Ref tracking

`selector = "ref:refs/heads/main"` or shorthand `selector = "refs/heads/main"`

### 4.4 Tag tracking

Tags are refs: `selector = "ref:refs/tags/v1.2.0"`

## 5.0 Resolution algorithm (deterministic)

Given `(pkg, selector, registry)`:

1. determine registry remote (explicit or `[registries].default`)
2. resolve selector:
   - commit pin: use commit hash
   - snapshot pin: use snapshot hash (commit optional)
   - ref/tag: fetch `refs/get(ref)` -> commit hash
3. fetch required artifacts:
   - commit object (if commit known)
   - result snapshot
4. verify minimal acceptance (basic by default):
   - commit exists and parses
   - snapshot reachable
5. write `[locked]` entry

Determinism rule: if the same remote ref heads are observed and store contents are identical, lock
bytes must be identical.

## 6.0 Verification timing knobs (v0.1 defaults)

- `pkg lock`: basic verification
- `pkg install`: basic by default, strict in CI (configurable)

Optional:

- `genesis pkg lock --strict`: verify policy obligations/evidence before writing lock
- `genesis pkg install --strict`: replay logs / re-run tests (if configured)

## 7.0 Minimum `[locked]` fields

Minimum:

- `commit` (unless snapshot-only)
- `snapshot`
- `registry`
- `source_selector`

Recommended:

- `resolved_ref` (if applicable)
- `exports_hash`
- `caps_hash`
- `obligations_hash`
- `attestation_hash` (if signatures required)

Example:

```toml
[locked]
"my-lib" = {
  commit = "h:ABCD...",
  snapshot = "h:EFGH...",
  registry = "default",
  source_selector = "refs/heads/main",
  resolved_ref = "refs/heads/main",
  exports_hash = "h:....",
  obligations_hash = "h:...."
}
```

## 8.0 Invariants and refusal conditions

### 8.1 `pkg install --frozen`

Refuses if:

- any requirement missing from locked
- any locked artifact missing and cannot be fetched
- lock format version mismatch

### 8.2 Tag policy strictness

If policy requires signatures for tags, `pkg lock --strict` must refuse if commit lacks required
attestation evidence.

