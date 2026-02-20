# Foundation Standard Library (Level 1) v0.2

Status: fully implemented (v0.2). This doc is normative for the stable Foundation surface and is
locked by conformance tests + CI guards.

This document defines the *Level 1 (Foundation)* standard library surface. The point is not to be
"big", but to be **canonical** and **uniform** so higher-level stacks do not re-invent basics and
so AI-generated code converges.

Where possible, functions should be:

- deterministic and total
- pure (effects only via effect programs)
- explicit about bytes vs strings
- consistent about error handling (sealed `core/protocol::error`)

## 1) Data Layer

### `core/list::*` (pairs/nil)

Canonical list model:

- `nil` is the empty list
- lists are pairs ending in `nil`

Required utilities (target):

- `core/list::is-nil?` (already exists via kernel primitive wrapper)
- `core/list::len` (proper lists only; improper lists -> ERROR)
- `core/list::reverse`, `core/list::append`
- `core/list::map`, `core/list::filter`, `core/list::foldl`

### `core/map::*` (persistent ordered maps)

Required utilities:

- `core/map::get`, `core/map::put`, `core/map::merge`, `core/map::len`, `core/map::entries`

Design note:

- Map key ordering is canonicalized by CoreForm term ordering; do not invent ad-hoc order rules in
  libraries.

### `core/vec::*` (persistent vectors)

Required utilities:

- `core/vec::get`, `core/vec::push`, `core/vec::len`, `core/vec::set`

### `core/bytes::*` and `core/str::*`

Required utilities:

- explicit UTF-8 conversion: `core/str::to-utf8`, `core/str::from-utf8`
- bytes slicing/indexing: `core/bytes::{len,get,slice,concat}`
- string ops: `core/str::{len,concat,repeat,join}`
- hashing: `core/crypto::blake3` (pure primitive)

### `core/sym::*`

Required utilities:

- `core/sym::eq?`, `core/sym::to-str`

Future (needs more string primitives):

- parse/format qualified symbols (`pkg/module::op-name`)

## 2) Message + Contract Ergonomics

Required:

- `core/msg::{make,op,payload}` (already implemented as prelude intrinsics)
- `core/contract::{make,extend,dispatch,explain,meta,proto,shape}` (already implemented)

Recommended helpers (target):

- `core/contract::call` convenience wrapper:
  - `(core/contract::call c op payload) == (core/contract::dispatch c (core/msg::make op payload))`
- aliases for protocol predicates:
  - `core/contract::is-unhandled`, `core/contract::is-error`, `core/contract::is-effect`

Hooks (future, built on GenesisGraph):

- `core/contract::{blame,why}` that link behavior back to commits/evidence.

## 3) Effect Programming Toolkit

Level 0 defines the effect IR; Level 1 makes it usable and uniform.

Required:

- `core/effect::{pure,perform,bind}` (already implemented as prelude intrinsics)
- `core/effect::{map,then}` helpers (already in `prelude/prelude.gc`)

Recommended (target):

- `core/effect::catch` and `core/effect::catch-payload`:
  - standardize error-as-value handling for effect programs
- standard payload conventions (maps with keyword keys):
  - filesystem: `{:path "...", ...}`
  - store: `{:artifact <datum>}` / `{:hash "..."}`
  - refs: `{:name "...", :hash "...", :policy "...", :expected-old "...|nil"}`

## 4) Testing + Obligations as First-Class

Required baseline obligations for publishable packages:

- `core/obligation::unit-tests`
- `core/obligation::replayable-tests`
- `core/obligation::capabilities-declared`
- `core/obligation::determinism` (for declared-pure code)

Library targets:

- `core/test` helpers (assertions, test cases, and reports)
- property testing v0 (small, seed recorded as evidence)

## 5) GenesisGraph + GenesisPkg "No Git / No Pip"

The primitive capability surface is effect ops:

- `core/store::*`, `core/refs::*`, `core/sync::*`
- `core/vcs::*` (diff/apply/merge/log/etc)
- `core/pkg::*` (init/add/lock/install/update/export/import/publish/verify/etc)

Foundation should provide:

- stable schemas (docs)
- convenience wrappers in GenesisCode that construct these effect programs uniformly (implemented in `prelude/prelude.gc`)
- obligation profiles and policy defaults

## 6) Conformance lock (CI-gated)

The Foundation surface is locked by:

- `crates/gc_prelude/tests/prelude_foundation_stdlib_conformance.rs`
- `scripts/check_foundation_stdlib_conformance.sh`

CI and health gates execute this conformance suite via:

- `.github/workflows/ci.yml` (`Foundation Stdlib Conformance Guard`)
- `scripts/check_upgrade_plan_health.sh` (`COMMON_GATES`)
