# GenesisCode Graphics Demos

These are end-to-end demo programs implemented entirely in GenesisCode (`.gc`) and exercised by automated tests.

## Demo files

- `/Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/ui_app.gc`
- `/Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/scene3d.gc`
- `/Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/hybrid_web.gc`

## Run demos

```sh
genesis eval /Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/ui_app.gc
genesis eval /Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/scene3d.gc
genesis eval /Users/corbensorenson/Documents/genesisCode/examples/gfx_demos/hybrid_web.gc
```

Each demo returns a deterministic map containing a `:frame-hash` and planned frame graph data.

## Test coverage

- Pure evaluation + deterministic shape checks:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/tests/gfx_demos_examples.rs`
- CLI execution smoke checks:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_gfx_demos.rs`
