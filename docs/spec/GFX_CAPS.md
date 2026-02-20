# Genesis Graphics Capability Surface (Draft v0.1)

This document defines the host boundary for a full 2D/3D graphics stack under GenesisCode.

Goals:
- One capability model for web apps, tools, and games.
- Deterministic command/data artifacts in the language layer.
- Host-specific GPU/window/input implementations behind effect ops.

Non-goals:
- Kernel-side rendering logic.
- Ambient time/input state outside effect logs.

## Capability families

All ops are deny-by-default and effect-logged.

- `gfx/gpu::*`
- `gpu/compute::*`
- `gfx/window::*`
- `gfx/input::*`
- `gfx/time::*`
- `gfx/audio::*` (optional stage)

## `gfx/gpu::*` ops

Resource lifecycle:
- `gfx/gpu::create-buffer`
- `gfx/gpu::create-texture`
- `gfx/gpu::create-sampler`
- `gfx/gpu::create-shader-module`
- `gfx/gpu::create-bind-group-layout`
- `gfx/gpu::create-bind-group`
- `gfx/gpu::create-pipeline-layout`
- `gfx/gpu::create-render-pipeline`
- `gfx/gpu::destroy-resource`

Data upload/readback:
- `gfx/gpu::write-buffer`
- `gfx/gpu::write-texture`
- `gfx/gpu::read-buffer`
- `gfx/gpu::read-texture`

Frame/dispatch:
- `gfx/gpu::submit-frame-graph`

Introspection:
- `gfx/gpu::limits`
- `gfx/gpu::features`

## `gpu/compute::*` ops (canonical compute surface)

Resource lifecycle:
- `gpu/compute::create-buffer`
- `gpu/compute::create-shader-module`
- `gpu/compute::create-bind-group-layout`
- `gpu/compute::create-bind-group`
- `gpu/compute::create-pipeline-layout`
- `gpu/compute::create-compute-pipeline`
- `gpu/compute::create-kernel` (alias of `create-compute-pipeline`)
- `gpu/compute::destroy-resource`

Data upload/readback:
- `gpu/compute::write-buffer`
- `gpu/compute::read-buffer`

Dispatch/introspection:
- `gpu/compute::submit`
- `gpu/compute::limits`
- `gpu/compute::features`

Compatibility layer:
- `core/gfx/gpu::create-compute-pipeline` and `core/gfx/gpu::submit-compute-graph` remain available as compatibility wrappers and forward to canonical `gpu/compute::*` ops.

## `gfx/window::*` ops

- `gfx/window::create-surface`
- `gfx/window::resize-surface`
- `gfx/window::set-title`
- `gfx/window::request-redraw`
- `gfx/window::surface-info`

## `gfx/input::*` ops

- `gfx/input::poll-events`
- `gfx/input::set-cursor-mode`

## `gfx/time::*` ops

- `gfx/time::frame-tick` (frame delta/timebase from host; must be logged for replay)

## Determinism contract

- Language-side scene/frame graph data is canonical CoreForm.
- GPU submission payloads are hashed and logged.
- Input/time effects are replayable by log.
- Rendering outputs are side effects; correctness is checked via golden-image obligations.

## Data model

`crates/gc_gfx` contains deterministic Rust-side data structures for:
- Frame graph + draw/compute commands
- Scene graph (2D/3D)
- PBR-oriented material schema

The GenesisCode language implementation will mirror these shapes as CoreForm contracts.
