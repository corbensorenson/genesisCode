# GenesisPkg + GenesisGraph CLI Spec v0.1

Exact commands, flags, lock snapshot format, and expected behavior.

## 1.0 Design principles

- No Git, no external package manager. Everything ships with `genesis`.
- Deterministic by default: operations touching store/refs/sync are effects and must emit effect logs.
- Content-addressed: artifacts referenced by hash; refs are names -> hashes.
- Obligation-gated ref updates: any command that advances a ref must pass policy-required obligations or refuse.

## 2.0 Common CLI conventions

### 2.1 Hashes

- CLI accepts base32 or hex. CLI prints a single canonical format (recommend base32 for readability).

### 2.2 Refs

Examples:

- `refs/heads/main`
- `refs/tags/v1.2.0`
- `refs/pkgs/my-lib/heads/dev`
- `refs/contracts/my-lib/counter::Counter/heads/dev`

### 2.3 Remote specs

- `--remote origin`
- `--remote gen://example.com/registry` (implementation-defined)

### 2.4 Output formats

- `--json` single JSON object
- `--quiet` essential hash/result only
- `--verbose` include more diagnostics

### 2.5 Effect logs

All commands invoking store/refs/sync must:

- write an effect log if `--log <path>` is provided
- otherwise write to a default location

## 3.0 Command groups

### 3.1 `genesis store`

- `genesis store put --in <file> [--kind <kind>]`
- `genesis store get <hash> --out <file>`
- `genesis store has <hash>`

### 3.2 `genesis refs`

- `genesis refs list [--prefix <refs/...>]`
- `genesis refs get <refname>`
- `genesis refs set <refname> <commit-hash> [--policy <...>] [--log <path>]`
- `genesis refs delete <refname> [--policy <...>] [--log <path>]`

### 3.3 `genesis vcs`

- `genesis vcs hash --in <file>`
- `genesis vcs diff --base <snapshot-hash> --to <snapshot-hash> --out <patchfile>`
- `genesis vcs apply --base <snapshot-hash> --patch <patch-hash-or-file> [--out <file>] [--store]`
- `genesis vcs merge3 --base <snap> --left <snap> --right <snap> [--out <file>] [--store]`
- `genesis vcs log <commit-hash-or-ref> [--max <n>]`
- `genesis vcs blame --snapshot <snap> --sym <qualified-symbol> [--path <ast-path>]`
- `genesis vcs why --snapshot <snap> --sym <qualified-symbol> [--op <qualified-op>]`

### 3.4 `genesis commit`

- `genesis commit new --target-kind <package|module|contract|workspace> --target-id <string> --base <snapshot-hash-or-ref> --patch <patch-hash-or-file> --message <string> [--why <string>] [--obligation <sym> ...] [--evidence <hash> ...] [--author <string>] [--sign <key-id>] [--store]`
- `genesis commit show <commit-hash>`

### 3.5 `genesis pkg`

Workspace state:

- `genesis.lock` (lock snapshot)
- optional `genesis.workspace`

Commands:

- `genesis pkg init [--name <workspace-name>] [--policy <name-or-hash>]`
- `genesis pkg add <pkg>@<ref-or-hash> [--update-policy <manual|auto>] [--registry <remote>]`
- `genesis pkg lock [--strict] [--update] [--remote <registry>] [--log <path>]`
- `genesis pkg install [--frozen] [--log <path>]`
- `genesis pkg update [<pkg-name> ...] [--log <path>]`
- `genesis pkg info <pkg-name>`
- `genesis pkg abi --pkg <package.toml>`
- `genesis pkg export <pkg-name> --ref <ref-or-commit-or-snapshot> --out <file.gpk> [--shallow|--full] [--depth <n>] [--include-evidence <required|all|none>] [--include-deps <none|locked|all>]`
- `genesis pkg import <file.gpk> [--set-ref <refname>=<hash|nil>[@<expected-old|nil>] ...] [--policy <policy-hash>] [--log <path>]`
- `genesis pkg publish <pkg-name> --remote <registry> --ref <refname> [--policy <name-or-hash>] [--log <path>]`
- `genesis pkg verify <pkg-name>@<commit-or-ref> [--policy <name-or-hash>]`

### 3.6 `genesis policy`

- `genesis policy list`
- `genesis policy show <name-or-hash>`
- `genesis policy set-default <name-or-hash>`

## 4.0 Lock snapshot format (`genesis.lock`)

Format: TOML.

Canonical ordering:

1. `version`
2. `workspace`
3. `policy`
4. `[registries]` sorted by name
5. `[requirements]` sorted by package name
6. `[locked]` sorted by package name
7. `[artifacts]` optional

Example:

```toml
version = 1
workspace = "my-app"
policy = "policy:default-v0.1"

[registries]
default = "gen://registry.example.com"

[requirements]
"my-lib" = { selector = "refs/heads/main", update_policy = "auto", registry = "default" }

[locked]
"my-lib" = { commit = "h:...", snapshot = "h:..." }
```

## 5.0 `.gpk` bundle format (minimal)

- Header: magic bytes + version
- Index: entries `hash -> {offset, length, kind}`
- Payload: canonical bytes for each artifact
- Optional: embedded refs section (name -> hash)
- Optional: attestations

Shallow bundles include snapshot closure plus required evidence.
Full bundles include commit history objects per policy and depth.

## 6.0 Exit codes

- `0` success
- `2` policy or obligation failure
- `3` conflict produced
- `4` missing artifact or fetch failure
- `5` invalid format or parse error
- `6` internal error (non-panicking)
