> Deprecated Top-Level Doc: Use `docs/DEPRECATION_MAP_v0.1.md` for canonical replacements.

# Registry Object Reachability Rules v0.1

What "reachable" means for shallow export, full export, and policy-gated publish.

Goal: prevent missing-object bugs by making inclusion closures precise and testable.

## 0) Definitions

### 0.1 Artifact kinds

Artifacts are canonical datums with `:type` and schema `:v`:

- `:vcs/commit`
- `:vcs/snapshot` (`:kind` = `:package|:module|:contract|:workspace`)
- `:vcs/patch`
- `:vcs/evidence`
- `:vcs/attestation`
- `:vcs/conflict` (not publishable)
- optionally `:vcs/policy`
- optionally `:vcs/exports`

### 0.2 Root pointers

Operations start from one of:

- commit root: `<commit-hash>`
- ref root: `<refname>` (resolves to commit)
- snapshot root: `<snapshot-hash>` (shallow export only, unless commit provided)

### 0.3 Closure operator

`Reach(mode, root, policy)` returns a set of hashes to include by following references inside
artifacts (hash fields), never by guessing.

Safety constraints:

- `:vcs/conflict` must never be considered publishable
- missing required referenced artifacts: operation fails (or fetch if allowed)

## 1) Canonical reference fields (normative)

### 1.1 Commit references

Commit artifact fields:

- `:parents` -> commit hashes
- `:base` -> snapshot hash (optional)
- `:patch` -> patch hash
- `:result` -> snapshot hash
- `:evidence` -> evidence hashes
- `:attestations` -> attestation hashes (optional)

### 1.2 Snapshot references

References depend on `:kind`.

Package snapshot references:

- `:members` map qualified symbol -> hash
- optional `:exports_hash` (exports artifact)
- `:deps` entries may include `:dep/commit` and `:dep/snapshot` (locked form)
- optional provenance hashes `:prov/*`

Module snapshot references:

- `:defs` map symbol -> hash
- optional exports hash
- deps/prov as above

Contract snapshot references:

- optional `:proto` hash
- `:overrides` map op -> handler hash
- provenance hashes

Workspace snapshot references:

- `:modules` map module-name -> module snapshot hash
- optional lock snapshot hash
- deps/prov as above

### 1.3 Patch references

Patch references include any `:value` hashes in ops (replace/insert/etc).

### 1.4 Evidence references

Evidence artifacts may reference:

- `inputs[]`, `outputs[]`
- `data` hash (or inline small payload)

### 1.5 Attestation references

Attestations may reference:

- signed commit hash
- signer public key hash (optional)
- signature blob hash (optional)

## 2) Reachability modes

### Mode A: Shallow export

Goal: export a usable snapshot without history.

Shallow reach includes:

1. snapshot hash
2. snapshot member closure (`:members` / `:defs` / `:modules` / `:overrides`, etc.)
3. optionally `:exports_hash`
4. optionally required evidence artifacts if policy requires bundling for install verification
5. optionally deps/prov depending on flags/policy

Shallow export does not include:

- parent commits
- patches
- commit DAG history

### Mode B: Full export (history baked in)

Goal: export enough data to do branch/merge workflows offline.

Full reach includes:

1. commit `C`
2. `C.result` snapshot + snapshot closure
3. `C.patch` + patch closure
4. `C.evidence` + evidence closure per policy
5. `C.attestations` if present/required
6. parent commits up to depth (or all)
7. optionally embedded refs mapping names -> hashes

## 3) Publish reachability is policy-dependent

Publishing is: push required reachable artifacts, then advance remote ref.

Publish reach always includes:

1. commit `C`
2. `C.result` snapshot + snapshot closure (runnable state)
3. `C.patch` + patch closure

Then include:

- policy-required evidence closure
- policy-required attestations/signatures
- dependencies: either presence-check on remote, or upload if vendor mode/flagged

Recommendation: default publish uploads only what consumers need to install deterministically; full
history export is optional.

## 4) Evidence inclusion rules (policy matrix)

Dev:

- required: unit test evidence (if claimed)
- optional: effect logs

Main:

- required: unit tests, effect logs, replay verification evidence, capability audit evidence

Tags:

- required: all main evidence + dependency lock evidence + signatures/attestations

## 5) Dependency inclusion rules

Shallow export:

- default: do not include deps
- optional: include deps (locked/all)

Full export:

- default: include package history; deps optional

Publish:

- default: do not upload deps unless `--vendor` or `--include-deps`
- but policy may require deps to be pinned and presence-checked

## 6) Implementation algorithm (pure closure planner)

Implement a pure function:

`closure(start_hashes, include_predicate) -> set<Hash>`

Where `include_predicate` is mode/policy-driven to decide whether to chase:

- evidence graphs
- provenance graphs
- dependency graphs
- parent commit history

Use this closure planner for:

- export planning
- import verification
- publish push planning
- garbage collection root marking

## 7) Required tests

- shallow export completeness (snapshot + members + exports)
- full export completeness (commit DAG + patches + evidence)
- publish refusal when policy-required evidence missing
- dependency inclusion behavior
- attestation inclusion for tags
