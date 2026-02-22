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
