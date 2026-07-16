#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
usage: scripts/update_agent_authoring_bundle.sh <component>

components:
  profile
  diagnostics
  derived-agent-surfaces
  canonical-examples
  task-benchmarks
  analysis-fixtures
  held-out-evaluation
  benchmark-scoring
  construct-validity
  benchmark-run
  protocol-fixtures
  baseline-authority
  corpus
  all
EOF
}

atomic_render() {
  local destination="$1"
  shift
  local directory
  local temporary
  directory="$(dirname "$destination")"
  temporary="$(mktemp "$directory/.genesis-authoring.XXXXXX")"
  if ! "$@" >"$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  mv "$temporary" "$destination"
}

configure_genesis_binary() {
  if [[ -n "${GENESIS_AUTHORING_BIN:-}" ]]; then
    return
  fi
  source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
  genesis_configure_cargo_target_dir "$ROOT_DIR" "agent-authoring-bundle-update" root-host
  cargo build -p gc_cli --bin genesis --locked --offline
  GENESIS_AUTHORING_BIN="$CARGO_TARGET_DIR/debug/genesis"
}

validate_unless_staged() {
  if [[ "${GENESIS_GENERATED_AUTHORITY_STAGE:-0}" != "1" ]]; then
    "$@"
  fi
}

update_profile() {
  atomic_render docs/spec/GC_AGENT_PROFILE_v0.3.json \
    python3 scripts/lib/gc_agent_profile.py --render
  validate_unless_staged bash scripts/check_gc_agent_profile.sh
  echo "update-agent-authoring-bundle: refreshed profile"
}

update_diagnostics() {
  bash scripts/update_gc_diagnostic_catalog.sh
  bash scripts/update_cli_diagnostic_goldens.sh
  bash scripts/update_gc_repair_utility_report.sh
  validate_unless_staged bash scripts/check_cli_diagnostics_contract.sh
  echo "update-agent-authoring-bundle: refreshed diagnostic authorities and evidence"
}

update_derived_agent_surfaces() {
  bash scripts/update_gc_agent_core_card.sh
  bash scripts/update_gc_agent_task_cards.sh
  bash scripts/update_gc_agent_symbol_index.sh
  validate_unless_staged bash scripts/check_gc_agent_symbol_index.sh
  python3 scripts/lib/genesisbench_reference_agent.py --write
  python3 scripts/lib/genesisbench_reference_agent.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed derived agent surfaces"
}

update_canonical_examples() {
  python3 scripts/lib/gc_canonical_examples.py --refresh
  python3 scripts/lib/gc_canonical_examples.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed canonical examples"
}

update_task_benchmarks() {
  python3 scripts/lib/gc_task_benchmarks.py --refresh
  python3 scripts/lib/gc_task_benchmarks.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed task benchmarks"
}

update_analysis_fixtures() {
  python3 scripts/lib/genesisbench_analysis.py --refresh-fixtures
  python3 scripts/lib/genesisbench_analysis.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed analysis authority and fixtures"
}

update_held_out_evaluation() {
  python3 scripts/lib/gc_held_out_evaluation.py --refresh-profile
  local epoch
  local private_pack
  local temporary
  epoch="$(python3 - <<'PY'
import json
document = json.load(open("docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json", encoding="utf-8"))
active = [row["id"] for row in document["epochs"] if row["status"] == "active"]
if len(active) != 1:
    raise SystemExit("held-out updater requires exactly one active epoch")
print(active[0])
PY
)"
  private_pack=".genesis/private/agent-evaluation/$epoch/pack.json"
  [[ -f "$private_pack" ]] || {
    echo "update-agent-authoring-bundle: missing private opening pack: $private_pack" >&2
    return 1
  }
  temporary="$(mktemp "$ROOT_DIR/docs/program/.genesisbench-temporal-audit.XXXXXX")"
  if ! python3 scripts/lib/gc_held_out_evaluation.py \
    --check \
    --verify-private "$private_pack" \
    --audit-out "$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  mv "$temporary" docs/program/GENESISBENCH_TEMPORAL_EPOCH_AUDIT_v0.1.json
  python3 scripts/lib/gc_held_out_evaluation.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed held-out commitments and opening audit"
}

update_benchmark_scoring() {
  atomic_render docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json \
    python3 scripts/lib/gc_agent_scoring.py --render
  python3 scripts/lib/gc_agent_scoring.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed benchmark scoring"
}

update_construct_validity() {
  configure_genesis_binary
  local temporary
  temporary="$(mktemp "${TMPDIR:-/tmp}/genesisbench-construct-validity.XXXXXX")"
  if ! python3 scripts/lib/genesisbench_construct_validity.py \
    --run \
    --genesis-bin "$GENESIS_AUTHORING_BIN" \
    --selfhost-artifact selfhost/toolchain.gc \
    --output "$temporary"; then
    rm -f "$temporary"
    return 1
  fi
  mv "$temporary" benchmarks/genesisbench/v0.1/construct-validity/report.json
  python3 scripts/lib/genesisbench_construct_validity.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed construct-validity study"
}

update_benchmark_run() {
  configure_genesis_binary
  python3 scripts/lib/gc_agent_benchmark_run.py \
    --refresh-example \
    --genesis-bin "$GENESIS_AUTHORING_BIN" \
    --selfhost-artifact selfhost/toolchain.gc
  python3 scripts/lib/gc_agent_benchmark_run.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed benchmark run"
}

update_protocol_fixtures() {
  python3 scripts/lib/genesisbench_protocol.py --refresh-profile
  python3 scripts/lib/genesisbench_protocol.py --refresh-fixtures
  python3 scripts/lib/genesisbench_protocol.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed protocol authority and fixtures"
}

update_baseline_authority() {
  python3 scripts/lib/genesisbench_baselines.py --write
  python3 scripts/lib/genesisbench_baselines.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed baseline-study authority and fixtures"
}

update_corpus() {
  python3 scripts/lib/gc_agent_corpus.py --refresh
  python3 scripts/lib/gc_agent_corpus.py --check --self-test
  echo "update-agent-authoring-bundle: refreshed corpus"
}

component="${1:-}"
case "$component" in
  profile) update_profile ;;
  diagnostics) update_diagnostics ;;
  derived-agent-surfaces) update_derived_agent_surfaces ;;
  canonical-examples) update_canonical_examples ;;
  task-benchmarks) update_task_benchmarks ;;
  analysis-fixtures) update_analysis_fixtures ;;
  held-out-evaluation) update_held_out_evaluation ;;
  benchmark-scoring) update_benchmark_scoring ;;
  construct-validity) update_construct_validity ;;
  benchmark-run) update_benchmark_run ;;
  protocol-fixtures) update_protocol_fixtures ;;
  baseline-authority) update_baseline_authority ;;
  corpus) update_corpus ;;
  all)
    update_profile
    update_diagnostics
    update_derived_agent_surfaces
    update_canonical_examples
    update_task_benchmarks
    update_analysis_fixtures
    update_held_out_evaluation
    update_benchmark_scoring
    update_construct_validity
    update_benchmark_run
    update_protocol_fixtures
    update_baseline_authority
    update_corpus
    validate_unless_staged bash scripts/check_agent_authoring_bundle.sh
    ;;
  -h|--help) usage ;;
  *) usage >&2; exit 2 ;;
esac
