# Graphics Obligations v0.2

This document specifies the graphics-oriented obligations supported by `genesis test`.

## `core/obligation::gfx-golden-images`

Purpose:
- lock deterministic rendering-planning outputs to golden hashes.

Configuration:
- `package.toml` `[gfx].golden_tests = ["suite/sym::goldens", ...]`

Suite entry schema:
- `:body` callable (`fn (_) ...`)
- `:kind` `:frame-graph | :scene` (optional, defaults to `:frame-graph`)
- `:expect-h` 64-char lowercase hex hash
- optional pixel-golden fields (frame-graph only):
  - `:expect-png-h` 64-char lowercase hex hash of deterministic PNG bytes
  - `:pixel-width` / `:pixel-height` positive ints (default `256`)

Body result:
- pure datum for frame-graph/scene (or wrapper map containing `:frame`/`:scene`)

Hashing:
- current implementation hashes canonical CoreForm terms (`core/coreform::hash-term`) for deterministic cross-host behavior.
- when `:expect-png-h` is present, obligations additionally run a deterministic native headless renderer and compare PNG hash.

Evidence artifact:
- `:kind = "genesis/gfx-golden-images-v0.2"`
- includes per-case expected hash, actual hash, and status.
- includes native headless pixel/png hashes when pixel gating is configured.

## `core/obligation::gfx-frame-budgets`

Purpose:
- enforce deterministic frame complexity and optional frame-time budgets.

Configuration:
- `package.toml` `[gfx].frame_budget_tests = [...]`
- one or more limits:
  - `max_render_passes_per_frame`
  - `max_compute_passes_per_frame`
  - `max_draw_commands_per_frame`
  - `max_compute_commands_per_frame`
  - `max_frame_graph_bytes`
  - `max_frame_time_ms`

Suite entry schema:
- callable, or map `{ :body callable }`

Body result:
- `:gfx/frame-graph`, or
- `{ :frame <frame-graph> :frame-time-ms <int|nil> }`

Evidence artifact:
- `:kind = "genesis/gfx-frame-budgets-v0.2"`
- includes configured limits + per-case measured metrics.

## `core/obligation::gfx-api-stability`

Purpose:
- lock the public Level-2 gfx API surface and catch accidental drift.

Configuration:
- `package.toml` `[gfx]`
  - `api_exports = ["core/gfx/..."]` (optional strict expected set)
  - `api_surface_hash = "<hex32>"` (optional expected surface hash)

Surface model:
- tracked symbols: exported gfx symbols (`core/gfx/*`) or configured `api_exports`
- each symbol is fingerprinted by hash of its defining `def` expression
- surface hash = hash of canonical surface descriptor term

Evidence artifact:
- `:kind = "genesis/gfx-api-stability-v0.2"`
- includes computed surface hash + surface descriptor.

## Browser parity gate

- Browser-backend pixel parity is enforced in CI/headless web smoke:
  - `scripts/wasm_web_smoke.mjs` computes deterministic frame-graph image hashes in:
    - native host (`crates/gc_wasm/examples/native_gfx_headless_hashes.rs`)
    - browser wasm backend (`gc_wasm::gfx_render_frame_graph_headless_hashes`)
  - hashes are cross-validated (`gfx_pixel_h`, `gfx_png_h`) under deterministic inputs.
- Package obligations remain pure/deterministic and continue to gate canonical frame/scene hash
  correctness plus native headless pixel-golden checks.
