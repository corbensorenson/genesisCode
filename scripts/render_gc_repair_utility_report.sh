#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 1 ]]; then
  echo "usage: $0 <report-output>" >&2
  exit 2
fi

REPORT_OUT="$1"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-repair-utility.XXXXXX")"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

source "$ROOT_DIR/scripts/lib/gate_telemetry.sh"
source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "gc-repair-utility" \
  root-host

cargo build -p gc_cli --quiet
GENESIS_BIN="$CARGO_TARGET_DIR/debug/genesis"
SELFHOST_ARTIFACT="$ROOT_DIR/selfhost/toolchain.gc"
[[ -x "$GENESIS_BIN" ]] || {
  echo "gc-repair-utility: missing genesis binary: $GENESIS_BIN" >&2
  exit 1
}
[[ -f "$SELFHOST_ARTIFACT" ]] || {
  echo "gc-repair-utility: missing selfhost artifact: $SELFHOST_ARTIFACT" >&2
  exit 1
}

for sample in 1 2; do
  python3 scripts/lib/gc_repair_utility_runner.py \
    --genesis "$GENESIS_BIN" \
    --selfhost-artifact "$SELFHOST_ARTIFACT" \
    --output "$TMP_DIR/report-$sample.json"
  python3 scripts/lib/gc_repair_utility.py \
    --check \
    --report "$TMP_DIR/report-$sample.json"
done

cmp -s "$TMP_DIR/report-1.json" "$TMP_DIR/report-2.json" || {
  echo "gc-repair-utility: repeated executions were not byte-identical" >&2
  diff -u "$TMP_DIR/report-1.json" "$TMP_DIR/report-2.json" >&2 || true
  exit 1
}

mkdir -p "$(dirname "$REPORT_OUT")"
cp "$TMP_DIR/report-1.json" "$REPORT_OUT"
echo "gc-repair-utility: deterministic replay ok"
