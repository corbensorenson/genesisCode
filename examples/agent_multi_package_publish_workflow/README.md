# Agent Multi-Package Publish Workflow

Deterministic selfhost workflow for publishing and syncing a multi-package app stack.

## What It Covers

- Two-package topology (`lib-core` + `app-main`).
- Policy-gated `pkg publish` for both packages.
- Remote sync + consumer verification for both commits.
- Effect-log replay determinism in consumer verification step.

## Run

From repo root:

```bash
bash examples/agent_multi_package_publish_workflow/workflow.sh
```
