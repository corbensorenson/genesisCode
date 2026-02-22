# Recipe: XR Workflow

Workflow script:

- `examples/agent_xr_runtime_workflow/workflow.sh`

Expected domain:

- `xr_runtime`

Mode:

- `standard`

Expected deterministic check path:

- `scripts/check_agent_reference_workflows.sh`
- `scripts/check_gpu_xr_productization_kits.sh`

XR deploy/test variant:

- same recipe path is reused by manifest id `xr_deploy_test_workflow`
- workflow: `scripts/check_gpu_xr_productization_kits.sh`
