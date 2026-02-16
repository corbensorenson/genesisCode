# Genesis Level 2 Graphics Architecture (v0.1)

This document locks the core architecture for the GenesisCode Level 2 graphics stack.

Goals:
- One deterministic graphics model for 2D UI, 3D scenes, and hybrid apps.
- Stable CoreForm data shapes for scene/frame submissions.
- Extensible plugin model via contracts (passes, components, asset codecs).

Non-goals:
- Kernel-side rendering behavior.
- Hidden host state outside capability effects and logs.

## Canonical architecture

The Level 2 stack has four layers:

1. `gfx/data` (pure): scene graph, frame graph, materials, render settings.
2. `gfx/runtime` (pure planners): culling, batching, pass planning, dependency sorting.
3. `gfx/backend` (effect wrappers): `gfx/gpu::*`, `gfx/window::*`, `gfx/input::*`, `gfx/time::*`.
4. `gfx/ui` (pure + effects): layout, vector/text primitives, accessibility semantics, event routing.

All layer boundaries exchange canonical CoreForm maps/vectors so hashes are stable.

## Data model

`crates/gc_gfx` is the normative schema anchor for Rust-side host/tooling interop.
GenesisCode-side data must match equivalent shapes and key semantics.

### Scene data

- Scene root:
  - `:name` string
  - `:root-nodes` vector of node indices
  - `:nodes` vector of node maps
- Node fields:
  - `:name`
  - `:transform` with integer fixed-point translation/rotation/scale
  - optional `:mesh`, `:material`, `:camera`, `:light`
  - `:children` vector of node indices

### Frame graph data

- Frame graph:
  - `:render-passes` vector
  - `:compute-passes` vector
- Render pass:
  - `:label`
  - `:color-attachments` vector of resource ids
  - optional `:depth-attachment`
  - `:commands` vector of draw commands
- Compute pass:
  - `:label`
  - `:commands` vector of compute commands

Resource references are opaque ids allocated by `gfx/gpu::*` capabilities.

### Assets as GenesisGraph artifacts

Asset payloads are immutable content-addressed artifacts.
Required asset classes:
- mesh geometry
- textures/images
- shader sources/IR
- fonts/shaping tables

Each runtime reference must carry artifact hash provenance in metadata for blame/why.

## ECS + scene graph interop

Canonical authoring model is scene graph. ECS is an execution/indexing view.

Rules:
- Scene graph is source-of-truth snapshot format.
- ECS views are deterministic derivations from scene graph snapshots.
- ECS-only state must either:
  - be derivable from scene snapshot, or
  - be explicit runtime state that never mutates source snapshots.

## Render graph planning

Frame planning is a pure function:
- Input: scene snapshot + camera set + render settings + plugin registrations.
- Output: frame graph term with explicit pass/resource dependencies.

Planning constraints:
- deterministic pass ordering
- deterministic resource alias decisions
- no ambient host queries during planning

## Plugin contracts

Extensions register through contracts and may contribute:
- components
- asset loaders/codecs
- render/compute passes
- UI widgets/layout primitives

Plugin contract requirements:
- explicit capability declaration (`:caps`)
- deterministic planning functions for pure phases
- effectful phases only through standard `gfx/*` ops

## UI foundation

UI is defined as deterministic retained trees and resolved layout boxes.

Minimum architecture:
- flex-like layout algorithm (deterministic tie-breaks)
- vector draw list generation
- text shaping + line breaking as artifact-backed deterministic pipeline
- accessibility tree derived from UI tree

UI rendering output is a frame graph fragment merged into the final frame plan.

## Determinism + obligations

Required future obligations for Level 2:
- golden image tests with replayed input/time logs
- frame graph hash stability tests for fixed inputs
- scene hash stability tests for canonical snapshots
- frame time budget evidence artifacts

## Compatibility policy

Public Level 2 API must version independently from kernel version.

Breaking changes require:
- schema version bump in docs + data tags
- migration patch strategy
- updated translation-validation evidence where applicable
