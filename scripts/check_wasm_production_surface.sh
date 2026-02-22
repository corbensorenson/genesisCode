#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-wasm-production-surface" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_WASM_PRODUCTION_SURFACE_CARGO_TARGET_DIR"

REPORT_PATH="${GENESIS_WASM_PRODUCTION_SURFACE_REPORT:-.genesis/perf/wasm_production_surface_report.json}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "wasm-production-surface: building default-feature wasm-bindgen artifact"
WASM_JS_PATH="$(bash scripts/wasm_bindgen_node.sh | tail -n 1)"
if [[ ! -f "$WASM_JS_PATH" ]]; then
  echo "wasm-production-surface: missing wasm-bindgen output: $WASM_JS_PATH" >&2
  exit 1
fi

FORBIDDEN_SYMBOLS=(
  "fmt_coreform_module_rust"
  "hash_coreform_module_rust"
  "eval_coreform_module_rust"
  "eval_coreform_module_with_gates_rust"
  "eval_module_rust("
  "eval_module_with_gates_rust("
)

REQUIRED_SYMBOLS=(
  "fmt_coreform_module_selfhost_with_artifact"
  "hash_coreform_module_selfhost_with_artifact"
  "eval_coreform_module_selfhost_with_artifact"
  "eval_module_selfhost_with_artifact("
)

forbidden_hits=()
for symbol in "${FORBIDDEN_SYMBOLS[@]}"; do
  if grep -Fq "$symbol" "$WASM_JS_PATH"; then
    forbidden_hits+=("$symbol")
  fi
done

missing_required=()
for symbol in "${REQUIRED_SYMBOLS[@]}"; do
  if ! grep -Fq "$symbol" "$WASM_JS_PATH"; then
    missing_required+=("$symbol")
  fi
done

python3 - "$REPORT_PATH" "$WASM_JS_PATH" "${forbidden_hits[*]:-}" "${missing_required[*]:-}" <<'PY'
import json
import pathlib
import sys

report_path = pathlib.Path(sys.argv[1])
wasm_js_path = pathlib.Path(sys.argv[2])
forbidden_hits = [x for x in sys.argv[3].split() if x]
missing_required = [x for x in sys.argv[4].split() if x]
ok = not forbidden_hits and not missing_required

doc = {
    "kind": "genesis/wasm-production-surface-v0.1",
    "ok": ok,
    "wasm_js_path": str(wasm_js_path),
    "forbidden_hits": forbidden_hits,
    "missing_required": missing_required,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"wasm-production-surface: wrote report {report_path}")
PY

if [[ "${#forbidden_hits[@]}" -gt 0 ]]; then
  echo "wasm-production-surface: forbidden parity-only symbols leaked into production artifact: ${forbidden_hits[*]}" >&2
  exit 1
fi
if [[ "${#missing_required[@]}" -gt 0 ]]; then
  echo "wasm-production-surface: required selfhost symbols missing: ${missing_required[*]}" >&2
  exit 1
fi

echo "wasm-production-surface: ok"
