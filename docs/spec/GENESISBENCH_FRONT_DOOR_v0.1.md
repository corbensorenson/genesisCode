# GenesisBench Front Door v0.1

Status: normative for canonical benchmark execution and transport-neutral run bundles.

## Purpose

`genesis bench` is the only supported command surface for executing a model adapter against the public GenesisBench suite. It composes the existing task, reference-agent, and scoring authorities; it does not redefine their semantics.

The front door emits `genesis/genesisbench-execution-run-v0.1`. This transport-neutral execution record is distinct from the earlier `genesis/agent-benchmark-run-v0.1` local-effect reproducibility fixture. The execution record is complete for request/response, candidate, score, adapter, and replay evidence. It is not, by itself, a ranked eligibility claim. The separate signed registry independently validates and rescores its bundle, then binds submitter, track, contamination, cohort, hardware, and signature evidence before deriving admission.

An adapter cannot self-assert eligibility: `rankEligible` is fixed to `false` in v0.1. The field makes the boundary machine-checkable until the independent registry derives a separate admission decision.

## Commands

```sh
genesis --json bench inspect [--case CASE] [--adapter ADAPTER.json]
genesis --json --selfhost-artifact selfhost/toolchain.gc bench run \
  --case CASE --adapter ADAPTER.json --out RUN_DIR \
  [--adapter-executable EXECUTABLE] [--model-artifact MODEL] \
  [--ablation retrieval]
genesis --json bench agent-campaign-plan \
  --campaign CAMPAIGN --phase reality-gate \
  --case completion-small --case deployment-small --case generation-small \
  --case package-migration-small --case performance-repair-small \
  --case policy-minimization-small --case refactor-small --case repair-small \
  --case replay-investigation-small \
  --runner codex-cli-hosted --agent-executable CODEX \
  --model MODEL --model-revision REVISION \
  --reasoning-effort xhigh --timeout-ms 900000 \
  --hardware-class HARDWARE_CLASS [--immutable-revision] \
  --out CAMPAIGN.json
genesis --json bench agent-plan \
  --case CASE --campaign-predeclaration CAMPAIGN.json \
  --out PREDECLARATION.json
genesis --json --selfhost-artifact selfhost/toolchain.gc bench agent-run \
  --campaign-predeclaration CAMPAIGN.json \
  --predeclaration PREDECLARATION.json \
  --agent-executable CODEX --out RUN_DIR
genesis --json bench agent-validate --run RUN_DIR/run.json
genesis --json --selfhost-artifact selfhost/toolchain.gc bench agent-replay \
  --run RUN_DIR/run.json
genesis --json bench validate-run --run RUN_DIR/run.json
genesis --json --selfhost-artifact selfhost/toolchain.gc bench score \
  --case CASE --candidate CANDIDATE_DIR [--out SCORE.json]
genesis --json --selfhost-artifact selfhost/toolchain.gc bench replay \
  --run RUN_DIR/run.json
genesis --json bench bundle --run RUN_DIR/run.json --out RESULT.gcbundle
genesis --json bench submit --bundle RESULT.gcbundle --claim CLAIM.json \
  --outbox OUTBOX --submitter STABLE_ID --key SUBMITTER.toml
genesis --json bench registry-init --registry REGISTRY \
  --policy POLICY.json --operator-key OPERATOR.toml
genesis --json --selfhost-artifact selfhost/toolchain.gc bench registry-admit \
  --registry REGISTRY --submission SUBMISSION.json --bundle RESULT.gcbundle \
  --operator-key OPERATOR.toml
genesis --json --selfhost-artifact selfhost/toolchain.gc bench registry-verify \
  --registry REGISTRY
genesis --json --selfhost-artifact selfhost/toolchain.gc bench registry-build \
  --registry REGISTRY --out STATIC_SITE
```

`run`, `score`, and successful-run `replay` invoke the existing `GC-AGENT-BENCHMARK-SCORING-v0.1` authority with the shipped GenesisCode executable and artifact-only self-host toolchain. `validate-run`, `bundle`, and `submit` do not execute candidate code. `replay` never invokes the model or adapter; it validates every recorded field and byte, then independently rescores only when a score exists.

Front-door v0.1 executes the canonical `retrieval` reference condition only. The other seven ablations remain immutable, predeclared analysis authorities, but are not silently approximated by this runtime; adding their distinct grammar, diagnostic, semantic-patch, and repair behavior requires an explicit front-door protocol revision.

## Open Agent Harness

The Open Agent extension is a separate content-addressed execution boundary. It does not add a sixth class to `GENESISBENCH_ADAPTERS_v0.1.json`, change Cold Acquisition, or make custom orchestration comparable to the fixed reference scaffold. New campaigns use `GENESISBENCH_OPEN_AGENT_v0.2.json`; the immutable v0.1 authority remains available for historical replay. Its recursively closed campaign, attempt-predeclaration, run, and campaign-report contracts are `GENESISBENCH_OPEN_AGENT_CAMPAIGN_v0.1.schema.json`, `GENESISBENCH_OPEN_AGENT_PREDECLARATION_v0.1.schema.json`, `GENESISBENCH_OPEN_AGENT_RUN_v0.1.schema.json`, and `GENESISBENCH_OPEN_AGENT_CAMPAIGN_REPORT_v0.1.schema.json`.

`agent-campaign-plan` must run before model inference. It atomically binds the complete 9-case reality gate or 27-case public matrix, all protocol/suite/snapshot/scaffold identities, exact Codex executable digest and version, requested model and revision, reasoning effort, local model artifact when applicable, hardware class, secret handling, one-attempt policy, capabilities, environment-name allowlist, finite wall-time/output/workspace budgets, non-selective stop rules, and expected-attempt count. `agent-plan` may only derive an attempt for a case already named by that immutable campaign; the attempt binds the campaign identity and repeats the common fields exactly. Neither contract records environment or secret values. Every harness predeclaration is deterministically `rankEligible: false`; only the independent signed registry may derive ranked admission after verifying provenance, contamination, artifact, cohort, replay, and rescore evidence. Passing `--immutable-revision` is a provenance assertion and is appropriate only when the provider exposes a genuinely immutable revision. A local cohort additionally requires `--model-artifact-sha256` and either `--local-provider lmstudio` or `--local-provider ollama`.

`agent-run` reconstructs the 1,775-file frozen protocol snapshot from the pinned Git commit and verifies every archived byte before execution. The v0.2 process workspace is created outside the repository ancestry so project `AGENTS.md`, skills, configuration, and other ambient instructions cannot be discovered through parent traversal. The snapshot and copied GenesisCode toolchain live under a read-only input root outside the writable case workspace. The fixed Codex invocation is ephemeral, JSONL, non-resumable, `workspace-write`, approval-free, and one prompt; hosted and loopback-local inference are separate invocation profiles. The process receives only the closed environment-name allowlist. Timeout or capture overflow kills and reaps the complete process group. Permission-aware cleanup removes every protected temporary snapshot after its retained evidence is atomically published.

After execution, the harness rechecks frozen source bytes, executable modes, directory topology, and symlink absence. It inventories the case workspace, rejects undeclared paths, non-editable drift, symlinks, malformed events, nonzero exit, timeout, or capture overflow, and scores only a clean candidate. Invalid attempts remain immutable evidence rather than disappearing. `agent-validate` derives the violation set again from retained facts and bytes. `agent-replay` never resolves or invokes the agent executable or model; it validates the complete run and independently rescores the exact retained candidate.

The initial hosted study requested by the project uses Codex CLI with Luna at `xhigh`. If `luna` resolves only through a mutable alias, predeclare it honestly:

```sh
CODEX=/Applications/ChatGPT.app/Contents/Resources/codex
genesis --json bench agent-campaign-plan \
  --campaign codex-luna-xhigh-2026-07 --phase reality-gate \
  --case completion-small --case deployment-small --case generation-small \
  --case package-migration-small --case performance-repair-small \
  --case policy-minimization-small --case refactor-small --case repair-small \
  --case replay-investigation-small \
  --runner codex-cli-hosted \
  --agent-executable "$CODEX" \
  --model luna \
  --model-revision provider-alias:luna@2026-07-17 \
  --reasoning-effort xhigh \
  --timeout-ms 900000 \
  --hardware-class apple-silicon-local \
  --out predeclarations/campaign.json
genesis --json bench agent-plan \
  --case completion-small \
  --campaign-predeclaration predeclarations/campaign.json \
  --out predeclarations/completion-small.json
```

Do not pass `--immutable-revision` for an alias merely to make a run rankable. Campaign expansion starts only after the nine-task-class reality gate passes, and hosted Luna, raw fixed-scaffold models, and local Codex-agent models remain separate cohorts.

The first immutable Luna campaign is retained under `benchmarks/genesisbench/v0.1/campaigns/codex-luna-xhigh-2026-07-17/reality-gate/`. All nine attempts validate and replay, but every attempt is invalid because the ChatGPT-backed Codex service rejected `luna` as unsupported. The campaign report also records the v0.1 ambient skill-discovery defect, forbids 27-condition expansion, and makes no capability claim. v0.2 closes that harness defect without rewriting or retrying the historical attempts.

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

`submit` performs no network operation. It validates the complete bundle and closed claim, signs a domain-separated canonical DSSE statement with an Ed25519 key generated by `genesis keygen`, and writes the SHA-256-named bundle plus content-addressed signed submission to a local immutable outbox. Private key bytes remain inside the Rust signing boundary and key files must be regular, non-symlink, and inaccessible to group/other users. Repeating an identical submission is idempotent; any identity collision fails closed.

The registry policy pins the exact protocol identity, operator key, submitter identities/keys/provenance, admitted tracks, bundle ceiling, and ranking semantics. Operator and submitter keys must differ. `registry-admit` verifies the submitter signature, validates every bundle field and artifact byte, replays without model access, independently rescoring when applicable, derives rather than trusts eligibility, then appends one operator-signed hash-chained event and a checkpoint over the complete signed event prefix. Results, submissions, bundles, events, and checkpoints are content-addressed and never overwritten or deleted by any command.

`registry-verify` does not stop at signatures. It rejects gaps, rewrites, hidden or orphaned objects, unknown topology, missing event-prefix checkpoints, and silent history suppression; it also extracts every retained bundle and independently rederives its complete result. `registry-build` performs the same verification and emits a new deterministic static JSON/HTML publication. It includes all attempts, failures, invalids, abstentions, missing lineages, per-lineage and per-class outcomes, Wilson intervals, model/runtime and contamination/hardware facts, economics, and replay commands.

Ranking occurs only for complete evaluation sets inside one exact content-addressed cohort. The lexicographic order is verified solve rate, conditional quality among solved lineages, capability excess, context bytes, tool calls, then repair calls. Invalid partial quality is zero. Cost, latency, energy, provider identity, and stable system IDs never improve substantive rank; exact metric ties remain ties.

## Conformance

Run the consolidated authority and shipped integration tests:

```sh
python3 scripts/lib/genesisbench_front_door.py check --self-test
python3 scripts/lib/genesisbench_open_agent.py check --self-test
cargo test -p gc_cli --test cli_genesisbench_front_door
cargo test -p gc_cli --test cli_genesisbench_registry
```

The authority test executes identical vectors through all five fixed adapters, then runs 14 Open Agent controls for hidden retries, capability broadening, model-claim forgery, snapshot byte/mode/symlink/topology mutation, malformed events, and descendant survival after timeout. The integration test exercises both public surfaces, deletes external adapter/agent executables before replay, independently rescores, proves byte-identical bundles, submits to an immutable outbox, retains failures, and rejects candidate tampering.
