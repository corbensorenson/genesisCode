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
- `gfx/gpu::create-compute-pipeline`
- `gfx/gpu::destroy-resource`

Data upload/readback:
- `gfx/gpu::write-buffer`
- `gfx/gpu::write-texture`
- `gfx/gpu::read-buffer`
- `gfx/gpu::read-texture`

Frame/dispatch:
- `gfx/gpu::submit-frame-graph`
- `gfx/gpu::submit-compute-graph`

Introspection:
- `gfx/gpu::limits`
- `gfx/gpu::features`

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

