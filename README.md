# GenesisCode (v0.2)

GenesisCode is an AI-first language/runtime project focused on deterministic evaluation, sealed error/effect boundaries, capability-gated host effects, and reproducible execution evidence.

The workspace builds a CLI binary named `genesis`.

## Documentation site

The combined tutorial and exhaustive reference is published at
[corbensorenson.github.io/genesisCode](https://corbensorenson.github.io/genesisCode/).
It includes guided human and agent learning paths plus generated indexes for every
frozen symbol, host operation, structured diagnostic, schema, example, and tracked
documentation authority. Keyboard navigation, mobile and print layouts, stable 404
recovery, canonical discovery metadata, and an agent-oriented `llms.txt` entrypoint
are part of the published contract. The site is a presentation of repository sources,
not a second semantic authority; `docs/INDEX.md` defines the canonical documentation
roots.

Render and validate the complete site locally with:

```sh
# genesis-doc-skip: documentation maintainer workflow; Quarto and Playwright are installed by docs-site CI
python3 scripts/render_quarto_reference.py --check
rm -rf _site
quarto render
python3 scripts/check_quarto_site.py
npm run test:docs-browser
```

The render writes `build-metadata.json` with the source commit, source-tree state,
reference-index hash, and whole-artifact hash. GitHub Pages deploys only the validated
artifact and then crawls the public tutorial, reference, agent index, sitemap,
provenance, and custom-404 surfaces against that commit.

## Design goals

- Pure, deterministic kernel evaluator (`Gλ` style)
- Unforgeable `UNHANDLED` / `EFFECT` / `ERROR` protocol values via seals
- Deny-by-default effect runner with deterministic logs and replay checks
- Package/obligation/evidence workflows designed for agent-driven iteration
- Strict hardening gates (panic guards, capability conformance, replay integrity)

## Repository layout

- `crates/`: Rust workspace crates (kernel, CLI, effects, obligations, patches, etc.)
- `prelude/`: Prelude modules and language surface helpers
- `selfhost/`: Selfhost artifact/toolchain material
- `examples/canonical_language/v0.1/`: signed valid/invalid teaching pairs for every foundational language workflow
- `docs/`: Specs, handoff, policy, and status docs
- `learn/`, `guides/`, `reference/`: Quarto learning paths and generated reference views
- `_quarto.yml`: exhaustive documentation-site render and navigation contract
- `scripts/`: test/health/profile gates and CI-style contract checks
- `ROADMAP.md`: canonical v0.2-to-v1 roadmap (truthful evidence, agent SDK, bounded runtime, semantic selfhost authority, platform delivery, and post-v1 research; strategic plan, not release evidence)
- `docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json`: generated machine-readable roadmap DAG with exact readiness, risk, ownership, guards, negative controls, rollback, and evidence requirements
- `docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json`: authenticated in-toto Statement v1, Genesis predicate, and SLSA Provenance v1 conformance vector
- `tools/genesis-evidence-verifier/`: standalone, read-only DSSE/SLSA/Genesis policy, signature, Merkle-tree, and artifact verifier outside the main CLI dependency graph
- `policies/evidence_storage_classes_v0.1.json`: enforced E0-E4 authority, in-tree fixture, immutable release-asset, and no-overwrite mirror boundaries
- `docs/program/EVIDENCE_ADVERSARIAL_MATRIX_v0.1.json`: executable missing-field, duplicate-key, path, version, replay, signature, source-freshness, and dirty-input rejection map
- `genesis.prerequisites.json`: authoritative versioned host-tool, feature-profile, and platform-SDK prerequisite manifest
- `policies/reference_host_profiles_v0.1.json`: benchmark-grade tier-1/tier-2 CPU, memory, filesystem, OS, compiler, and power-mode host authority
- `policies/perf/roadmap_workloads_v0.1.json`: exact, content-addressed PB-1 through PB-10 sources, outcomes, sizes, cache states, timeouts, and statistical protocols
- `docs/program/evidence/roadmap-baselines/`: append-only signed E0 raw-sample baselines; signatures protect fixture integrity but cannot self-promote evidence authority
- `docs/program/RELEASE_NOTES_v0.2.0.json`: generated E1 release facts covering compatibility, migrations, gaps, evidence, dependencies, and mandatory security checks without claiming those checks passed
- `docs/spec/GC_AGENT_PROFILE_v0.3.json`: frozen, content-addressed agent-training surface with executable semantics and mandatory experimental-syntax, host-only-operation, unavailable-target, nondeterministic-facility, and out-of-profile-capability exclusions
- `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`: generated <=4,000-byte core card with a parser-checked complete symbol/example manifest
- `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`: seven generated, source-hashed cards selected and embedded into `agent-plan` from declared intent
- `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`: content-addressed exact lookup for every frozen symbol without loading the full documentation tree
- `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json`: closed, content-addressed catalog of versioned production diagnostics with phases, spans, parameters, causes, and safe repairs
- `genesis.dependency-mirror.json`: closed fetch-once, content-addressed mirror, clean offline-build, and kernel network-denial policy
- `policies/cargo_cache_v0.1.json`: closed, content-addressed Cargo cache scopes that prevent script- and profile-specific rebuild islands

## GenesisBench

GenesisBench is the project’s benchmark-first adoption surface. It tests whether an
agent can learn GenesisCode from a frozen repository/runtime/documentation snapshot
and complete real language tasks under exact context, tool, capability, attempt, and
scoring rules. Quality is determined by executable artifacts, never model-judge
preference.

- Active profile: `docs/spec/GENESISBENCH_PROTOCOL_v0.1.json`
- Fixed Cold Acquisition agent: `docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json`
- Controlled ablations: eight predeclared conditions over the same nine lineages in `docs/spec/GENESISBENCH_REFERENCE_AGENT_ABLATIONS_v0.1.json`
- Canonical execution and adapter contract: `docs/spec/GENESISBENCH_FRONT_DOOR_v0.1.md`
- Signed append-only registry and lexicographic static leaderboard: `docs/spec/GENESISBENCH_REGISTRY_v0.1.json`
- Construct-validity policy and reproducible study: `policies/genesisbench_construct_validity_v0.1.json` and `benchmarks/genesisbench/v0.1/construct-validity/report.json`
- Normative explanation and tutorial: `guides/genesisbench.qmd`
- Contamination-attestation schema: `docs/spec/GENESISBENCH_CONTAMINATION_ATTESTATION_v0.1.schema.json`
- Public practice suite: nine independent lineages under 27 context conditions in `benchmarks/agent_tasks/v0.1/suite.json`
- Predeclared lineage analysis: `docs/spec/GENESISBENCH_ANALYSIS_PLAN_v0.1.json`
- Active temporal epoch: `docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json` (90 private lineages; public commitments and balance metadata only)
- Maintained temporal overlay: `docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.json`
- Deterministic analysis fixtures: `benchmarks/genesisbench/v0.1/analysis/`
- Canonical contamination fixture: `benchmarks/genesisbench/v0.1/contamination.fixture.json`
- Canonical eligibility fixture: `benchmarks/genesisbench/v0.1/eligibility.fixture.json`

Validate the complete frozen profile and classify the conformance run:

```sh
python3 scripts/lib/genesisbench_protocol.py --check --self-test
python3 scripts/lib/genesisbench_reference_agent.py --check --self-test
python3 scripts/lib/genesisbench_front_door.py check --self-test
python3 scripts/lib/genesisbench_registry.py check --self-test
python3 scripts/lib/genesisbench_analysis.py --check --self-test
python3 scripts/lib/genesisbench_protocol.py --check \
  --run examples/agent_benchmark_reproducibility/run.json \
  --attestation benchmarks/genesisbench/v0.1/contamination.fixture.json \
  --json
```

Run the permanently unranked deterministic conformance adapter through the same public front door used by real providers and local runtimes:

```sh
BENCH_TMP="$(mktemp -d "${TMPDIR:-/tmp}/genesisbench-quickstart.XXXXXX")"
cargo run -p gc_cli --bin genesis -- --json --selfhost-artifact selfhost/toolchain.gc bench run \
  --case generation-small \
  --adapter benchmarks/genesisbench/v0.1/adapters/deterministic-mock.json \
  --out "$BENCH_TMP/run"
cargo run -p gc_cli --bin genesis -- --json --selfhost-artifact selfhost/toolchain.gc bench replay \
  --run "$BENCH_TMP/run/run.json"
rm -rf "$BENCH_TMP"
```

Public references are explicitly `declared-contaminated` and unranked. Missing model
training provenance is `unknown`; only release-relative task precommitment plus
commitment and custody evidence can support `temporal-clean`. A new language is not,
by itself, evidence that a model has never seen it.

## Quickstart

Build everything:

```sh
cargo build --workspace
```

Diagnose the required host tools without installing or changing anything:

```sh
bash scripts/genesis_prerequisites.sh --profile core
```

Prepare the declared dependency mirror once, then prove a clean offline build:

```sh
# genesis-doc-skip: network-enabled mirror preparation and long-running isolated rebuild
MIRROR="$(bash scripts/update_dependency_mirror.sh --format path)"
bash scripts/test_offline_dependency_mirror.sh --mirror "$MIRROR"
```

Run the CLI:

```sh
cargo run -p gc_cli -- --help
```

Format CoreForm:

```sh
cargo run -p gc_cli -- fmt examples/hello_pkg/hello.gc --check
```

Evaluate pure code:

```sh
cargo run -p gc_cli -- eval examples/hello_pkg/hello.gc
```

Run effects with capability policy and deterministic log:

```sh
cargo run -p gc_cli -- run examples/effects_demo/read.gc --caps examples/effects_demo/caps.toml --log examples/effects_demo/read.gclog
cargo run -p gc_cli -- replay examples/effects_demo/read.gc --log examples/effects_demo/read.gclog
```

Package/testing flow:

```sh
cargo run -p gc_cli -- test --pkg examples/hello_pkg/package.toml
cargo run -p gc_cli -- pack --pkg examples/hello_pkg/package.toml
```

Apply semantic patch:

```sh
cargo run -p gc_cli -- apply-patch tests/spec/pkg_basic/pure.gcpatch --pkg tests/spec/pkg_basic/package.toml --caps tests/spec/pkg_basic/caps.toml
```

## Local development gates

Green front-door for a new agent session:

```sh
# genesis-doc-skip: aggregate developer gate; run directly when validating a full local checkout
bash scripts/check_green_front_door.sh
```

Fast changed-aware loop:

```sh
# genesis-doc-skip: developer gate exercised by scripts/check_green_front_door.sh
bash scripts/test_changed_fast.sh
```

The default loop keeps timing metrics temporary. Persist a reviewed local E0
history only with `bash scripts/update_test_changed_fast_metrics.sh`.

Alias / broader loop:

```sh
# genesis-doc-skip: developer gate exercised by scripts/check_green_front_door.sh
bash scripts/test_fast.sh
bash scripts/test_fast.sh --full
```

Strict profile used for release-quality agent readiness:

```sh
# genesis-doc-skip: long-running release-quality gate; run outside docs quickstart
bash scripts/check_upgrade_plan_health.sh --profile prepush-standard
```

Standalone release-hardening gates:

```sh
# genesis-doc-skip: covered by scripts/check_green_front_door.sh and CI standard/full profiles
bash scripts/check_versioning_release_hygiene.sh
bash scripts/check_gc_agent_profile.sh
bash scripts/check_gc_agent_core_card.sh
bash scripts/check_gc_agent_task_cards.sh
bash scripts/check_release_notes.sh
bash scripts/check_supply_chain.sh
bash scripts/check_release_smoke.sh
```

## Specs and core docs

- Docs index: `docs/INDEX.md`
- Primary design/paper: `docs/PAPER_v0.2.md`
- Technical handoff: `docs/TECH_HANDOFF.md`
- Versioning policy: `docs/spec/VERSIONING_v0.1.md`
- Generated release-note authority: `docs/program/RELEASE_NOTES_v0.2.0.json` with `policies/release_notes_v0.1.json`
- Frozen agent language profile: `docs/spec/GC_AGENT_PROFILE_v0.3.json` with `policies/gc_agent_profile_v0.3.json`
- Compact agent language card: `docs/spec/GC_AGENT_CORE_CARD_v0.3.md` with `docs/spec/GC_AGENT_CORE_CARD_v0.3.json`
- Intent-selected task cards: `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md` with embedded registry `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`
- Exact agent symbol lookup: `genesis --json agent-index --symbol <exact-name>` backed by `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`
- Exact diagnostic lookup: `genesis --json agent-index --diagnostic <exact-code>` backed by `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json`
- Version/format compatibility registry: `genesis.version-surfaces.json` and `docs/spec/VERSION_SURFACES_v0.1.md`
- Release smoke contract: `docs/spec/RELEASE_SMOKE_v0.1.md`
- Read-only check/update contract: `docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md`
- Machine-readable gate manifest: `genesis.gates.json`
- Dependency mirror/offline contract: `docs/spec/DEPENDENCY_MIRROR_v0.1.md`
- Changelog: `CHANGELOG.md`
- Seals/dispatch/replay spec: `docs/spec/SEALS_DISPATCH_REPLAY.md`
- Patch schema spec: `docs/spec/PATCH_SCHEMA.md`
- Capability surface matrix: `feature_matrix.md`

## License

Dual licensed under either:

- Apache-2.0 (`LICENSE-APACHE`)
- MIT (`LICENSE-MIT`)

See `LICENSE` for the dual-license notice.
