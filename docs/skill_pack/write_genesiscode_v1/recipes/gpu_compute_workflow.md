# Recipe: GPU Compute Workflow

Workflow script:

- `examples/agent_gpu_compute_workflow/workflow.sh`

Expected domain:

- `gpu_compute`

Expected deterministic check path:

- `scripts/check_agent_reference_workflows.sh`
- `scripts/check_gpu_xr_productization_kits.sh`

Non-graphics compute variant:

- same recipe path is reused by manifest id `gpu_data_simulation_workflow`
- workflow: `examples/agent_compute_workflow/workflow.sh`
