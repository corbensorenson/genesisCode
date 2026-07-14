#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

INPUTS=(
  policies/perf/roadmap_workloads_v0.1.json
  docs/spec/ROADMAP_WORKLOADS_v0.1.schema.json
  scripts/lib/roadmap_workloads.py
  benchmarks/roadmap/v0.1/pb1_fib_25.gc
  benchmarks/roadmap/v0.1/pb4_vector_1000000.gc
  benchmarks/roadmap/v0.1/pb5_map_100000.gc
  benchmarks/roadmap/v0.1/pb6_trivial_check.gc
  benchmarks/roadmap/v0.1/pb8_warm_request.jsonl
  benchmarks/roadmap/v0.1/pb9_semantic_parity_corpus.json
  benchmarks/roadmap/v0.1/pb10_bootstrap_inputs.json
  policies/reference_host_profiles_v0.1.json
  prelude/prelude.gc
  selfhost/parse.gc
  selfhost/toolchain.gc
  selfhost/toolchain_manifest.gc
  tests/spec/coreform/app_sugar.in.gc
  tests/spec/coreform/map_order.in.gc
  tests/spec/pkg_fail_budgets/budget.gc
  tests/spec/pkg_fail_determinism/fail.gc
)

snapshot() {
  python3 - "${INPUTS[@]}" <<'PY'
from hashlib import sha256
from pathlib import Path
import sys

for raw in sys.argv[1:]:
    path = Path(raw)
    print(f"{sha256(path.read_bytes()).hexdigest()}  {path.as_posix()}")
PY
}

before="$(snapshot)"
python3 scripts/lib/roadmap_workloads.py check
python3 scripts/lib/roadmap_workloads.py self-test
identity_a="$(python3 scripts/lib/roadmap_workloads.py identity)"
identity_b="$(python3 scripts/lib/roadmap_workloads.py identity)"
[[ "$identity_a" == "$identity_b" ]] || {
  echo "roadmap-workloads: repeated policy identities differ" >&2
  exit 1
}
after="$(snapshot)"
[[ "$before" == "$after" ]] || {
  echo "roadmap-workloads: check mutated retained inputs" >&2
  exit 1
}

echo "roadmap-workloads-contract: ok (workloads=10 active=4 blocked=5 decision_gated=1 controls=12 check_mode=read_only)"
