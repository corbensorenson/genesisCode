# GenesisCode Authoring Skill (v0.3)

Use this skill when implementing or modifying GenesisCode language/runtime/tooling behavior, or when writing `.gc` modules intended for long-term self-hosted evolution.

## Mission
Deliver deterministic, obligation-gated changes that move GenesisCode toward practical AI-first self-hosted product delivery with minimal bootstrap-language dependence.

## Required references (must stay synchronized)
- `docs/spec/CLI.md`
- `docs/spec/CLI_SCHEMA_v0.1.md`
- `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`
- `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
- `docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md`
- `docs/spec/HOST_ABI_INDEX_v0.1.json`
- `docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json`
- `docs/spec/SELF_HOST_BOUNDARY.md`
- `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- `upgrade_plan.md`

## Required contract IDs (must stay present)
- `genesis/cli-schema-v0.1`
- `genesis/error-v0.2`
- `genesis/pkg-lock-v0.1`
- `genesis/pkg-update-v0.1`
- `genesis/pkg-publish-v0.1`

## Ground rules (non-negotiable)
- Kernel purity: never add ambient filesystem/time/network/LLM behavior to evaluator semantics.
- Protocol integrity: preserve unforgeable `UNHANDLED`, `EFFECT`, and `ERROR` behavior.
- No panic on user input: Rust internals return `Result`; language boundaries return sealed protocol errors.
- Determinism first: every effectful workflow must support log replay with stable outcomes.
- Deny-by-default capabilities: all effect operations require explicit policy allowlists.
- No mock/stub behavior in production paths.
- No hidden policy broadening: each new operation must specify minimum policy keys and failure behavior.

## Canonical workflow (agent prompt protocol)
1. Plan
- Choose highest-impact unresolved items from `upgrade_plan.md`.
- Restate measurable acceptance criteria before editing.
- Identify affected contracts, schemas, and health gates.

2. Patch
- Implement the smallest complete vertical slice.
- Prefer `.gc` prelude/editor/tooling layers when behavior belongs in language space.
- Keep modules narrow; split by domain when files exceed maintainable size.

3. Evidence
- Run focused tests first, then broader gates.
- For prelude edits, always run `bash scripts/assemble_prelude.sh`.
- For capability wrapper changes, always run:
  - `bash scripts/update_capability_indices.sh`
  - `bash scripts/check_capability_indices.sh`
- Run impacted crate tests and quality gates.

4. Accept
- Mark `upgrade_plan.md` items complete only when all acceptance criteria pass.
- Update `feature_matrix.md` and `docs/status/REDTEAM_REPORT.md` when status changes.
- Keep unresolved-only backlog discipline in `upgrade_plan.md`.

## Effects, capabilities, and policies
- Every capability wrapper must have:
  - deterministic request payload shape
  - deterministic success/error envelope behavior
  - explicit policy requirements
  - replay-equivalence coverage
- Capability introduction checklist:
  - add prelude wrapper in domain module (`core/fs`, `core/net`, `core/process`, etc.)
  - add request-op assertions in `gc_prelude` tests
  - add run/replay conformance tests in `gc_effects`
  - update capability indices and drift checks
- Host bridge behavior:
  - fail closed
  - preserve stable `:error/code` families
  - enforce bridge policy fields (`bridge_cmd`, allowlists, bounds)
  - include deterministic diagnostics in sealed error context
- GPU policy posture:
  - dev lanes may allow deterministic fallback
  - release/full posture must enforce `device-runtime` backend contracts
  - conformance lane parity must stay green

## GenesisGraph / GenesisPkg expectations
- Patch-first workflow:
  - generate structural patch artifacts
  - apply patches and re-evaluate obligations
  - attach evidence artifacts for policy-gated refs/publish flows
- Package workflow invariants:
  - lock/install/verify behavior must stay deterministic
  - policy enforcement must be fail-closed
  - schema outputs must be stable and versioned
- VCS workflow invariants:
  - `diff/apply/merge3` contract paths must preserve deterministic artifact relationships
  - conflict artifacts must be machine-mergeable and reproducible

## Self-hosting strategy
- Prefer implementing new behavior in `.gc` modules and selfhost toolchain components.
- Keep Rust changes limited to host/bootstrap boundaries, safety guards, and runtime bridges.
- Before removing bootstrap paths:
  - verify equivalent selfhost behavior
  - verify strict smoke + replay parity
  - archive deprecated bootstrap assets with clear non-production status
- Never leave deprecated active paths behind after selfhost replacement is verified.

## Required output quality in reviews/PR notes
- Findings first when reviewing.
- Always include:
  - changed files and semantic intent
  - tests and gate commands run
  - pass/fail status and residual risks
  - exact remaining `upgrade_plan.md` item count
- If blocked:
  - state blocking condition
  - state attempted mitigations
  - propose smallest unblocking next step

## Domain playbooks
### Concurrency and task runtime
- Validate all three classes:
  - cancellation + await state transitions
  - channel close/race semantics
  - bounded parallel map/reduce determinism
- Always assert run/replay hash equivalence.
- Track stress budgets and persist time-series reports under `.genesis/perf/`.

### Host bridge fault behavior
- Inject failures per family:
  - filesystem
  - network
  - process
  - plugin
- Assert stable sealed error code families and replay stability.
- Prevent accidental silent fallback from bridge failure into permissive behavior.

### Agent gauntlet workflows
- Each required domain must have at least one workflow with:
  - deterministic run/replay
  - explicit capability policy file
  - domain-specific correctness assertion
- GPU workflows must expose backend evidence in output maps.
- Release/full posture must enforce `device-runtime` backend contract checks.

### Prelude and wrapper modularity
- Keep low-level wrappers in domain modules:
  - `00_core_fs.gc`
  - `00_core_net.gc`
  - `00_core_process.gc`
  - `00_core_time.gc`
  - `00_core_plugin.gc`
  - `00_core_pkg_vcs.gc`
- Avoid giant mixed-purpose modules; use manifest ordering and explicit deps.

## Contract templates
### Effect request wrapper
```clojure
(def core/domain::op
  (fn (arg1)
    (((core/effect::perform (quote family/op::name))
      {:arg1 arg1})
      (fn (resp) (core/effect::pure resp)))))
```

### Stable panel/report map
```clojure
{:kind :domain/report.v1 :ok true :data payload}
```

### Sealed error expectation
```text
:error/code must be stable by family and policy class
```

## Anti-patterns (reject immediately)
- Introducing ambient nondeterminism in evaluator logic.
- Capability wrappers without policy docs/tests.
- Broad allowlists added to make tests pass.
- New output schemas without version tags or compatibility notes.
- Massive single-file expansions when modular split is obvious.
- Marking plan items complete without objective acceptance evidence.

## Validation loop (minimum command set)
- `bash scripts/assemble_prelude.sh`
- `bash scripts/update_capability_indices.sh`
- `bash scripts/check_capability_indices.sh`
- `bash scripts/check_agent_reference_workflows.sh`
- `bash scripts/check_upgrade_plan_health.sh --profile dev-fast`
- plus targeted crate tests for changed domains

## AI-first authoring guidance
- Prefer explicit, low-ambiguity data contracts over concise human-centric shorthand.
- Make intermediate terms machine-traceable (`:kind`, `:schema`, explicit IDs).
- Keep function arity and payload schemas predictable.
- Favor composable small helpers over deeply nested one-off logic.
- Preserve deterministic naming and ordering for hashes, refs, and artifact graphs.
