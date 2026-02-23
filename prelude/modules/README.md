# Prelude Modules

`prelude/prelude.gc` is an assembled artifact.

Authoring source order + dependency edges live in:
- `manifest.toml`

Authoring source modules:
- `00_core.gc`
- `00_core_system_ops.gc`
- `00_core_fs.gc`
- `00_core_media.gc`
- `00_core_net.gc`
- `00_core_process.gc`
- `00_core_time.gc`
- `00_core_plugin.gc`
- `00_core_pkg_vcs.gc`
- `00_core_reachability.gc`
- `10_gfx_00_gpu_scene.gc`
- `10_gfx_01_frame_desc.gc`
- `10_gfx_02_2d_host.gc`
- `10_browser_host.gc`
- `10_xr_host.gc`
- `10_gfx_ui_runtime.gc`
- `10_gfx_runtime_planner.gc`
- `10_gfx_runtime_trace_00_plan_trace.gc`
- `10_gfx_runtime_trace_01_reports.gc`
- `10_gfx_runtime_trace_02_budget_api.gc`
- `11_gpu_compute.gc`
- `20_editor_lint_00_core.gc`
- `20_editor_lint_01_module.gc`
- `20_editor_lint_02_panel_obligation.gc`
- `20_editor.gc`
- `20_editor_agent_pkg.gc`
- `30_service_orchestration.gc`
- `31_data_pipeline.gc`
- `32_network_workflow.gc`
- `33_game_loop.gc`
- `34_xr_workflow.gc`
- `35_media_pipeline.gc`

To rebuild the assembled prelude:

```bash
scripts/assemble_prelude.sh
```

Assembly writes:
- `prelude/prelude.gc`
- `prelude/prelude.manifest.sha256`

Validation is enforced in `crates/gc_prelude/tests/prelude_modularization.rs`.
