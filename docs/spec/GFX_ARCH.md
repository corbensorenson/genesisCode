> Bundle Entry: `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

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

Implemented obligations for Level 2:
- `core/obligation::gfx-golden-images`
  - deterministic golden hashing for frame-graph/scene outputs
- `core/obligation::gfx-frame-budgets`
  - frame complexity/time budget evidence per configured suite
- `core/obligation::gfx-api-stability`
  - public gfx API surface fingerprint checks

See `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_OBLIGATIONS.md` for schema details and evidence formats.

Planned extension:
- headless pixel-golden backends (browser/native) using deterministic input/time logs for image-level parity checks.
  - native deterministic headless backend is now implemented and wired into `core/obligation::gfx-golden-images` via `:expect-png-h`.

## Compatibility policy

Public Level 2 API must version independently from kernel version.

Breaking changes require:
- schema version bump in docs + data tags
- migration patch strategy
- updated translation-validation evidence where applicable

## Current GenesisCode surface (implemented)

The following pure/effect wrappers are currently implemented in `prelude/prelude.gc`:

- Frame graph builders:
  - `core/gfx/frame::empty`
  - `core/gfx/frame::render-pass`
  - `core/gfx/frame::compute-pass`
  - `core/gfx/frame::add-render-pass`
  - `core/gfx/frame::add-compute-pass`
  - `core/gfx/frame::submit`
- Scene builders:
  - `core/gfx/scene::identity-transform`
  - `core/gfx/scene::empty`
  - `core/gfx/scene::node`
  - `core/gfx/scene::add-node`
  - `core/gfx/scene::set-roots`
- Expanded schema-aligned data/builders:
  - descriptor constructors: `core/gfx/desc::{buffer,texture,sampler,shader-module,render-pipeline,compute-pipeline}`
  - command/pass constructors: `core/gfx/frame::{color-attachment,depth-attachment,cmd-set-pipeline,cmd-set-vertex-buffer,cmd-set-index-buffer,cmd-set-bind-group,cmd-set-push-constants,cmd-draw,cmd-draw-indexed,cmd-dispatch,render-pass-empty,compute-pass-empty,render-pass-add-color-attachment,render-pass-add-command,compute-pass-add-command}`
  - scene/math helpers: `core/gfx/math::{v2,v3,quat,rgb,rgba}`, `core/gfx/scene::{transform,mesh-ref,material-pbr,camera-perspective,camera-orthographic,light-point,light-directional,node-basic,add-root,add-child}`
  - 2D/UI data builders: `core/gfx/2d::{identity-rotation,transform,sprite-material,sprite-node,scene-empty,scene-add-draw,draw-sprite,draw-rect,draw-text}` and `core/gfx/ui::{node,style,text,button,container,vertical,horizontal}`
  - runtime planners: `core/gfx/runtime::{plan-frame-2d,plan-frame-2d-batched,plan-frame-2d-scene-batched,plan-frame-3d,plan-frame-3d-pbr,plan-frame-2d+ui,plan-render-pass,plan-render-pass-3d,plan-2d-pass-batched,2d-batches,first-camera,count-lights,count-shadow-lights,ui-node-count,hash-scene,hash-frame-graph}`
  - UI runtime projection/planning: `core/gfx/ui/runtime::{to-2d-draws,to-2d-scene,plan-frame-batched,render-node-to-draws}`

### 2D batching baseline (implemented)

`core/gfx/runtime::2d-batches` performs deterministic run-length batching over 2D draw items using structural batch keys (`:kind`, `:texture`, `:font`, `:blend`), and `core/gfx/runtime::plan-frame-2d-batched` emits one draw command per batch with `:instance-count` set to the batch size.

### 3D PBR baseline (implemented)

`core/gfx/runtime::plan-frame-3d-pbr` emits a deterministic main render pass plus deterministic depth-only shadow passes (one per shadow-casting light), and attaches stable frame metadata under `:meta`:
- `:camera` (first camera found in scene traversal order, or `nil`)
- `:light-count` (total lights)
- `:shadow-light-count` (lights where `:casts-shadow` is true or omitted)

Shadow pass labels and depth views are deterministic (`<prefix><index>`), and shadow pass command streams reuse the same renderable-node planner as main pass construction.

### UI toolkit baseline (implemented)

`core/gfx/ui/runtime` now projects retained UI trees into deterministic 2D draw lists and plans batched 2D frame graphs:
- style `:paint/:bg` emits rect draws
- `text` and `button` nodes emit text draws (with deterministic defaults for font/color/size)
- container `:layout/:axis` + `:spacing/:gap` drives deterministic child placement
- resulting `:gfx/2d-scene` flows through `core/gfx/runtime::plan-frame-2d-scene-batched`

### End-to-end demos (implemented)

See `/Users/corbensorenson/Documents/genesisCode/docs/GFX_DEMOS.md` and `/Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/` for runnable `.gc` demos:
- `ui_app.gc`
- `scene3d.gc`
- `hybrid_web.gc`

- Capability wrappers:
  - `core/gfx/gpu::*`, `core/gfx/window::*`, `core/gfx/input::*`, `core/gfx/time::*`, `core/gfx/audio::*`
