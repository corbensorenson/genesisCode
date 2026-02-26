# Capability Coverage Audit v0.1

Generated from:
- `docs/spec/HOST_ABI_INDEX_v0.1.json`
- `docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `upgrade_plan.md`

Summary:
- Families: 33 (implemented=32, policy-disabled=1, planned=0)
- Operations: host=187 prelude=184
- Planned upgrade IDs: none

## Coverage Table

| Family | Status | Plan ID | Host Ops | Prelude Ops | Host-Only Ops | Release Gates |
|---|---|---|---:|---:|---:|---|
| `browser/audio` | `implemented` | `-` | 2 | 2 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh |
| `browser/input` | `implemented` | `-` | 1 | 1 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh |
| `browser/storage` | `implemented` | `-` | 3 | 3 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh |
| `browser/window` | `implemented` | `-` | 3 | 3 | 0 | scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh<br>scripts/check_webxr_browser_conformance_lane.sh |
| `core/crypto` | `implemented` | `-` | 6 | 6 | 0 | - |
| `core/gc-low` | `implemented` | `-` | 5 | 5 | 0 | - |
| `core/gpk-low` | `implemented` | `-` | 2 | 2 | 0 | - |
| `core/media` | `implemented` | `-` | 3 | 3 | 0 | scripts/check_agent_reference_workflows.sh<br>scripts/check_upgrade_plan_health.sh |
| `core/pkg-low` | `implemented` | `-` | 13 | 13 | 0 | scripts/check_agent_reference_workflows.sh<br>scripts/check_gcpm_operation_contract_pack.sh<br>scripts/check_remote_registry_runtime_parity.sh<br>scripts/check_upgrade_plan_health.sh |
| `core/refs` | `implemented` | `-` | 4 | 4 | 0 | - |
| `core/store` | `implemented` | `-` | 4 | 4 | 0 | scripts/check_agent_reference_workflows.sh<br>scripts/check_gcpm_operation_contract_pack.sh<br>scripts/check_remote_registry_runtime_parity.sh<br>scripts/check_upgrade_plan_health.sh |
| `core/sync` | `implemented` | `-` | 2 | 2 | 0 | scripts/check_agent_reference_workflows.sh<br>scripts/check_gcpm_operation_contract_pack.sh<br>scripts/check_remote_registry_runtime_parity.sh<br>scripts/check_upgrade_plan_health.sh |
| `core/task` | `implemented` | `-` | 10 | 10 | 0 | - |
| `core/vcs-low` | `implemented` | `-` | 10 | 10 | 0 | - |
| `editor/clipboard` | `implemented` | `-` | 2 | 2 | 0 | - |
| `editor/dialog` | `implemented` | `-` | 2 | 2 | 0 | - |
| `editor/plugin` | `implemented` | `-` | 1 | 1 | 0 | - |
| `editor/task` | `implemented` | `-` | 9 | 9 | 0 | scripts/check_task_concurrency_stress.sh<br>scripts/check_upgrade_plan_health.sh |
| `editor/watch` | `implemented` | `-` | 3 | 3 | 0 | - |
| `gfx/audio` | `implemented` | `-` | 2 | 2 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh |
| `gfx/gpu` | `implemented` | `-` | 16 | 16 | 0 | scripts/check_agent_reference_workflows.sh<br>scripts/check_gpu_compute_device_conformance.sh<br>scripts/check_gpu_compute_runtime_profile.sh<br>scripts/check_gpu_device_conformance_matrix.sh<br>scripts/check_gpu_stack_decoupling.sh<br>scripts/check_upgrade_plan_health.sh |
| `gfx/input` | `implemented` | `-` | 2 | 2 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh |
| `gfx/time` | `implemented` | `-` | 1 | 1 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh |
| `gfx/window` | `implemented` | `-` | 5 | 5 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh |
| `gfx/xr` | `implemented` | `-` | 15 | 15 | 0 | scripts/check_gfx_runtime_profile.sh<br>scripts/check_gpu_xr_productization_kits.sh<br>scripts/check_upgrade_plan_health.sh<br>scripts/check_webxr_browser_conformance_lane.sh |
| `gpu/compute` | `implemented` | `-` | 13 | 13 | 0 | scripts/check_agent_reference_workflows.sh<br>scripts/check_gpu_compute_device_conformance.sh<br>scripts/check_gpu_compute_runtime_profile.sh<br>scripts/check_gpu_device_conformance_matrix.sh<br>scripts/check_gpu_stack_decoupling.sh<br>scripts/check_upgrade_plan_health.sh |
| `host/ffi` | `policy-disabled` | `-` | 3 | 0 | 3 | scripts/check_host_abi_conformance.sh<br>scripts/check_upgrade_plan_health.sh |
| `host/plugin` | `implemented` | `-` | 1 | 1 | 0 | - |
| `io/db` | `implemented` | `-` | 10 | 10 | 0 | - |
| `io/fs` | `implemented` | `-` | 7 | 7 | 0 | - |
| `io/net` | `implemented` | `-` | 19 | 19 | 0 | - |
| `sys/process` | `implemented` | `-` | 7 | 7 | 0 | - |
| `sys/time` | `implemented` | `-` | 1 | 1 | 0 | - |
