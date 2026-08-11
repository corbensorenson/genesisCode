# GenesisCode v0.2 Patch Schema (Normative)

Semantic patches are stored as a single canonical CoreForm term in a `.gcpatch` file.

## Top-level

Top-level term is a map with keys:

- `:version` (int)  
  - must be `1` for v0.2
- `:intent` (string)
- `:provenance` (map)  
  - freeform; used for authorship/tooling metadata
- `:ops` (vector)  
  - vector of op maps

## Op maps

Each op is a map and must include:

Every module path field (`:module-path`, `:from-module-path`, and `:to-module-path`) is a
portable package-relative path. It must be Unicode NFC, use `/` separators, and contain no
empty, `.`, `..`, root, or drive-prefix component. Implementations must reject an invalid path
before reading or mutating package state.

Before opening the evidence store or mutating package state, production apply invokes the
GenesisCode-authoritative `core/cli::patch-preflight` binding. Its closed request contains exactly
`:kind`, `:patch`, `:path-states`, `:profile`, and `:v`, with fixed values
`genesis/patch-preflight-request-v0.1`, `genesis/patch-authority-v0.1`, and `1`. `:patch` is the
normalized semantic patch. `:path-states` is the canonical path-sorted vector of all
operation-referenced paths, where each closed record is `{:path string :state string}` and state is
one of `absent`, `file`, or `other`. The host boundary rejects symlinks in any inspected component
instead of representing them as an admissible state.

The closed report contains exactly `:checks`, `:conflict`, `:final-path-states`, `:kind`, `:ok`,
`:patch-h`, `:path-states-h`, `:profile`, and `:v`. Its kind is
`genesis/patch-preflight-v0.1`; its two hashes bind the normalized patch and input snapshot.
GenesisCode evaluates operations in vector order. Existing-module edits and removals require
`file`; additions require `absent`; moves and splits require a `file` source followed by an
`absent` target. Successful transitions update the virtual state before the next operation. A
failure stops at the first unsatisfied check and emits the closed conflict record
`{:actual string :code "patch/path-state-conflict" :expected string :op symbol :ordinal int :path string}`.
The host validates complete ordered check coverage and fails closed on malformed, incomplete,
unbound, or identity-mismatched reports; it never retries with Rust precondition semantics.

- `:op` (symbol) one of:
  - `:replace-node`
  - `:replace-node-id`
  - `:add-module`
  - `:remove-module`
  - `:update-manifest`
  - `:rename-symbol`
  - `:move-module`
  - `:split-module`
  - `:rewrite-imports`
  - `:rewrite-exports`
  - `:migrate-contract-signature`

### `:replace-node`

Required keys:

- `:module-path` (string) path relative to the package directory (the directory containing `package.toml`)
- `:path` (vector) path steps (see below)
- `:new` (term) the replacement CoreForm term

### `:replace-node-id`

Required keys:

- `:module-path` (string) path relative to the package directory (the directory containing `package.toml`)
- `:node-id` (string) stable semantic node identifier for the target node
- `:new` (term) the replacement CoreForm term

Semantics:

- `:replace-node-id` is resolved against the module's canonicalized CoreForm AST.
- The runtime computes the node path deterministically from `:node-id`, applies the same structural replacement semantics as `:replace-node`, and re-canonicalizes before writing.

### `:add-module`

Required keys:

- `:module-path` (string)
- `:content` either:
  - a string containing `.gc` source, or
  - a vector of CoreForm forms (module top-forms)

### `:remove-module`

Required keys:

- `:module-path` (string)

### `:update-manifest`

Supported keys:

- `:set` (map)  
  - keys are symbol field names (e.g. `:caps_policy`, `:name`, `:version`)  
  - values are CoreForm terms converted to TOML conservatively
- `:obligations-add` (vector of symbols)
- `:obligations-remove` (vector of symbols)
- `:tests-add` (vector of symbols)
- `:tests-remove` (vector of symbols)
- `:caps-policy` (string) convenience for setting the manifest `caps_policy` field

### `:rename-symbol`

Required keys:

- `:module-path` (string)
- `:from` (symbol or string)
- `:to` (symbol or string)

Semantics:

- Applies deterministic symbol-level rewrite across the canonical module term tree.
- Fails if no rewrite sites are found.

### `:move-module`

Required keys:

- `:from-module-path` (string)
- `:to-module-path` (string)

Semantics:

- Moves the module file and rewrites `package.toml` module path entry.
- Fails if source is missing, target exists, or manifest does not contain source path.

### `:split-module`

Required keys:

- `:from-module-path` (string)
- `:to-module-path` (string)
- `:symbols` (non-empty vector of symbols or strings)

Semantics:

- Extracts matching top-level `(def <symbol> ...)` forms from source module into new target module.
- Rewrites source/target modules in canonical form and appends target path to manifest module list.

### `:rewrite-imports` / `:rewrite-exports`

Required keys:

- `:module-path` (string)

Optional keys:

- `:add` (vector of symbols or strings)
- `:remove` (vector of symbols or strings)
- `:replace` (vector of symbols or strings)

Semantics:

- Rewrites `::meta` map list field (`:imports` or `:exports`) deterministically.
- `:replace` seeds full list; `:remove` then `:add` are applied set-wise.

### `:migrate-contract-signature`

Required keys:

- `:module-path` (string)
- `:contract-symbol` (symbol or string)
- `:from-param` (symbol or string)
- `:to-param` (symbol or string)

Semantics:

- Targets `(def <contract-symbol> (fn (...) ...))`.
- Renames first function parameter from `:from-param` to `:to-param` and rewrites in-scope body references with lexical shadowing respected for nested `fn`/`let`.
- Fails if target contract/function shape is not found.

## Path encoding (for `:replace-node`)

`:path` is a vector of steps; each step is a vector where the first element is a tag symbol:

- `[:form i]`  
  - select the i-th top-level form in the module (0-indexed)
- `[:pair-car]` / `[:pair-cdr]`  
  - descend through a CoreForm pair/list node
- `[:vec i]`  
  - select i-th element of a vector (0-indexed)
- `[:map key_term]`  
  - select the value at `key_term` in a map

All patch application happens against the module’s canonicalized CoreForm, and the result is re-canonicalized before writing.

## Canonical normalization and identity

Before execution, production must call the artifact-loaded GenesisCode binding
`core/cli::patch-normalize`. Its closed request contains exactly:

- `:kind` = `"genesis/patch-normalize-request-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:patch` = the source patch term
- `:v` = `1`

The source patch and every operation map are closed schemas. Unknown fields,
missing required fields, unknown operations, wrong field types, empty required
identifiers, and unsupported versions fail closed. Normalization:

- preserves operation vector order as the only execution order
- converts symbol-or-string identifier fields to non-empty strings
- converts set-valued vectors to canonical sorted, duplicate-free string vectors
- emits every optional field explicitly as its normalized value, `[]`, or `nil`
- preserves `:intent`, `:provenance`, replacement terms, paths, and module content

The closed response contains exactly `:kind`, `:normalized-patch`, `:ok`,
`:op-identities`, `:patch-h`, `:profile`, `:source-patch-h`, and `:v`.
Fixed values are:

- `:kind` = `"genesis/patch-normalize-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:ok` = `true`
- `:v` = `1`

`source-patch-h` is `hash-term(source patch)`. `patch-h` is
`hash-term(normalized patch)`. `:op-identities` is an exact ordered vector of
closed `{:ordinal int :op-h hex64}` maps, where `op-h` is the normalized
operation's `hash-term` identity. The host decoder must bind all hashes,
ordinals, field sets, types, and operation count to the request and normalized
term before execution. It may decode the accepted closed plan into host
execution types but cannot accept, repair, reorder, or synthesize semantic
fields. Rejected, missing, malformed, resource-exhausted, or unbound authority
output has no production Rust fallback.

The patch artifact stored by patch application is the normalized patch term.
Its evidence-store address is the unprefixed BLAKE3 hash of canonical stored
bytes and is distinct from the domain-separated semantic `patch-h`. Apply
reports carry `:patch-h`, `:source-patch-h`, and the complete ordered
`:op-identities` vector.

## Replay-Aware Evidence

Patch apply reports include deterministic per-op evidence entries. For high-level refactor ops, entries include:

- operation symbol (`:op`)
- target module path
- before/after module hashes (`:before-module-h`, `:after-module-h`)
- structured op-specific detail map (`:detail`)

## Stable Node IDs

Node IDs are deterministic and path-derived:

- Traverse canonical module forms in deterministic order:
  - top-level forms by index
  - pairs via `:pair-car` then `:pair-cdr`
  - vectors by index
  - maps by canonical key order
- For each node path, compute:
  - `node-id = blake3("GCv0.2\\0semantic-node-id\\0" || module-path || "\\0" || print(path-term))`

This ensures agentic patch targeting is stable for unchanged structure and independent of source formatting.

### Node-index authority protocol

Production node indexing and node-ID resolution are authoritative only through the
artifact-loaded GenesisCode binding `core/cli::patch-semantic-node-index`. The
request is a closed map with exactly these fields:

- `:kind` = `"genesis/patch-node-index-request-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:module-path` = a non-empty package-relative module path string
- `:forms` = the canonical module-form vector
- `:v` = `1`

The binding returns a closed report map with exactly `:kind`, `:module-h`,
`:module-path`, `:nodes`, `:ok`, `:profile`, and `:v`. Its fixed values are:

- `:kind` = `"genesis/patch-node-index-v0.1"`
- `:profile` = `"genesis/patch-authority-v0.1"`
- `:ok` = `true`
- `:v` = `1`

`:module-h` is the canonical module hash. `:nodes` is the complete preorder
inventory described above. Every node record is a closed map with exactly:

- `:module-path` and `:node-id`
- `:path` and its canonical `:path-repr`
- `:term-h` and `:term-tag`

The host boundary must reject unknown fields, missing or reordered inventory,
request/report path or module-h disagreement, duplicate or non-lowercase-hex64
node IDs, and term hash/tag disagreement. It validates report binding but does
not independently recompute node IDs. The Rust node-ID/index implementation is
compiled only by the `gc_patches/parity-oracle` feature and is not a production
fallback. A rejected, missing, malformed, or resource-exhausted GenesisCode
authority result fails closed.
