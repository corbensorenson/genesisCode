# GenesisCode Stacks (Levels 0–2) v0.2

Status: design guidance (treat as medium-confidence input; evolves with implementation reality).

GenesisCode is intentionally "layered". The language is not one monolith: features ship as **stacks**
that bundle:

- capability surface (effect ops + policies)
- obligations (what evidence must exist to accept/publish)
- syntax/IR transforms (desugaring) where applicable
- libraries (CoreForm modules and helper contracts)

This layering keeps the **kernel** small while still enabling an "end-to-end" experience quickly.

## Level 0 (Core / TCB-A)

Non-negotiable: small, deterministic, auditable.

Includes:

- CoreForm parsing/printing/canonicalization + hashing rules
- Gλ evaluator (pure)
- seals and the hardened protocol surface (UNHANDLED/EFFECT/ERROR as unforgeable sealed values)
- contracts: make/extend/dispatch/explain + protocol predicates
- effect IR constructors (`core/effect::{pure,perform,bind}`) and deterministic runner + `.gclog` replay

Rule: Level 0 must remain WASM-friendly and minimal; all non-determinism is outside the kernel.

## Level 1 (Foundation)

Goal: after Level 0 + Level 1, you can write real programs, run tests, and share/publish/install
packages without reaching for Git or an external package manager.

Level 1 is still "foundational" and should stabilize early.

Includes (target):

- Standard data layer utilities (`core/list`, `core/map`, `core/vec`, `core/bytes`, `core/str`, `core/sym`)
  with stable naming and conventions
- Message + contract convenience helpers so contract code is uniform across the ecosystem
- Effect programming toolkit conventions (`catch`, standard payload shapes, run/replay wrappers)
- Obligations + testing as first-class (unit tests + replayable tests as baseline)
- GenesisGraph + GenesisPkg "no Git / no pip" workflows:
  - store/refs/sync/vcs/pkg effects are the primitive capabilities
  - library wrappers and schemas make them convenient from GenesisCode

Principle: Level 1 should feel "complete enough" to build serious tooling and libraries, even if
some parts are initially implemented in Rust and then migrated to self-hosted GenesisCode.

## Level 2 (Batteries + Paradigms)

Goal: big subsystems and paradigms that build on Level 1.

Examples (target):

- class/trait/interface sugars (OO-as-contracts, row-extension ergonomics)
- type stack: row-polymorphic contract typing + effect rows (obligation-based)
- optimizer stack (e-graphs, translation validation)
- distributed/actor/CRDT stacks
- "DB" as an indexing/query layer over the content-addressed store (see below)
- AI co-development workflows (patch proposals, obligation-gated acceptance loops)

Rule: Level 2 should not be required to write and ship useful packages.

## "The Store Is The Database"

GenesisCode's content-addressed artifact store + refs + commits already form an append-only,
auditable substrate.

The recommended "database" direction is therefore:

- indexing and query over stored artifacts and commit graphs
- transactional semantics via policy-gated `refs::set` (commit/accept)
- optional secondary indices as additional artifacts (content-addressed)

This keeps the system "Genesis-y" (Merkle DAG + evidence), instead of bolting on an unrelated SQL
stack.

## Stacks As Packages (Recommended Packaging)

A stack itself should be a package (GenesisPkg) whose exports provide:

- stable library surface (`core/*` helpers or `stack/*` modules)
- policy defaults (as artifacts)
- obligation profiles (what must pass for dev/main/tags)

Suggested early naming:

- `stack/foundation` (Level 1)
- `stack/batteries` (Level 2)

These can start as Rust-bootstrapped "blessed packages" and later become self-hosted.

