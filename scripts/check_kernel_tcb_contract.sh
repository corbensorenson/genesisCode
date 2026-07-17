#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-kernel-tcb.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_kernel_tcb_contract_report.sh \
  "$TMP_DIR/kernel_tcb_contract_report.json"

python3 - "$TMP_DIR" <<'PY'
from pathlib import Path
import sys

temp = Path(sys.argv[1])
source = Path("policies/kernel_tcb_contract.toml").read_text(encoding="utf-8")
mutations = {
    "bad-role": (
        '"eval_treewalk.rs" = "reference-semantics"',
        '"eval_treewalk.rs" = "optimized-tier"',
    ),
    "missing-differential": (
        "reference_compiled_differential_matrix_covers_semantic_observables",
        "missing_reference_compiled_differential_matrix",
    ),
    "reference-import": ('  "CExpr",', '  "Term",'),
}
for name, (old, new) in mutations.items():
    if source.count(old) != 1:
        raise SystemExit(f"kernel-tcb-contract: mutation anchor drift: {name}")
    (temp / f"{name}.toml").write_text(source.replace(old, new), encoding="utf-8")
PY

negative_controls=0
for mutant in "$TMP_DIR"/*.toml; do
  if GENESIS_KERNEL_TCB_POLICY="$mutant" \
    bash scripts/render_kernel_tcb_contract_report.sh "$TMP_DIR/mutant-report.json" \
      >/dev/null 2>&1; then
    echo "kernel-tcb-contract: accepted mutant policy: $(basename "$mutant")" >&2
    exit 1
  fi
  negative_controls=$((negative_controls + 1))
done
echo "kernel-tcb-contract: self-test ok (negative_controls=$negative_controls)"
