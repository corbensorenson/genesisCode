# GenesisPkg + GenesisGraph CLI Spec v0.1

Exact commands, flags, lock snapshot format, and expected behavior.

## 1.0 Design principles

- No Git, no external package manager. Everything here is shipped with `genesis`.
- Deterministic by default: all operations that touch store/refs/sync are effects and must emit effect logs.
- Content-addressed: all artifacts referenced by hash; refs are names -> hashes.
- Obligation-gated ref updates: any command that advances a ref must either:
  - pass policy-required obligations automatically, or
  - refuse and print which obligations/evidence are missing.

## 2.0 Common CLI conventions

### 2.1 Hashes

- `HASH` is base32 or hex string. CLI accepts both.
- CLI prints hashes in a single canonical format (pick one: recommend base32 for readability).

### 2.2 Refs

Ref names are strings like:

- `refs/heads/main`
- `refs/heads/dev`
- `refs/tags/v1.2.0`
- `refs/pkgs/my-lib/heads/main`
- `refs/contracts/my-lib/counter::Counter/heads/dev`

### 2.3 Remote specs

Remote is a named config entry or inline URL-like spec:

- `--remote origin`
- `--remote gen://example.com/registry` (implementation-defined)

### 2.4 Output formats

All commands support:

- `--json`: machine-readable output (single JSON object)
- `--quiet`: print only the essential hash/result
- `--verbose`: include more diagnostics

### 2.5 Effect logs

All commands that invoke `store/*`, `refs/*`, or `sync/*` must:

- write an effect log file if `--log <path>` is provided
- otherwise write to a default location (see §7.2)
- print the log hash/path in output

## 3.0 Command groups

### 3.1 `genesis store` - local artifact store ops

#### `genesis store put`

Store an artifact in the local store.

Usage:

```text
genesis store put --in <file> [--kind <kind>] [--json] [--quiet]
```

Behavior:

- reads file bytes, validates artifact canonical encoding (or canonicalizes if allowed by policy)
- computes content hash
- stores under hash
- prints hash

#### `genesis store get`

```text
genesis store get <hash> --out <file> [--json]
```

#### `genesis store has`

```text
genesis store has <hash> [--json]
```

### 3.2 `genesis refs` - manage refs/branches/tags

#### `genesis refs list`

```text
genesis refs list [--prefix <refs/...>] [--json]
```

#### `genesis refs get`

```text
genesis refs get <refname> [--json]
```

#### `genesis refs set`

Advance a ref. Must be policy-gated.

```text
genesis refs set <refname> <commit-hash> \
  [--policy <policy-hash-or-name>] \
  [--log <path>] \
  [--json]
```

Behavior:

- load policy (default if not specified)
- verify commit exists; verify required obligations/evidence for that policy (see Policy Defaults doc)
- if pass: set ref to commit-hash and record effect log
- if fail: refuse; print missing obligations/evidence

#### `genesis refs delete`

```text
genesis refs delete <refname> [--policy <...>] [--log <path>]
```

### 3.3 `genesis vcs` - diff/apply/merge/log/blame/why

#### `genesis vcs hash`

Hash a canonical datum/artifact (for debugging).

```text
genesis vcs hash --in <file> [--json]
```

#### `genesis vcs diff`

Compute semantic patch between two snapshots.

```text
genesis vcs diff --base <snapshot-hash> --to <snapshot-hash> \
  --out <patchfile> [--json]
```

Behavior:

- load snapshots from store
- produce `:vcs/patch` artifact and write it (also store it unless `--no-store`)

#### `genesis vcs apply`

Apply patch to base snapshot.

```text
genesis vcs apply --base <snapshot-hash> --patch <patch-hash-or-file> \
  [--out <snapshotfile>] [--store] [--json]
```

Behavior:

- if conflicts, writes/returns a `:vcs/conflict` artifact (store it if `--store`)

#### `genesis vcs merge3`

3-way semantic merge.

```text
genesis vcs merge3 --base <snapshot-hash> --left <snapshot-hash> --right <snapshot-hash> \
  [--out <snapshotfile>] [--store] [--json]
```

Behavior:

- produces merged snapshot or conflict artifact

#### `genesis vcs log`

Walk commit DAG from a commit hash or ref.

```text
genesis vcs log <commit-hash-or-ref> [--max <n>] [--json]
```

#### `genesis vcs blame`

Attribute a symbol/path in a snapshot to the commit that introduced it.

```text
genesis vcs blame --snapshot <snapshot-hash> --sym <qualified-symbol> [--path <ast-path>] [--json]
```

#### `genesis vcs why`

Return "why" bundle: commit + rationale + evidence pointers + explain trace (if op).

```text
genesis vcs why --snapshot <snapshot-hash> --sym <qualified-symbol> [--op <qualified-op>] [--json]
```

### 3.4 `genesis commit` - create commits (core workflow)

Creates a commit object binding patch + obligations + evidence.

#### `genesis commit new`

```text
genesis commit new \
  --target-kind <package|module|contract|workspace> \
  --target-id <string> \
  --base <snapshot-hash-or-ref> \
  --patch <patch-hash-or-file> \
  --message <string> \
  [--why <string>] \
  [--obligation <obligation-sym> ...] \
  [--evidence <hash> ...] \
  [--author <string>] \
  [--sign <key-id>] \
  [--store] \
  [--json]
```

Behavior:

- apply patch to base snapshot (pure)
- if conflict: refuse (or `--allow-conflict` writes conflict artifact)
- create `:vcs/commit` artifact with parents:
  - if `--base` is a ref: parent is commit at that ref
  - if `--base` is a snapshot hash with no commit context: parent list empty unless provided
- store commit artifact if `--store`
- does not advance refs; that is `refs set` / `pkg publish` policy step

#### `genesis commit show`

```text
genesis commit show <commit-hash> [--json]
```

### 3.5 `genesis pkg` - pip-like package operations

#### 3.5.1 Workspace assumptions

All `pkg` commands operate on a workspace rooted at current directory (or `--workspace <path>`).
Workspace maintains:

- `genesis.lock` (lock snapshot)
- optional `genesis.workspace` config

#### `genesis pkg init`

```text
genesis pkg init [--name <workspace-name>] [--policy <name-or-hash>]
```

Creates:

- `genesis.lock` (empty deps, policy refs)
- `genesis.workspace` config (optional)

#### `genesis pkg add`

```text
genesis pkg add <pkg-name>@<ref-or-commit-or-tag> \
  [--update-policy <manual|auto>] \
  [--registry <remote>] \
  [--json]
```

Stores in lock file as requirement with desired selector.

#### `genesis pkg lock`

```text
genesis pkg lock [--update] [--remote <registry>] [--log <path>] [--json]
```

Behavior:

- pull required refs from registry if needed (effect)
- resolve requirements to specific commit + snapshot
- write updated `genesis.lock`
- record effect log

#### `genesis pkg install`

```text
genesis pkg install [--frozen] [--log <path>] [--json]
```

Behavior:

- if `--frozen`, refuses if lock out-of-date
- fetch missing artifacts from registries/remotes (effect)
- verify policy-required obligations for installed packages (strictness configurable)
- record effect log

#### `genesis pkg update`

```text
genesis pkg update [<pkg-name> ...] [--log <path>] [--json]
```

Behavior:

- for deps with `update-policy:auto` and ref selectors: pull latest head, update lock, re-verify

#### `genesis pkg info`

```text
genesis pkg info <pkg-name> [--json]
```

#### `genesis pkg export`

```text
genesis pkg export <pkg-name> \
  --ref <ref-or-commit-or-snapshot> \
  --out <file.gpk> \
  [--shallow|--full] \
  [--depth <n>] \
  [--include-evidence <required|all|none>] \
  [--include-deps <none|locked|all>] \
  [--json]
```

Defaults:

- `--shallow`
- evidence: `required`
- deps: `locked`

#### `genesis pkg import`

```text
genesis pkg import <file.gpk> \
  [--set-ref <refname>=<hash> ...] \
  [--log <path>] \
  [--json]
```

#### `genesis pkg publish`

```text
genesis pkg publish <pkg-name> \
  --remote <registry> \
  --ref <refname> \
  [--policy <name-or-hash>] \
  [--log <path>] \
  [--json]
```

Behavior:

- ensure commit at ref satisfies publish policy
- push reachable artifacts required by policy (commit, snapshots, patches, evidence)
- advance remote ref via `refs set` on remote

#### `genesis pkg verify`

```text
genesis pkg verify <pkg-name>@<commit-or-ref> \
  [--policy <name-or-hash>] \
  [--json]
```

### 3.6 `genesis policy` - manage local policies

#### `genesis policy list`

```text
genesis policy list
```

#### `genesis policy show`

```text
genesis policy show <name-or-hash> [--json]
```

#### `genesis policy set-default`

```text
genesis policy set-default <name-or-hash>
```

## 4.0 Lock snapshot format (`genesis.lock`)

`genesis.lock` is deterministic, describes workspace state:

- `requirements`: desired tracking (ref/commit/tag)
- `locked`: resolved immutable commit+snapshot used for builds

See `docs/LOCK_GENERATOR_RULESET_v0.1.md` for invariants and generation rules.

## 5.0 `.gpk` bundle format (minimal spec)

v0.1 tooling ships `.gpk` **v1** bundles with a simple binary format:

- header:
  - magic bytes `GPK\\0`
  - `version` (u32 little-endian) = `1`
  - `root` (32-byte BLAKE3 hash of the root snapshot)
  - `count` (u64 little-endian)
- index: `count` fixed-size entries, in the same order as payload:
  - `hash` (32 bytes)
  - `kind` (u8). v1 uses `0` for "raw canonical artifact bytes".
  - `reserved` (7 bytes, zero)
  - `offset` (u64 little-endian, absolute file offset to payload bytes)
  - `length` (u64 little-endian)
- payload:
  - concatenated `length` bytes for each entry, in index order

Notes:

- v1 has no trailing sections (no embedded refs/attestations). Extra trailing bytes are treated as corruption.
- Future extensions (embedded refs, attestations, compression) must bump `version`.

## 6.0 Exit codes (for scripting)

- `0` success
- `2` policy/obligation failure
- `3` conflict produced
- `4` missing artifact / fetch failure
- `5` invalid format / parse error
- `6` internal error

## 7.0 Default file locations

### 7.1 Local store

- `./.genesis/store/` (workspace-local)
- optional global store overlay `~/.genesis/store/` (future)

### 7.2 Effect logs

- default: `./.genesis/logs/<timestamp-or-seq>.elog`
- override with `--log <path>`

## 8.0 MVP implementation order

1. `store put/get/has`
2. `refs get/set/list`
3. `vcs diff/apply/merge3/log`
4. `pkg init/add/lock/install`
5. `pkg export/import` (shallow only)
6. `policy show/list/set-default`
7. `pkg publish` + `sync push/pull` (remote later)
