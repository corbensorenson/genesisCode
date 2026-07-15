# GenesisCode Agent Task Benchmark v0.1

This public benchmark freezes 27 authoring tasks: nine task classes crossed with
small, medium, and large context packs. `suite.json` is the machine authority.

The reference candidates are public development oracles, not held-out material.
They prove that every workload is satisfiable through the production CLI. They
must not be used to claim held-out quality or model-independent aggregate scores.

Validate the suite with:

```sh
python3 scripts/lib/gc_task_benchmarks.py --check --self-test
cargo test -p gc_cli --test cli_agent_task_benchmarks
```
