# Prelude Modules

`prelude/prelude.gc` is an assembled artifact.

Authoring source order + dependency edges live in:
- `manifest.toml`

Authoring source modules:
- `00_core.gc`
- `00_core_system_ops.gc`
- `00_core_fs.gc`
- `00_core_net.gc`
- `00_core_process.gc`
- `00_core_time.gc`
- `00_core_plugin.gc`
- `00_core_pkg_vcs.gc`
- `00_core_reachability.gc`
- `10_gfx.gc`
- `10_browser_host.gc`
- `10_xr_host.gc`
- `10_gfx_ui_runtime.gc`
- `10_gfx_runtime_planner.gc`
- `10_gfx_runtime_trace.gc`
- `11_gpu_compute.gc`
- `20_editor_lint.gc`
- `20_editor.gc`
- `20_editor_agent_pkg.gc`
- `30_service_orchestration.gc`
- `31_data_pipeline.gc`
- `32_network_workflow.gc`
- `33_game_loop.gc`
- `34_xr_workflow.gc`

To rebuild the assembled prelude:

```bash
scripts/assemble_prelude.sh
```

Assembly writes:
- `prelude/prelude.gc`
- `prelude/prelude.manifest.sha256`

Validation is enforced in `crates/gc_prelude/tests/prelude_modularization.rs`.
