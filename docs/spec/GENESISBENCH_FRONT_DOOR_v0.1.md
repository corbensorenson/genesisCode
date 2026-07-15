# GenesisBench Front Door v0.1

Status: normative for canonical benchmark execution and transport-neutral run bundles.

## Purpose

`genesis bench` is the only supported command surface for executing a model adapter against the public GenesisBench suite. It composes the existing task, reference-agent, and scoring authorities; it does not redefine their semantics.

The front door emits `genesis/genesisbench-execution-run-v0.1`. This transport-neutral execution record is distinct from the earlier `genesis/agent-benchmark-run-v0.1` local-effect reproducibility fixture. The execution record is complete for request/response, candidate, score, adapter, and replay evidence. It is not, by itself, a ranked eligibility claim: R1.4.m must independently validate and rescore its bundle and bind submitter, track, contamination, cohort, hardware, and signature evidence before registry admission.

An adapter cannot self-assert eligibility: `rankEligible` is fixed to `false` in v0.1. The field makes the boundary machine-checkable until the independent registry derives a separate admission decision.

## Commands

```sh
genesis --json bench inspect [--case CASE] [--adapter ADAPTER.json]
genesis --json --selfhost-artifact selfhost/toolchain.gc bench run \
  --case CASE --adapter ADAPTER.json --out RUN_DIR \
  [--adapter-executable EXECUTABLE] [--model-artifact MODEL] \
  [--ablation retrieval]
genesis --json bench validate-run --run RUN_DIR/run.json
genesis --json --selfhost-artifact selfhost/toolchain.gc bench score \
  --case CASE --candidate CANDIDATE_DIR [--out SCORE.json]
genesis --json --selfhost-artifact selfhost/toolchain.gc bench replay \
  --run RUN_DIR/run.json
genesis --json bench bundle --run RUN_DIR/run.json --out RESULT.gcbundle
genesis --json bench submit --bundle RESULT.gcbundle \
  --outbox OUTBOX --submitter STABLE_ID
```

`run`, `score`, and successful-run `replay` invoke the existing `GC-AGENT-BENCHMARK-SCORING-v0.1` authority with the shipped GenesisCode executable and artifact-only self-host toolchain. `validate-run`, `bundle`, and `submit` do not execute candidate code. `replay` never invokes the model or adapter; it validates every recorded field and byte, then independently rescores only when a score exists.

Front-door v0.1 executes the canonical `retrieval` reference condition only. The other seven ablations remain immutable, predeclared analysis authorities, but are not silently approximated by this runtime; adding their distinct grammar, diagnostic, semantic-patch, and repair behavior requires an explicit front-door protocol revision.

## Closed adapters

`GENESISBENCH_ADAPTERS_v0.1.json` closes exactly five classes:

| Class | Granted authority | Required binding |
|---|---|---|
| `hosted-api` | One exact non-loopback HTTPS origin and path; one declared secret name | Immutable provider model revision; redirects denied |
| `local-openai-compatible` | One exact loopback HTTP origin and path | Immutable local server/model revision; redirects denied |
| `direct-local-runtime` | One process group; digest-bound executable and model artifact | Exact executable/model SHA-256, finite timeout/output |
| `command-plugin` | One process group; digest-bound executable; empty temporary working directory | Exact executable SHA-256, closed argv, finite timeout/output |
| `deterministic-mock` | No process, filesystem, environment, or network | Request-hash-bound fixture; permanently unranked |

All adapters consume the same closed request and produce the same closed response. Mapping is typed and lossless. It may add transport framing but cannot change candidate bytes, rewrite semantics, add tools, add messages, select a better hidden attempt, or retry. Provider tools and hidden retries are always false. Every provider fact must use a key predeclared by the adapter. Unknown response, provider-fact, candidate, or error fields fail closed.

Hosted credentials are read only from the one declared environment-variable name. Values are never serialized. Each attempt records the declared name, whether it was present, and that no value was retained. Errors are normalized without credential, endpoint-response, hostname, username, or absolute-path payloads.

Process adapters receive canonical JSON on standard input and emit one bounded JSON response. They run in a new process group with a sanitized environment and empty temporary current directory. Direct runtimes alone receive the verified model-artifact locator. Timeout, explicit cancellation, and interruption kill and reap the entire process group. A failed, cancelled, or timed-out invocation is retained as an immutable `invalid` run with its request and normalized response; it is never discarded or silently retried.

The process adapter is a trusted, digest-bound transport boundary. Its manifest may not claim OS-level filesystem or network isolation that the host does not provide. Ranked policy may require a separately attested sandbox; contract-only `command-plugin` fixtures remain unranked.

## Run closure

A run directory contains:

- `adapter.json`: exact validated adapter manifest;
- `plan.json`: exact reference-agent plan for the case and ablation;
- `attempts/NNN/request.json` and `response.json`: every attempt in order;
- `candidate/`: exact public inputs plus adapter-returned editable files, or an empty directory for an invocation failure;
- `score.json`: the canonical score when candidate scoring occurred;
- `run.json`: all bindings, statuses, secret-presence facts, identities, inventories, outcome, and replay policy.

The run identity covers every serialized field. Its artifact inventory covers every run byte except `run.json`, avoiding a recursive file hash; `run.json` separately binds that inventory identity. Validation reconstructs the exact case and reference-agent plan from repository authorities, validates the copied adapter and all request/response fields, compares inventories to bytes, and verifies score/outcome consistency. Symlinks, absolute paths, traversal, duplicate paths, unsorted inventories, unknown fields, undeclared provider facts, non-editable candidate paths, and identity drift are rejected.

## Replay and bundles

Replay is transcript replay, not model regeneration. It succeeds after the external adapter executable, endpoint, credentials, and model artifacts are unavailable. A successful run is independently rescored and must match its stored score object byte-for-object after parsing. An invocation failure has no score and replay reports that rescore was not applicable.

`.gcbundle` is deterministic gzip over sorted USTAR members. The gzip timestamp and every tar timestamp, owner, group, and mode are fixed; symlinks and non-regular members are forbidden. `bundle-manifest.json` binds every artifact byte and the run identity. Repacking unchanged evidence must produce identical bytes.

`submit` performs no network operation. It validates the complete bundle and writes a SHA-256-named bundle plus content-addressed submission envelope to a local immutable outbox. Repeating an identical submission is idempotent; any identity collision fails closed. Signing, registry transport, independent admission workers, append-only history, and leaderboard semantics belong to R1.4.m.

## Conformance

Run the consolidated authority and shipped integration tests:

```sh
python3 scripts/lib/genesisbench_front_door.py check --self-test
cargo test -p gc_cli --test cli_genesisbench_front_door
```

The authority test executes identical vectors through all five classes, hard-kills timeout and cancellation fixtures, and rejects capability, retry, semantic-rewrite, redirect, model, secret, run-metadata, and identity mutations. The integration test exercises the public `genesis bench` surface, deletes the external command adapter before replay, independently rescores, proves byte-identical bundles, submits to an immutable outbox, retains a failed hosted attempt, and rejects candidate tampering.
