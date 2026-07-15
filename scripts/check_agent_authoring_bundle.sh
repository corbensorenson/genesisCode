#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

python3 scripts/lib/gc_agent_corpus.py --check --self-test
python3 scripts/lib/gc_canonical_examples.py --check --self-test
python3 scripts/lib/gc_task_benchmarks.py --check --self-test
python3 scripts/lib/gc_held_out_evaluation.py --check --self-test
python3 scripts/lib/gc_agent_scoring.py --check --self-test
python3 scripts/lib/gc_agent_benchmark_run.py --check --self-test
python3 scripts/lib/genesisbench_protocol.py --check --self-test
python3 scripts/lib/genesisbench_analysis.py --check --self-test
protocol_report="$(mktemp)"
trap 'rm -f "$protocol_report"' EXIT
python3 scripts/lib/genesisbench_protocol.py \
  --check \
  --run examples/agent_benchmark_reproducibility/run.json \
  --attestation benchmarks/genesisbench/v0.1/contamination.fixture.json \
  --json >"$protocol_report"
cmp -s "$protocol_report" benchmarks/genesisbench/v0.1/eligibility.fixture.json || {
  echo "agent-authoring-bundle: stale GenesisBench eligibility fixture" >&2
  exit 1
}

if git ls-files '.genesis/private/agent-evaluation/**' | grep -q .; then
  echo "agent-authoring-bundle: private held-out custody material is tracked" >&2
  exit 1
fi

BUNDLE="docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md"
AGENT_INDEX_SPEC="docs/spec/AGENT_INDEX_v0.1.md"
AGENT_INDEX_CMD="crates/gc_cli_driver/src/cmd_agent_index.rs"
CANONICAL_TEST="crates/gc_cli/tests/cli_canonical_language_examples.rs"
TASK_BENCHMARK_TEST="crates/gc_cli/tests/cli_agent_task_benchmarks.rs"
SCORING_TEST="crates/gc_cli/tests/cli_agent_benchmark_scoring.rs"
RUN_TEST="crates/gc_cli/tests/cli_agent_benchmark_run.rs"

[[ -f "$BUNDLE" ]] || {
  echo "agent-authoring-bundle: missing bundle doc: $BUNDLE" >&2
  exit 1
}
[[ -f "$AGENT_INDEX_SPEC" ]] || {
  echo "agent-authoring-bundle: missing agent index spec: $AGENT_INDEX_SPEC" >&2
  exit 1
}
[[ -f "$AGENT_INDEX_CMD" ]] || {
  echo "agent-authoring-bundle: missing agent index command source: $AGENT_INDEX_CMD" >&2
  exit 1
}
[[ -f "$CANONICAL_TEST" ]] || {
  echo "agent-authoring-bundle: missing canonical production test: $CANONICAL_TEST" >&2
  exit 1
}
[[ -f "$TASK_BENCHMARK_TEST" ]] || {
  echo "agent-authoring-bundle: missing task benchmark production test: $TASK_BENCHMARK_TEST" >&2
  exit 1
}
[[ -f "$SCORING_TEST" ]] || {
  echo "agent-authoring-bundle: missing scoring production test: $SCORING_TEST" >&2
  exit 1
}
[[ -f "$RUN_TEST" ]] || {
  echo "agent-authoring-bundle: missing benchmark-run production test: $RUN_TEST" >&2
  exit 1
}

python3 - "$BUNDLE" "$AGENT_INDEX_SPEC" "$AGENT_INDEX_CMD" "$CANONICAL_TEST" "$TASK_BENCHMARK_TEST" "$SCORING_TEST" "$RUN_TEST" <<'PY'
import pathlib
import re
import sys

bundle_path = pathlib.Path(sys.argv[1])
agent_index_spec_path = pathlib.Path(sys.argv[2])
agent_index_cmd_path = pathlib.Path(sys.argv[3])
canonical_test_path = pathlib.Path(sys.argv[4])
task_benchmark_test_path = pathlib.Path(sys.argv[5])
scoring_test_path = pathlib.Path(sys.argv[6])
run_test_path = pathlib.Path(sys.argv[7])

bundle = bundle_path.read_text(encoding="utf-8")
agent_index_spec = agent_index_spec_path.read_text(encoding="utf-8")
agent_index_cmd = agent_index_cmd_path.read_text(encoding="utf-8")
canonical_test = canonical_test_path.read_text(encoding="utf-8")
task_benchmark_test = task_benchmark_test_path.read_text(encoding="utf-8")
scoring_test = scoring_test_path.read_text(encoding="utf-8")
run_test = run_test_path.read_text(encoding="utf-8")

include_re = re.compile(r"^- `([^`]+)`\s*$", re.MULTILINE)
included_paths = include_re.findall(bundle)
if not included_paths:
    raise SystemExit("agent-authoring-bundle: no included specs found in bundle")

required_included = [
    "docs/spec/CLI_TOOLING_BUNDLE_v0.1.md",
    "docs/spec/GC_AGENT_CORE_CARD_v0.3.md",
    "docs/spec/GC_AGENT_CORPUS_v0.1.json",
    "docs/spec/GC_AGENT_CORPUS_v0.1.schema.json",
    "docs/spec/GC_CANONICAL_EXAMPLES_v0.1.schema.json",
    "docs/spec/GC_AGENT_TASK_BENCHMARK_v0.1.schema.json",
    "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json",
    "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.schema.json",
    "docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json",
    "docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json",
    "docs/spec/GENESISBENCH_ELIGIBILITY_v0.1.schema.json",
    "docs/spec/GENESISBENCH_CONTAMINATION_ATTESTATION_v0.1.schema.json",
    "docs/spec/GENESISBENCH_PROTOCOL_v0.1.json",
    "docs/spec/GENESISBENCH_PROTOCOL_v0.1.schema.json",
    "docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json",
    "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json",
    "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.schema.json",
    "docs/spec/GC_AGENT_HELD_OUT_PRIVATE_PACK_v0.1.schema.json",
    "docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.json",
    "docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.schema.json",
    "docs/program/GENESISBENCH_TEMPORAL_EPOCH_AUDIT_v0.1.json",
    "docs/spec/GC_AGENT_PROFILE_v0.3.json",
    "docs/spec/GC_AGENT_TASK_CARDS_v0.3.md",
    "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json",
    "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json",
    "docs/spec/GCPM_BUNDLE_v0.1.md",
    "docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md",
    "docs/spec/TESTING_BUNDLE_v0.1.md",
    "docs/spec/AGENT_INDEX_v0.1.md",
    "docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md",
    "docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md",
    "docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json",
    "docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md",
    "docs/spec/GENESISBENCH_ADAPTATION_MANIFEST_v0.1.schema.json",
    "docs/spec/GENESISBENCH_HARDWARE_EVIDENCE_v0.1.schema.json",
    "docs/spec/GENESISBENCH_SCAFFOLD_MANIFEST_v0.1.schema.json",
    "docs/skill_pack/write_genesiscode_v1/manifest.json",
    "docs/write_genesisCode_skill.md",
    "examples/canonical_language/v0.1/README.md",
    "examples/canonical_language/v0.1/suite.json",
    "benchmarks/agent_tasks/v0.1/suite.json",
    "benchmarks/genesisbench/v0.1/README.md",
    "benchmarks/genesisbench/v0.1/contamination.fixture.json",
    "benchmarks/genesisbench/v0.1/eligibility.fixture.json",
    "guides/genesisbench.qmd",
    "scripts/lib/gc_agent_scoring.py",
    "scripts/lib/gc_agent_scoring_contract.py",
    "scripts/lib/gc_agent_benchmark_run.py",
    "scripts/lib/genesisbench_protocol.py",
    "scripts/lib/genesisbench_protocol_contract.py",
    "scripts/lib/genesisbench_contamination.py",
    "scripts/lib/genesisbench_protocol_run.py",
    "scripts/lib/genesisbench_tracks.py",
    "scripts/lib/genesisbench_eligibility.py",
    "scripts/lib/gc_held_out_evaluation.py",
    "scripts/lib/gc_capability_lease.py",
    "examples/agent_benchmark_reproducibility/run.json",
    "crates/gc_cli/tests/cli_agent_benchmark_run.rs",
]
missing_required = [p for p in required_included if p not in included_paths]
if missing_required:
    raise SystemExit(
        "agent-authoring-bundle: missing required included spec path(s): "
        + ", ".join(missing_required)
    )

for p in included_paths:
    if not pathlib.Path(p).is_file():
        raise SystemExit(f"agent-authoring-bundle: listed path does not exist: {p}")

legacy_header = "## Legacy Split Docs (must stay marked)"
if legacy_header not in bundle:
    raise SystemExit("agent-authoring-bundle: missing legacy split docs section")

legacy_block = bundle.split(legacy_header, 1)[1]
legacy_paths = include_re.findall(legacy_block)
if not legacy_paths:
    raise SystemExit("agent-authoring-bundle: legacy split docs section has no paths")

for p in legacy_paths:
    doc = pathlib.Path(p)
    if not doc.is_file():
        raise SystemExit(f"agent-authoring-bundle: legacy split doc missing: {p}")
    src = doc.read_text(encoding="utf-8")
    if "Bundle Entry:" not in src or "Legacy Split Doc:" not in src:
        raise SystemExit(
            f"agent-authoring-bundle: legacy split doc is not clearly marked: {p}"
        )

bundle_rel = bundle_path.as_posix()
if bundle_rel not in agent_index_spec:
    raise SystemExit(
        "agent-authoring-bundle: AGENT_INDEX spec must reference the authoring bundle path"
    )
if bundle_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: cmd_agent_index must expose authoring bundle in docs map"
    )

profile_rel = "docs/spec/GC_AGENT_PROFILE_v0.3.json"
if profile_rel not in agent_index_spec or profile_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index spec and command must expose GC-AGENT-v0.3"
    )

card_rel = "docs/spec/GC_AGENT_CORE_CARD_v0.3.md"
if card_rel not in agent_index_spec or card_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index spec and command must expose the compact core card"
    )

task_cards_rel = "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json"
if task_cards_rel not in agent_index_spec or task_cards_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index spec and command must expose task cards"
    )

symbol_index_rel = "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"
if symbol_index_rel not in agent_index_spec or symbol_index_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index spec and command must expose exact symbol lookup"
    )

corpus_rel = "docs/spec/GC_AGENT_CORPUS_v0.1.json"
if corpus_rel not in agent_index_spec or corpus_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index spec and command must expose the corpus manifest"
    )

examples_rel = "examples/canonical_language/v0.1/suite.json"
if examples_rel not in agent_index_spec or examples_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index spec and command must expose canonical examples"
    )
if examples_rel not in canonical_test or "cargo_bin_cmd!(\"genesis\")" not in canonical_test:
    raise SystemExit(
        "agent-authoring-bundle: canonical examples need a shipped genesis integration test"
    )

benchmark_rel = "benchmarks/agent_tasks/v0.1/suite.json"
if benchmark_rel not in agent_index_spec or benchmark_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index must expose the task benchmark"
    )
if benchmark_rel not in task_benchmark_test or "cargo_bin_cmd!(\"genesis\")" not in task_benchmark_test:
    raise SystemExit(
        "agent-authoring-bundle: task benchmark needs a shipped genesis integration test"
    )

held_out_rel = "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json"
if held_out_rel not in agent_index_spec or held_out_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index must expose held-out commitments"
    )

scoring_rel = "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"
score_schema_rel = "docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json"
for path in (scoring_rel, score_schema_rel):
    if path not in agent_index_spec or path not in agent_index_cmd:
        raise SystemExit(
            f"agent-authoring-bundle: agent index must expose scoring authority: {path}"
        )
if (
    "scripts/lib/gc_agent_scoring.py" not in scoring_test
    or 'env!("CARGO_BIN_EXE_genesis")' not in scoring_test
    or "modelSpecificMetrics" not in scoring_test
):
    raise SystemExit(
        "agent-authoring-bundle: scoring needs shipped-binary and model-separation integration coverage"
    )

run_schema_rel = "docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json"
model_effect_rel = "docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json"
for path in (run_schema_rel, model_effect_rel):
    if path not in agent_index_spec or path not in agent_index_cmd:
        raise SystemExit(
            f"agent-authoring-bundle: agent index must expose run authority: {path}"
        )
if (
    "scripts/lib/gc_agent_benchmark_run.py" not in run_test
    or 'cargo_bin_cmd!("genesis")' not in run_test
    or "model-effect.gclog" not in run_test
):
    raise SystemExit(
        "agent-authoring-bundle: benchmark run needs validator, shipped-binary, and replay coverage"
    )

protocol_rel = "docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"
if protocol_rel not in agent_index_spec or protocol_rel not in agent_index_cmd:
    raise SystemExit(
        "agent-authoring-bundle: agent index must expose the GenesisBench profile"
    )

analysis_paths = (
    "docs/spec/GENESISBENCH_ANALYSIS_PLAN_v0.1.json",
    "docs/spec/GENESISBENCH_OBSERVATIONS_v0.1.schema.json",
    "docs/spec/GENESISBENCH_ANALYSIS_REPORT_v0.1.schema.json",
)
for path in analysis_paths:
    if path not in agent_index_spec or path not in agent_index_cmd:
        raise SystemExit(
            f"agent-authoring-bundle: agent index must expose lineage analysis authority: {path}"
        )

temporal_paths = (
    "docs/program/GENESISBENCH_TEMPORAL_EPOCH_AUDIT_v0.1.json",
    "docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.json",
)
for path in temporal_paths:
    if path not in agent_index_spec or path not in agent_index_cmd:
        raise SystemExit(
            f"agent-authoring-bundle: agent index must expose temporal epoch authority: {path}"
        )

print(
    "agent-authoring-bundle: ok "
    f"(included={len(included_paths)} legacy_marked={len(legacy_paths)})"
)
PY
