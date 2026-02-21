# Domain Kits v0.1

Status: normative high-level prelude contract layer for AI-authored workflows.

## Purpose

Define deterministic, reusable contract schemas for common workload families so agents can
compose workflows from stable kits instead of ad-hoc effect chains.

## Prelude Modules

- `prelude/modules/30_service_orchestration.gc`
- `prelude/modules/31_data_pipeline.gc`
- `prelude/modules/32_network_workflow.gc`
- `prelude/modules/33_game_loop.gc`

## Contract Schemas

Service orchestration:
- `:core/kit/service-dependency.v1`
- `:core/kit/service-manifest.v1`
- `:core/kit/service-step.v1`
- `:core/kit/service-workflow.v1`
- `:core/kit/service-status.v1`

Data pipeline:
- `:core/kit/pipeline-stage.v1`
- `:core/kit/pipeline-spec.v1`

Network workflow:
- `:core/kit/network-http-step.v1`
- `:core/kit/network-process-step.v1`
- `:core/kit/network-workflow.v1`
- `:core/kit/network-workflow-result.v1`

Game loop:
- `:core/kit/game-fixed-loop.v1`
- `:core/kit/game-loop-result.v1`

## Determinism Rules

- Kits define only data contracts + deterministic orchestration helpers.
- All side effects remain capability-routed through `core/effect::perform`.
- No hidden time/network/process behavior is introduced outside explicit effect calls.

## Reference Workflow Adoption

Reference workflow entrypoints now use kit APIs:

- `examples/agent_compute_workflow/workflow_run.gc` -> pipeline kit
- `examples/agent_gpu_compute_workflow/workflow_run.gc` -> pipeline kit
- `examples/agent_network_process_workflow/workflow_run.gc` -> network kit
- `examples/agent_long_running_gfx_loop_workflow/workflow_run.gc` -> game-loop kit
- `examples/agent_service_workflow/workflow.sh` generated check program -> service kit

## Drift Guard

Migration and module presence are enforced by:

- `scripts/check_domain_kit_workflows.sh`
