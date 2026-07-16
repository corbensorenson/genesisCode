# Check/Update Boundary v0.1

Status: normative R0 operations contract.

## Purpose

A scripts/check_*.sh entrypoint answers whether declared inputs satisfy a
contract. It does not repair, refresh, publish, reclaim, or preserve new
evidence. A scripts/update_*.sh entrypoint may produce derived material, but
the result must be reviewed and validated by a separate check.

This separation keeps agent loops reproducible: running a check cannot turn a
missing prerequisite into a pass or silently rewrite the evidence being
evaluated.

## Reachable Execution Surface

The boundary applies to the full local shell dependency closure, not only the
top-level check file. The generated audit follows:

- directly executed local shell helpers;
- directly sourced scripts/ shell libraries;
- nested executable helpers reachable from either category; and
- nested check_*.sh calls for transitive compliance.

Each check records its own SHA-256, the path and SHA-256 of every reachable
helper, and a combined execution_identity_sha256. Moving a forbidden action
into a renderer or library therefore does not remove it from review.

The scanner is intentionally conservative. A reachable helper with a
persistent default remains debt until its interface requires caller-owned
temporary output or it is split into explicit check/update surfaces.

## Check Contract

A check may:

- read repository inputs and declared E0 observations;
- create and remove private temporary files;
- execute compilation when compilation is the declared subject of that check;
- use a declared network lane when offline verification cannot be the subject;
  and
- fail with the exact producer command for absent or stale derived material.

A check must not:

- write .genesis/perf or another persistent evidence destination;
- invoke an update_* command;
- default a refresh control to enabled;
- call build-cache reclamation or another maintenance mutator;
- download an undeclared input;
- rewrite a committed generated view; or
- treat an artifact it just generated as independent evidence.

Caller-provided report environment variables cannot redirect a migrated check.
Only the corresponding update command accepts persistent output overrides.

## Policy and Audit

Canonical policy:

- policies/check_update_boundary_v0.1.json

Generated audit:

- docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json

Commands:

~~~sh
# Read-only policy, mutation-smoke, adversarial, and freshness validation.
bash scripts/check_check_update_boundary.sh

# Explicitly regenerate the reviewed audit after changing a check or policy.
bash scripts/update_check_update_boundary_audit.sh
bash scripts/check_check_update_boundary.sh
~~~

The policy exactly enumerates every check and every reachable compilation,
network, legacy persistent-output, and mutation-helper declaration. Ratchets
may stay fixed or decrease. An increase requires an explicit reviewed policy
change and cannot raise the zero targets for refresh, update invocation,
mutation helpers, or eventual persistent-output producers.

## Gate Manifest

`genesis.gates.json` is the complete resolved execution contract for every
`scripts/check_*.sh` entrypoint. `policies/gates_v0.1.json` owns reviewed
classification and resource defaults, the boundary audit owns discovered
source/execution identities and direct check dependencies, and
`genesis.prerequisites.json` owns platform and tool identities. The closed
serialization is `docs/spec/GATE_MANIFEST_v0.1.schema.json`.

Every gate declares:

- exact entrypoint and reachable-helper input paths plus conservative input sets;
- temporary, rebuildable-cache, or retained-evidence output classes;
- directly observed check dependencies;
- minimum execution profile and one of `static`, `build`, `test`, `benchmark`,
  `proof`, or `release-only`;
- expected duration and maximum working-disk envelope;
- deny, loopback-only, optional-external, or required-external network policy;
- validated platform and prerequisite-tool scope; and
- shard mode, maximum shard count, and cache/worktree isolation.

`maxShards = 0` means host-bounded automatic gate-level parallelism; it never
means unlimited process creation. `maxShards = 1` disables sharding.

The current duration and disk values are reviewed planning envelopes. R0.4.d
provides common measured telemetry, while R0.4.f turns the GB budgets into
historical enforcement; until then the manifest must not be cited as proof
that a gate met its envelope. Input sets are conservative and changed-impact
selection uses the exact resolved input and reverse-dependency closures. All
check outputs remain read-only: retained paths are empty and repository writes
are false. Benchmark gates require isolated worktree/cache execution.
Compilation gates must declare a rebuildable Cargo target output. Only
source-detected network gates may receive an external declaration, and every
external declaration names its input.

The retained manifest is generated without timestamps or host paths and binds
SHA-256 identities for policy, boundary audit, prerequisite manifest, every
entrypoint, and every reachable execution closure. Validation independently
rediscovers all check scripts, requires exact byte-equivalent regeneration,
checks the direct dependency graph is acyclic, and rejects undeclared tools,
platforms, network, compilation outputs, and retained writes.

Three declared inputs use domain markers instead of content hashes to prevent
an impossible evidence cycle: `genesis.gates.json` is
`generated-self-excluded`, `ROADMAP.md` is `evidence-citation-excluded`, and
the generated roadmap execution manifest is `roadmap-derived-excluded`. The
roadmap cites the gate bundle, and its execution view derives from that
roadmap. These are the only exclusions; their paths remain declared and their
dedicated byte-regeneration gates remain authoritative.

~~~sh
# Read-only freshness, semantic, source-drift, and adversarial validation.
bash scripts/check_gate_manifest.sh

# Explicit regeneration after reviewing policy, check, helper, or audit changes.
bash scripts/update_check_update_boundary_audit.sh
bash scripts/update_gate_manifest.sh
bash scripts/check_gate_manifest.sh
~~~

Never hand-edit `genesis.gates.json`. Adding or removing a check requires a
reviewed boundary-policy ratchet and manifest regeneration. Changing a gate or
reachable helper changes its execution identity and makes the retained
manifest stale. The permanent negative controls reject unknown fields,
missing or duplicate gates, stale hashes, unknown dependencies, host paths,
network downgrades, repository writes, zero disk budgets, unknown profiles,
shared benchmark caches, undeclared compilation outputs, and duplicate JSON
keys.

## Gate Resource Telemetry

Every governed `scripts/check_*.sh` entrypoint MUST re-execute through
`scripts/lib/gate_telemetry.py`. The parent observer emits exactly one
`genesis/gate-resource-telemetry-v0.1` record for that invocation, conforming
to `docs/spec/GATE_RESOURCE_TELEMETRY_v0.1.schema.json`. Aggregate gates do not
suppress child observations: the active marker names an entrypoint and only
prevents that same entrypoint from recursively wrapping itself.

Every record binds the gate ID, entrypoint, and execution identity from
`genesis.gates.json`; preserves passed, failed, and signaled exits; excludes
absolute checkout paths; and reports all seven metrics: monotonic duration,
process-tree peak RSS, bytes read, bytes written, generated-disk delta, cache
hits, and network attempts. Each metric carries a method and one of `exact`,
`instrumented`, `sampled`, `estimated`, or `unavailable`. A numeric zero never
implies that uninstrumented work was proved absent. Consumers MUST evaluate
the completeness label before using a metric as release evidence.

The default generated-disk observation is a constant-time filesystem
allocation delta and is `sampled`; `GENESIS_GATE_TELEMETRY_EXACT_DISK=1`
enables an exact logical-size delta over only the policy-declared generated
roots. Linux process I/O is sampled from the observed process tree. Platforms
without an equivalent facility use explicitly labeled estimates or
unavailable values. Peak RSS combines process-tree samples with child rusage.
The policy fixes sampling intervals, generated roots, event kinds, and the
event-count ceiling. Ordinary gates use the high-resolution interval.
Aggregate orchestrators use a separate lower-frequency interval so a
full-system process-tree walk does not materially perturb the child SLOs the
aggregate is intended to validate; child gates still emit their own
high-resolution observations.

Cache hits and network attempts cross a closed append-only event channel owned
by the parent observer. Repository Cargo cache materialization emits a cache
hit only after a matching content-addressed cache is reused. A gate emits a
network attempt immediately before each declared external operation, whether
that operation later succeeds or fails. Unknown fields, duplicate JSON keys,
unknown event kinds, non-positive or excessive counts, malformed repository
paths, missing wrappers, and path leakage are rejected by
`scripts/check_gate_resource_telemetry.sh`.

Telemetry is local operational evidence only. It MUST NOT enter evaluator
state, deterministic effect logs, replay identity, package identity, generated
artifact hashes, or semantic outputs, and no remote collector is required.
Ordinary checks print one canonical record to stderr and retain no repository
file. `GENESIS_GATE_TELEMETRY_DISABLE=1` is reserved for explicit authority
regeneration and telemetry self-conformance; release orchestration MUST NOT
disable observation.

## Governance Consolidation Budget

`policies/gates_v0.1.json` owns the closed one-in/one-out budget for governed
check, update, and render entrypoints until M1. `genesis.gates.json` publishes
the baseline, current inventory, non-positive delta, ceilings, retired alias
mapping, and removed declared duration/disk envelope. A new check entrypoint is
valid only for a distinct trust boundary and requires retiring another
entrypoint in the same reviewed change; documentation convenience and legacy
command compatibility are not trust boundaries.

The gate-manifest renderer independently counts all three entrypoint surfaces,
requires every retired alias to be absent and its canonical replacement to
exist, rejects compatibility-wrapper markers, and prevents count or aggregate
declared-envelope growth above the R0 baseline. The initial consolidation
retires the feature-matrix check/update aliases in favor of the canonical
capability-ledger entrypoints, reducing the governed inventory from 125 to 124
checks and from 79 to 78 updaters while leaving 58 renderers. The removed check
carried a 600-second and 4096-MiB declared CI envelope; these are scheduling
budget savings, not a claim that every run consumed the full envelope.

## Engineering Gate Budgets

`policies/engineering_gate_budgets_v0.1.json`, closed by
`docs/spec/ENGINEERING_GATE_BUDGETS_v0.1.schema.json`, is the sole numeric
authority for GB-1 through GB-8. `scripts/check_engineering_gate_contract.sh`
cross-checks it against the generated gate manifest, profile runner, changed
impact loop, deterministic cleanup policy, source-decomposition waivers,
prebuilt verifier path, prerequisite manifest, and every Python import under
`scripts/` and `tools/`. Unknown fields, duplicate keys, relaxed numbers,
unreviewed static outliers, host paths, undeclared Python modules, and
unbounded source waivers fail closed.

Every governed gate enforces its own manifest duration, positive generated
disk delta, and deny-network event count after the subject exits successfully.
The shell wrapper forces enforcement; only telemetry self-conformance and
explicit authority regeneration may disable it. A resource violation changes
the effective exit to failure even when the subject command returned zero.
Static gates are capped at 15 seconds warm, 64 MiB additional disk, no
compilation, and no network. Measured build, benchmark, or network subjects
must be reclassified rather than granted a static exception.

The changed-file loop measures its complete invocation, forces Cargo offline,
and caps additional disk at 1 GiB. `prepush-standard` is capped at eight
minutes and 3 GiB additional disk. `release-full` retains the stricter local
30-minute default while rejecting any configured ceiling above 45 minutes and
any generated artifact footprint above 20 GiB. Profile checks retain private
runtime history; explicit updaters alone may persist E0 observations.

GB-5 binds a normal workspace build to 8 GiB and the deterministic
`dev-clean` residual to 2 GiB. Only producer-marked rebuildable roots count as
deletable generated state; a size target never authorizes deletion. GB-6 runs
an explicitly supplied, already-built independent verifier for at most five
minutes under a fail-closed OS network-denial backend. That execution path
contains no Cargo or Rust compiler invocation.

GB-7 rejects every new production Rust file above 1,000 lines. Existing files
above the limit require exact source-decomposition rows with an owner, bounded
scope, rationale, parity gate, and unexpired review date. Semantic crates above
20,000 production lines require an exact M3-expiring waiver; no such waiver may
survive M3. GB-8 keeps `genesis.prerequisites.json` as the single fresh-clone
tool authority. Python 3.9 TOML compatibility is repository-vendored with its
license, and the import-closure audit rejects undeclared host packages.

## Reference Host Profiles

`policies/reference_host_profiles_v0.1.json`, closed by
`docs/spec/REFERENCE_HOST_PROFILES_v0.1.schema.json`, is the sole authority for
benchmark host classes. It mirrors all four prerequisite platforms exactly:
macOS arm64 and Linux x86_64 are tier-1 references; Linux arm64 and Windows
x86_64 remain tier-2 candidates until promoted by independently verified E3
evidence from at least two hosts and thirty samples per normalized workload.

Every profile fixes CPU architecture/model/core minima, physical memory,
workspace filesystem and case behavior, OS range, Rust/native compiler ranges,
and power-mode requirements. Reference measurements require exclusive,
non-virtualized hosts on AC power with low-power mode disabled, nominal thermal
state, and bounded background load. A passing build cannot promote a host.

`scripts/render_reference_host_observation.sh` writes one explicit-output E0
observation closed by `docs/spec/REFERENCE_HOST_OBSERVATION_v0.1.schema.json`.
The observation excludes timestamps, hostnames, users, serial numbers, and
absolute paths and binds canonical JSON with SHA-256. Conformance is recomputed
from policy rather than trusted from the producer. `scripts/check_reference_host_profiles.sh`
uses private temporary outputs, requires repeatable bytes, exercises adversarial
policy/identity/privacy controls, and leaves retained inputs unchanged. Unsigned
E0 host observations are diagnostic only and cannot support release claims.

## Normalized Roadmap Workloads

`policies/perf/roadmap_workloads_v0.1.json`, closed by
`docs/spec/ROADMAP_WORKLOADS_v0.1.schema.json`, is the sole PB-1 through PB-10
workload authority. Every PB row binds exact repository-relative input bytes and
SHA-256 identities, canonical expected-outcome descriptors, input sizes, target
direction/unit/value, implementation availability, runner, cache state, warmup,
sample count and unit, timeout, and statistical protocol. PB-1/PB-2/PB-3 share
one exact `fib(25)` term and expected result. Unimplemented bytecode, snapshot,
warm-leak, parity, and bootstrap runners remain `roadmap-blocked`; the optional
JIT remains `decision-gated`. Neither state can emit passing evidence.

Timing workloads retain thirty ordered samples after five warmups. Their primary
summary is the median, p95 uses nearest-rank ceiling, dispersion is median
absolute deviation, no outlier is discarded, and the budget decision uses the
directional bound of the exact distribution-free 95 percent median interval
(ranks 10 and 21 for n=30). Any timeout or failed semantic check invalidates the
entire set. PB-8 retains thirty quiescent RSS checkpoints across exactly 100,000
bounded requests and requires both bounded maximum growth and a nonpositive
Theil-Sen slope. PB-9 and PB-10 use all-match deterministic treatment; no
statistical aggregation can hide a mismatch.

`benchmarks/roadmap/v0.1/` contains the exact source and protocol fixtures.
`scripts/lib/roadmap_workloads.py` independently recomputes every input and
descriptor identity, validates transitive PB-9/PB-10 inputs, rejects path escape
and duplicate keys, and checks availability and cross-field invariants.
`scripts/check_roadmap_workloads.sh` is read-only and byte-stable. Scalar
`best_of` reports from the legacy smoke harness remain diagnostic E0 only. They
cannot establish a normalized or signed baseline; R0.5.c must retain raw samples,
bind a conformant host observation and this canonical policy identity, preserve
failures, and pass independent evidence verification.

### Signed E0 baseline

`docs/spec/ROADMAP_BASELINE_STATEMENT_v0.1.schema.json` closes the normalized
raw-sample statement. Each active workload retains five warmups and thirty
ordered measurements in nanoseconds; summaries are recomputed from raw values
using exact rationals and floor only at integer serialization. Inactive runners
carry no fabricated samples. Budget misses, timeouts, semantic failures,
unavailable runners, and the unapproved JIT decision remain explicit. The
statement binds the workload-policy identity, portable reference-host
observation, release-equivalent binary, Rust toolchain, Git revision, and a
SHA-256 over the complete dirty tracked diff plus sorted untracked path/content
identities. Dirty local evidence remains E0.

`docs/spec/ROADMAP_BASELINE_BUNDLE_v0.1.schema.json` wraps the canonical
statement in DSSE with Ed25519. `tools/genesis-evidence-producer` reads a
caller-created 32-byte key from a regular 0600 file, signs only
non-authoritative E0 baseline statements, writes only stdout, and retains no
private key. The checked fixture key is public only; its private seed was
destroyed after capture. `genesis-roadmap-baseline-verifier`, built in the
standalone verifier workspace and sharing no producer code, requires the public
key and expected key ID outside the envelope and reports
`signatureGrantsAuthority=false`. The signature protects reviewed fixture
integrity. It cannot promote E0 to E3/E4 or authorize a release.

`scripts/update_roadmap_baseline.sh` is the sole capture path. It runs one
workload per process with hard policy timeout, preserves all 120 measured and 20
warmup results, creates content-addressed files with create-new semantics,
destroys the ephemeral private seed, and rolls back only outputs it just
created if verification fails. Existing dates and identities are never
overwritten. `scripts/check_roadmap_baseline.sh` never captures or updates; it
recomputes statistics and identities, builds producer/verifier offline, requires
byte-identical repeated verification/signing for fixed inputs, and rejects
signature, payload, authority, key, duplicate-key, key-permission, short-key,
failure-erasure, fabricated-runner, and overwrite attacks.

### Generated release notes

`policies/release_notes_v0.1.json` is the reviewed authority for release-note
inputs, outputs, dependency lockfiles, mandatory security gates, and claim
boundaries. `docs/spec/RELEASE_NOTES_POLICY_v0.1.schema.json` closes that policy;
`docs/spec/RELEASE_NOTES_v0.1.schema.json` closes the generated artifact.
`docs/program/RELEASE_NOTES_v0.2.0.json` binds exact source SHA-256 identities
and deterministically derives compatibility surfaces and reserved v1 states,
migration records, every capability limitation and roadmap gap, evidence class
and fixture authority, signed E0 baseline failures, lockfile inventories,
dependency-mirror policy, and mandatory release/security gates.

The generated artifact and its bounded `CHANGELOG.md` block are E1 traceability,
not execution attestations. Static generation MUST leave `passedChecks` empty,
describe mandatory gates as `required-but-not-attested`, preserve all lower-than-
L5 capability limitations, and authorize an unqualified capability claim only
when every tier-1 platform is L5 with immutable E3/E4 evidence. A valid
signature over an E0 fixture cannot raise its authority.

`scripts/check_release_notes.sh` is read-only, compiler-free, and network-denied.
It recomputes both outputs from canonical inputs and rejects compatibility or
evidence escalation, omitted migrations or gaps, erased limitations, dependency
or source tampering, injected gate success, missing security gates, unknown or
duplicate fields, host paths, and content-identity drift. It fails with the
single explicit repair command `bash scripts/update_release_notes.sh`; the
updater may replace only the generated JSON and the uniquely marked Unreleased
changelog region, leaving released history outside its write boundary.

### Frozen agent language profile

`policies/gc_agent_profile_v0.3.json` is the reviewed semantic authority for
`GC-AGENT-v0.3`. It freezes eleven domains: lexical grammar, CoreForm mapping,
evaluation, values and the exact primitive allowlist, contracts, modules,
effects, packages, errors, resource limits, and compatibility identifiers. It
also lists unsupported behavior explicitly. Its schema and policy require exactly
five ordered roadmap classes: `experimental-syntax`, `host-only-operation`,
`unavailable-target`, `nondeterministic-facility`, and
`out-of-profile-capability`. Every record is status `unsupported`, names an
enforcement mode and safe alternative, and optionally points to the roadmap task
that can change the boundary. Profile membership cannot grant a host capability,
stabilize a reserved v1 ID, or imply target, stage2, bytecode, or JIT availability.

`docs/spec/GC_AGENT_PROFILE_v0.3.schema.json` is recursively closed. The resolved
`docs/spec/GC_AGENT_PROFILE_v0.3.json` binds every authority and conformance path
by SHA-256 plus one canonical profile identity. `scripts/lib/gc_agent_profile.py`
independently rediscovers Rust primitive match arms, runtime profile constants,
format versions, v1 claim state, source anchors, roadmap tasks, and the exact
domain inventory. The profile contains positive and negative parser cases plus
evaluator, deterministic resource-exhaustion, and package schema/path cases.

`scripts/check_gc_agent_profile.sh` is read-only and network-denied. It validates
schema closure and retained-byte freshness, runs twenty authority/surface/
case/source adversarial controls, then compiles and executes the Rust integration
corpus directly against CoreForm, the kernel, Prelude, and package loader. A
stale profile fails with `bash scripts/update_agent_authoring_bundle.sh profile`; that explicit
resolver updates only the resolved JSON and cannot alter the reviewed policy.

The same updater is the single mutation surface for the coupled agent-authoring
trust domain. Its component selectors regenerate one authority and immediately
validate it; `all` executes profile, canonical examples, task benchmarks,
held-out openings, scoring, construct validity, benchmark run, protocol fixtures,
and corpus in dependency order, then runs the complete read-only authoring gate.
Adding one updater per generated file is forbidden by the governance entrypoint
budget.

### Compact agent core card

`scripts/check_gc_agent_core_card.sh` is read-only and network-denied. It proves
that `docs/spec/GC_AGENT_CORE_CARD_v0.3.md` is generated from the frozen profile,
ASCII-only, and at most 4,000 bytes, giving a tokenizer-independent upper bound
of at most 4,000 tokens. Its content-addressed JSON manifest lists every surface
symbol and canonical positive or semantic-negative example, plus the complete
unsupported records and five-class order; Rust conformance parses examples and
proves all classes remain visible in the compact card. Only
`scripts/update_gc_agent_core_card.sh` may regenerate
the card and manifest from `policies/gc_agent_core_card_v0.3.json`.

### Intent-selected task cards

`policies/gc_agent_task_cards_v0.3.json` is the reviewed selector/content authority
for capability, package, patch, replay, testing, deployment, and troubleshooting
cards. Every card is ASCII-only, independently source-hashed, and bounded to 1,400
bytes. The all-card bundle is below the 8,000-byte target and must never exceed
AB-2's 12,000-byte tokenizer-independent upper bound. Test, benchmark, mutable
evidence, and host-specific paths cannot authorize card content.

`scripts/check_gc_agent_task_cards.sh` is read-only and network-denied. It validates
source anchors, retained-byte freshness, selector fixtures, adversarial intent and
registry mutations, and byte-for-byte parity between the independent Python selector
and production `genesis --json agent-plan`. Only
`scripts/update_gc_agent_task_cards.sh` may regenerate the compendium and embedded
registry.

### Exact agent symbol index

`policies/gc_agent_symbol_index_v0.3.json` is the reviewed enrichment authority for
the complete frozen profile surface. `scripts/lib/gc_agent_symbol_index.py` joins it
with `docs/spec/GC_AGENT_PROFILE_v0.3.json`, independently closes the runtime primitive
allowlist, verifies every repository-relative source anchor, and emits the recursively
closed `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`. Each unique symbol has structured
signature, purity/effect semantics, required capabilities, semantic contracts,
examples, diagnostics, explicit deprecation state, and source links. The index identity
binds every source and record; host paths, prompt authority, duplicate names, incomplete
records, and unindexed primitives fail closed.

`scripts/check_gc_agent_symbol_index.sh` is read-only and network-denied. It executes
ten mutation controls, two exact-lookup controls, and production CLI integration tests.
`genesis --json agent-index` exposes only bounded index metadata; `--symbol <exact-name>`
returns at most one self-contained record without reading the documentation tree and
rejects normalization, padding, case drift, or unknown names. Only
`scripts/update_gc_agent_symbol_index.sh` may replace the generated index.

### Closed diagnostic catalog

`policies/gc_diagnostic_catalog_v0.1.json` assigns every production CLI
diagnostic family to an explicit lifecycle phase with likely causes, safe repair
actions, and anchored documentation. `scripts/lib/gc_diagnostic_catalog.py`
discovers exact production emission codes, rejects unknown families, and emits
the content-addressed `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json` under its
recursively closed Draft 2020-12 schema. Every record has a versioned ID,
severity, span contract, structured parameter definitions, and source callsites.

`scripts/check_cli_diagnostics_contract.sh` is read-only: it checks byte
freshness, source closure, identity, eleven adversarial mutations, two bounded
lookup controls, runtime envelope conformance, and exact production agent
lookup. `scripts/update_generated_authority.sh` owns the coupled tracked
diagnostic closure. Optional local timing evidence remains an explicit,
untracked invocation of `scripts/render_cli_diagnostics_contract_report.sh`.

## Deterministic Cleanup

`policies/deterministic_cleanup_v0.1.json`, closed by
`docs/spec/DETERMINISTIC_CLEANUP_POLICY_v0.1.schema.json`, is the sole
authority for repository cleanup. It enumerates exact roots in four classes:
`rebuildable-output`, `retained-evidence`, `dependency-mirror`, and
`user-authored`. The first three are selectable only through the reviewed
`dev-clean`, `observations-clean`, `mirror-clean`, and `generated-clean`
profiles. `user-authored` is permanently non-deletable. Unknown children of
`.genesis` are classified as user-authored and preserved.

A path pattern, age, size, ignore match, filename, or untracked status is never
sufficient deletion authority. Every deletable root MUST also contain a valid
`genesis/deterministic-cleanup-root-v0.1` producer marker conforming to
`docs/spec/DETERMINISTIC_CLEANUP_MARKER_v0.1.schema.json`. The marker binds the
exact repository-relative path, class, current policy SHA-256, and producer.
Cargo cache materialization marks only `.genesis/build`; dependency-mirror
preparation marks only `.genesis/dependency-mirrors`. Explicit initialization
accepts only a reviewed deletable root, rejects symlinks, symlinked parent
chains, repository escapes, filesystem/mount boundaries, and tracked content,
and is itself a conscious declaration that the whole reserved root contains no
user source. Markers can never be issued for user-authored roots.

Dry-run is the default and MUST NOT mutate the repository. It emits a canonical
path-relative `genesis/deterministic-cleanup-plan-v0.1` document conforming to
`docs/spec/DETERMINISTIC_CLEANUP_PLAN_v0.1.schema.json`. Every reviewed root,
plus each unknown discovered root, receives an `absent`, `delete`, or
`preserve` classification with logical bytes, allocated bytes, entry count,
marker identity, and a metadata-tree identity. Metadata-tree identities are
local drift guards, not portable evidence or semantic identities; they bind
relative names, type, mode, size, modification time, and hashed symlink target
without reading generated payload bytes.

Execute mode requires the reviewed plan file and its printed canonical
SHA-256. It reloads the current policy, rejects duplicate or unknown fields,
re-renders the entire plan byte-semantically, rejects any drift, and verifies
that no selected root contains tracked content. Selected roots are atomically
renamed into the fixed `.genesis/cleanup-quarantine/<plan-sha256>` transaction,
then their tree and marker identities are revalidated. Any pre-delete mismatch
rolls every rename back. Only after all roots pass post-rename validation are
the quarantined trees removed. An unresolved quarantine fails closed and is
never treated as an ordinary generated root. Interrupt and termination signals
are blocked only across this transaction and restored afterward, preventing a
cooperative signal from stranding a partially renamed batch. Execution emits a
closed `genesis/deterministic-cleanup-result-v0.1` document conforming to
`docs/spec/DETERMINISTIC_CLEANUP_RESULT_v0.1.schema.json`.

The cleanup implementation accepts no arbitrary root, preserve-path escape,
age selector, glob selector, best-effort mode, or automatic invocation from a
check. Plans contain no absolute checkout path or timestamp. They never enter
evaluator state, replay logs, package hashes, or release evidence. Checks may
diagnose low space, but only an explicit operator can run the two-phase update:

~~~sh
PLAN="${TMPDIR:-/tmp}/genesis-cleanup-plan.json"
bash scripts/reclaim_build_space.sh --dry-run --profile dev-clean --out "$PLAN"
# Review the plan and substitute the SHA-256 printed by dry-run.
bash scripts/reclaim_build_space.sh --execute --plan "$PLAN" --confirm-sha256 <printed-plan-sha256>
~~~

`scripts/check_deterministic_cleanup.sh` proves deterministic read-only
planning, all four class boundaries, missing/tampered marker rejection,
tracked and symlink protection, host-path exclusion, plan/confirmation/drift
binding, transactional rollback, selective execution, unresolved-quarantine
rejection, producer integration, and retirement of the legacy destructive
interface in isolated fixture repositories.

### Bounded generated state

`policies/generated_state_v0.1.json`, closed by
`docs/spec/GENERATED_STATE_POLICY_v0.1.schema.json`, is the admission and
retention authority for generated state. Its producer registry declares the
owner, allowed roots, content-key strategy, size classes, retention class,
lease mode, and deterministic reclamation order for Cargo caches, root Cargo
targets, self-host caches, temporary outputs, package installs, retained
evidence, dependency mirrors, and rollback quarantine. Unknown producers,
paths outside an owner's declared roots, undeclared size classes, duplicate
keys, and hard-quota overrides above GB-5 fail closed.

Rebuildable producers MUST acquire a process-bound random lease before
materializing repository-local state. Admission accounts for the larger of a
size-class reservation and observed allocated bytes, reclaims inactive entries
in `(reclaim-order, last-use-sequence, entry-id)` order at the soft quota, and
denies admission when active or requested state cannot fit under the hard quota
or the minimum-free-space reserve. An inactive requested entry already above
the hard quota may be transactionally evicted and recreated; an active entry
is never reclaimed. Process identity includes operating-system boot/session
and process-start identity so PID reuse cannot preserve a stale lease.

The disposable registry conforms to
`docs/spec/GENERATED_STATE_REGISTRY_v0.1.schema.json`. Its mutex is resolved
through the Git control directory, outside every cleanup root, so admission
and whole-root quarantine serialize on Unix, macOS, Windows, and linked Git
worktrees without holding an open file inside the tree being renamed. Registry
writes are atomic and every reclamation journals `planned` then `quarantined`
state before removal. A later admission or status operation deterministically
finishes an interrupted quarantine. Recursive removal requires the platform's
symlink-attack-resistant implementation and uses bounded retries only for
transient metadata recreation; continuous mutation remains a fail-closed
transaction for later recovery.

Automatic reclamation never grants deletion authority. Every reclaimed path
must still be inside a reviewed `rebuildable-output` root with a valid current
cleanup marker. Retained evidence, dependency mirrors, rollback quarantine,
user-authored roots, unknown `.genesis` children, and active leases are excluded
from quota candidates. Whole-root cleanup takes the same external mutex and
rejects a selected root when a live lease intersects it. Status and registry
documents are timestamp-free and host-path-free; lease tokens and process
identities are local coordination secrets and never enter semantic hashes,
effect logs, packages, or release evidence.

Repository-owned Cargo entrypoints acquire and release leases through
`scripts/lib/cargo_cache.py` and `scripts/lib/cargo_target_dir.sh`. A normal
producer transition releases its prior lease before changing semantic scope;
abnormal process exit is recovered on the next lifecycle operation. The
deterministic-cleanup gate additionally proves quota denial, low-disk denial,
legacy-island reclamation, protected retention, stale-PID recovery, interrupted
transaction recovery, concurrent same-entry builders, cleanup/admission race
closure, and bounded repeated host/WASI profile cycles.

## Cargo Cache Policy

Repository-owned Cargo invocations MUST resolve `CARGO_TARGET_DIR` through
`scripts/lib/cargo_cache.py` and `scripts/lib/cargo_target_dir.sh`. Script,
report, test-lane, and health-profile names are not build configurations and
MUST NOT select target directories. `policies/cargo_cache_v0.1.json`, closed
by `docs/spec/CARGO_CACHE_POLICY_v0.1.schema.json`, declares exactly four
semantic scopes: the root workspace for host, WASI, and browser/Node Wasm
targets and the independent evidence-verifier workspace for the host target.

The address is `<cache-root>/<workspace>/<target>/<sha256-canonical-key>`.
The canonical key binds strategy version, scope, target triple, pinned and
observed Rust toolchain identity, every declared workspace manifest, lockfile,
repository Cargo configuration, feature and Cargo-profile definitions, and
policy-declared build environment. It excludes absolute checkout paths,
caller names, timestamps, health profiles, and source files. Cargo itself
fingerprints source edits, selected features, and selected profiles; hashing
source revisions into the directory would destroy incremental reuse. Changing
feature/profile definitions or any other configuration authority rotates the
key.

`GENESIS_CARGO_CACHE_ROOT` may relocate the complete hierarchy for CI,
hermetic fixtures, operator storage, or benchmark isolation without changing
the key. A repository-local override must remain below `.genesis`.
Script-specific target variables, undeclared scopes, and arbitrary inherited
`CARGO_TARGET_DIR` values are forbidden. Inherited targets require resolver
provenance and must match when resolving the same scope; an explicit switch to
a different declared scope replaces the target in the current process.

Each materialized target contains canonical
`.genesis-cargo-cache-key.json` metadata with no host path. Existing metadata
must exactly match the requested key or resolution fails closed. The read-only
`scripts/check_cargo_target_dir_policy.sh` gate proves context convergence,
source-edit stability, toolchain/lock/manifest/feature/target/flags
sensitivity, workspace separation, relocation stability, metadata integrity,
host-path exclusion, static script/CI closure, and adversarial rejection of
legacy overrides, arbitrary inheritance, duplicate keys, unknown fields, and
undeclared scopes. Optional E0 observations are retained only through
`scripts/update_cargo_target_dir_policy_report.sh`.

## Changed-Impact Selection

`policies/changed_impact_v0.1.json` and its closed schema govern the default
changed-file loop. `scripts/lib/changed_impact.py` derives the workspace graph
from locked, offline Cargo metadata and computes the complete reverse
dependency closure for every changed crate. It maps exact and input-set paths
through `genesis.gates.json`, then propagates impact through reverse gate
dependencies. The plan records both the full semantic impact closure and the
bounded commands selected for the current loop.

Git discovery unions committed divergence, staged changes, unstaged changes,
deletions, both sides of renames, and non-ignored untracked paths. Test-only
file overrides replace rather than weaken that discovery. Paths must be
canonical repository-relative UTF-8. Workspace/toolchain/CI changes,
generated schemas and views, unknown paths, missing gate relationships, and
target sets above policy cardinality limits escalate to the declared
`prepush-standard` fallback instead of silently narrowing coverage.

`scripts/check_changed_impact.sh` independently verifies root and leaf crate
closures, generated-schema escalation, unknown and malformed path rejection,
order-independent path-free plans, complete Git-state collection, and closed
policy behavior. The changed-fast timing report binds the selected plan hash,
affected crate/gate counts, and fallback profile.

## Atomic Generated-Authority Closure

The `generated_authority` object embedded in
`policies/check_update_boundary_v0.1.json` is the sole dependency and ownership
authority for tracked derived publication. Its closed schema is
`docs/spec/GENERATED_AUTHORITY_GRAPH_v0.1.schema.json`. It deliberately reuses
`genesis.gates.json` and
`docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json` as the check discovery
authorities: a declared validator must exist in both, and every live
`scripts/update_*.sh` entrypoint must be classified exactly once as the
orchestrator, a graph-owned component, or an explicitly excluded local/operator
producer. A second independent updater inventory is forbidden.

Every graph node declares one exact owner ID, direct argv, dependencies, input
globs, exact output paths, read-only validators, mode, timeout, and disk bound.
Output ownership is globally unique. Every output must also occur in its owner's
input set so an edited or stale generated file routes back to its producer.
Tracked generated source is not exempt: the assembled Prelude and its manifest
identity, the self-host toolchain artifact, the cutover dashboard, and the
self-host review each have explicit owners and freshness validators in the same
graph as publication manifests and reference pages.
Rebuildable writes are inherited from the single `.genesis/**` staging-only
class, bounded by each node's disk declaration, and are never promoted.
Dependencies are acyclic and deterministically ordered. Graph, output, timeout,
and disk cardinalities are bounded. The four fixed-point exclusions for the gate
manifest, changelog, roadmap citation destination, and roadmap execution
manifest are explicit and domain-specific; adding an implicit self-exclusion is
not allowed.

`scripts/update_generated_authority.sh` is the only complete tracked-publication
orchestrator. It selects direct input matches and their complete downstream
closure, creates a detached staging worktree outside the authoritative checkout,
overlays committed, staged, unstaged, deleted, and non-ignored untracked state,
then executes direct argv in topological order. A byte snapshot before and after
each node must differ only at that node's declared outputs. The staged result
must pass every affected read-only validator before publication. A validator
never invokes the updater against the authoritative checkout.

Publication snapshots all non-output inputs, rejects concurrent source drift,
and acquires one create-new lock in the Git common directory. Each replacement
is prepared beside its destination while termination signals are blocked. A
rollback journal preserves the original bytes until every output is promoted;
any copy, replace, validation, injected-failure, or signal-path error restores
every already replaced path in reverse order and removes the lock. Read-only
generated-authority checks fail closed while that lock exists. Repeating a
successful closure must produce no changed bytes and a clean working tree.

Automatic closure can never execute signing, attestation, key generation,
dependency custody, release-asset publication, or retained E3/E4 evidence
rewrites. Those producers remain operator-gated and outside the tracked graph;
reaching one is an error rather than a partial update. The closure may regenerate
E1/E2 fixtures and indexes, but regeneration never upgrades a maturity,
qualification, assurance, or release claim.

`scripts/test_changed_fast.sh` sends the exact changed-path set through staged
freshness closure before selected gates run. `prepush-standard`, `release-full`,
and `full-selfhost-cutover` do the same for committed divergence from
`origin/main` when available. Six fixed routing controls cover `Cargo.lock`,
the CLI schema, roadmap text, agent diagnostic/profile authority, and the
GenesisBench suite. The structural gate also executes adversarial controls for
duplicate owners, cycles, unknown checks and updaters, resource overflow,
protected evidence, forbidden signing, automatic operator-gated publication,
route drift, undeclared writes, concurrent input/output drift, mid-promotion
failure, byte-identical rollback, and second-pass no-op behavior.

## Explicit E0 Producers

These migrated checks render into private temporary paths. Their optional local
E0 observations are produced only by the paired update command.

| Read-only check | Explicit producer |
|---|---|
| scripts/check_doc_complexity_budget.sh | scripts/update_doc_complexity_report.sh |
| scripts/check_cargo_target_dir_policy.sh | scripts/update_cargo_target_dir_policy_report.sh |
| scripts/check_kernel_tcb_contract.sh | scripts/update_kernel_tcb_contract_report.sh |
| scripts/check_host_api_evolution_contracts.sh | scripts/update_host_api_evolution_contract_report.sh |
| scripts/check_tool_qualification_lineage.sh | scripts/update_tool_qualification_lineage_report.sh |
| scripts/check_selfhost_gc_migration_plan.sh | scripts/update_selfhost_gc_migration_plan_report.sh |
| scripts/check_source_decomposition_progress.sh | scripts/update_source_decomposition_progress_report.sh |
| scripts/check_assurance_profile_packs.sh | scripts/update_assurance_profile_packs_report.sh |
| scripts/check_assurance_standards_crosswalk.sh | scripts/update_assurance_standards_crosswalk_report.sh |
| scripts/check_no_user_panics_compiler.sh | scripts/update_no_user_panics_report.sh |
| scripts/check_selfhost_artifact_fresh.sh | scripts/update_selfhost_artifact_fresh_report.sh |
| scripts/check_selfhost_dashboard_fresh.sh | scripts/update_selfhost_dashboard_fresh_report.sh |
| scripts/check_selfhost_readiness_scorecard.sh | scripts/update_selfhost_readiness_scorecard_report.sh |
| scripts/check_bootstrap_retirement_gate.sh | scripts/update_bootstrap_retirement_gate_report.sh |
| scripts/check_full_selfhost_cutover_profile.sh | scripts/update_full_selfhost_cutover_profile_report.sh |
| scripts/check_remote_registry_runtime_parity.sh | scripts/update_remote_registry_runtime_parity_report.sh |
| scripts/check_gcpm_operation_contract_pack.sh | scripts/update_gcpm_operation_contract_pack_report.sh |
| scripts/check_vcs_selfhost_contract.sh | scripts/update_vcs_selfhost_contract_report.sh |
| scripts/check_selfhost_symbol_ownership.sh | scripts/update_selfhost_symbol_ownership_report.sh |
| scripts/check_cli_diagnostics_contract.sh | scripts/render_cli_diagnostics_contract_report.sh |
| scripts/check_foundation_stdlib_conformance.sh | scripts/update_foundation_stdlib_conformance_report.sh |
| scripts/check_fuzz_differential_hardening.sh | scripts/update_fuzz_differential_hardening_report.sh |
| scripts/check_wasm_production_surface.sh | scripts/update_wasm_production_surface_report.sh |
| scripts/check_webxr_browser_conformance.sh | scripts/update_webxr_browser_conformance_report.sh |
| scripts/check_gfx_runtime_profile.sh | scripts/update_gfx_runtime_profile_report.sh |
| scripts/check_production_cli_help_surface.sh | scripts/update_production_cli_help_surface_report.sh |
| scripts/check_production_cli_parse_surface.sh | scripts/update_production_cli_parse_surface_report.sh |
| scripts/check_agent_reference_workflows.sh | scripts/update_agent_reference_workflows_report.sh |
| scripts/check_agent_generative_workloads.sh | scripts/update_agent_generative_workloads_report.sh |
| scripts/check_agent_scenario_perf.sh | scripts/update_agent_scenario_perf_report.sh |
| scripts/check_agent_workflow_runtime_parity.sh | scripts/update_agent_workflow_runtime_parity_report.sh |
| scripts/check_runtime_microbench_budgets.sh | scripts/update_runtime_microbench_budgets_report.sh |
| scripts/check_gpu_compute_runtime_profile.sh | scripts/update_gpu_compute_runtime_profile_report.sh |
| scripts/check_gpu_compute_device_conformance.sh | scripts/update_gpu_compute_device_conformance_report.sh |
| scripts/check_gpu_device_conformance_lane_parity.sh | scripts/update_gpu_device_conformance_lane_parity_report.sh |
| scripts/check_gpu_device_conformance_matrix.sh | scripts/update_gpu_device_conformance_matrix_report.sh |
| scripts/check_gpu_gfx_headroom_conformance.sh | scripts/update_gpu_gfx_headroom_conformance_report.sh |
| scripts/check_gpu_xr_productization_kits.sh | scripts/update_gpu_xr_productization_kits_report.sh |
| scripts/check_task_concurrency_stress.sh | scripts/update_task_concurrency_stress_report.sh |
| scripts/check_host_bridge_fault_injection.sh | scripts/update_host_bridge_fault_injection_report.sh |
| scripts/check_hot_path_budgets.sh | scripts/update_hot_path_budgets_report.sh |
| scripts/check_perf_budgets.sh | scripts/update_perf_budgets_report.sh |
| scripts/check_runtime_workload_budgets.sh | scripts/update_runtime_workload_budgets_report.sh |
| scripts/check_ai_iteration_slo.sh | scripts/update_ai_iteration_slo_report.sh |
| scripts/check_ai_stress_suite.sh | scripts/update_ai_stress_suite_report.sh |
| scripts/check_backend_starter_workflows.sh | scripts/update_backend_starter_workflows_report.sh |
| scripts/check_domain_starter_registry_bootstrap.sh | scripts/update_domain_starter_registry_bootstrap_report.sh |
| scripts/check_full_cross_host_profile_budget.sh | scripts/update_full_cross_host_profile_budget_report.sh |
| scripts/check_gcpm_target_runtime_pipelines.sh | scripts/update_gcpm_target_runtime_pipelines_report.sh |
| scripts/check_runtime_backend_feature_matrix.sh | scripts/update_runtime_backend_feature_matrix_report.sh |
| scripts/check_write_genesiscode_skill_conformance.sh | scripts/update_write_genesiscode_skill_conformance_report.sh |
| scripts/check_source_decomposition_tracked_parity.sh | scripts/update_source_decomposition_tracked_parity_report.sh |
| scripts/check_large_workspace_agent_perf.sh | scripts/update_large_workspace_agent_perf_report.sh |
| scripts/check_upgrade_plan_health.sh | scripts/update_upgrade_plan_health_report.sh |

selfhost/toolchain.review.md follows the same separation through
scripts/render_selfhost_toolchain_review.sh,
scripts/update_selfhost_toolchain_review.sh, and
scripts/check_selfhost_toolchain_review_fresh.sh.

## Roadmap Execution Manifest

`docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json` is the deterministic,
machine-readable execution graph for every task in `ROADMAP.md`. It lets agents
select dependency-ready work without scraping phase prose or inventing missing
authority, risk, evidence, or rollback rules. It is a scheduling and review
artifact, not release evidence or completion authority.

Authority, in descending order:

1. `ROADMAP.md` owns task IDs, order, state, title, objective, and the required
   `done YYYY-MM-DD; evidence: ...; input: ...` annotation.
2. `policies/roadmap_execution_v0.1.json` owns execution profiles,
   prerequisites, risk/resource classes, owner surfaces, invariant guards,
   negative controls, rollback strategy, and task-specific dependency edges.
3. `docs/spec/ROADMAP_EXECUTION_MANIFEST_v0.1.schema.json` owns serialization.

The renderer hashes all three inputs and excludes timestamps and host paths.
The retained manifest is valid only when its identities and complete rendered
bytes match current inputs.

Every task resolves to exact prerequisites and unsatisfied prerequisites,
derived `start_ready` state, risk/resource class, repository-relative owner
surfaces, existing read-only guards, parallel-safe workstreams, negative
controls, expected inputs/deliverable/artifacts, non-automatic rollback, and
acceptance evidence. Mutable E0 observations never close a task. High and
critical tasks require independent verification and failed-evidence retention.

An open task always has `acceptance.status = "required"` and null evidence. A
done task is accepted only when its roadmap line has a valid annotation, all
prerequisites are done, cited scripts exist, no evidence command is an updater,
and the input identity is SHA-256-qualified. The manifest always records
`manifest_can_authorize_completion = false`.

Workstream prerequisites may name a task or workstream; a workstream resolves
to its final task. Sequential workstreams add the preceding sibling.
`task_prerequisites` records cross-cutting edges. Unknown references,
self-dependencies, cycles, duplicate tasks, incomplete prerequisites on done
tasks, and workstream coverage drift fail closed. `ready_task_ids` only grants
permission to begin scoped work, never to change policy or trust roots.

Commands:

~~~sh
# Read-only private render, byte comparison, nested validation, and adversarial fixtures.
bash scripts/check_roadmap_execution_manifest.sh

# Explicit regeneration after reviewing roadmap, policy, or schema changes.
bash scripts/update_roadmap_execution_manifest.sh
bash scripts/check_roadmap_execution_manifest.sh
~~~

Never hand-edit the generated manifest. The contract rejects duplicate/missing/
reordered tasks, duplicate JSON keys, unknown fields, stale identities,
dependency cycles, source/state/objective/readiness/summary drift, absolute
owner paths, non-check guards, self-authorized completion, mutating or missing
evidence, automatic rollback, and weakened evidence-preservation policy.

`kind = genesis/roadmap-execution-manifest-v0.1` and `version = 0.1` identify
the profile. Incompatible authority, dependency, acceptance, or serialization
changes require a new version and migration.

scripts/test_changed_fast.sh also defaults to a private temporary report/history
pair. Use scripts/update_test_changed_fast_metrics.sh to retain local E0 timing
history, or pass both --report and --history when an enclosing benchmark owns
temporary destinations. The legacy GENESIS_TEST_CHANGED_REPORT and
GENESIS_TEST_CHANGED_HISTORY variables are accepted only by the update command.

## Negative Controls

The boundary gate must reject at least:

- an unreviewed check;
- undeclared reachable compilation;
- persistent-output growth;
- refresh enabled by default;
- check-to-update invocation;
- undeclared network access;
- a reachable cache-reclaim helper; and
- a non-zero persistent-output target.

Live mutation smoke snapshots the lightweight migrated reports, executes those
checks, and proves the snapshots are unchanged. It also passes canary report
overrides and proves checks ignore them, while every renderer rejects a missing
explicit destination. Heavy build/runtime checks are enforced transitively by
the execution-closure audit and wrapper contracts, with focused mutation tests
that execute their real gates against temporary outputs.
