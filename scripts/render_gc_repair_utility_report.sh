#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 1 ]]; then
  echo "usage: $0 <report-output>" >&2
  exit 2
fi

REPORT_OUT="$1"
REPEATS="${GENESIS_GC_REPAIR_UTILITY_REPEATS:-2}"
[[ "$REPEATS" == "1" || "$REPEATS" == "2" ]] || {
  echo "gc-repair-utility: GENESIS_GC_REPAIR_UTILITY_REPEATS must be 1 or 2" >&2
  exit 2
}
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

declare -a REPLAY_PIDS=()
for ((sample = 1; sample <= REPEATS; sample++)); do
  (
    python3 scripts/lib/gc_repair_utility_runner.py \
      --genesis "$GENESIS_BIN" \
      --selfhost-artifact "$SELFHOST_ARTIFACT" \
      --output "$TMP_DIR/report-$sample.json"
    python3 scripts/lib/gc_repair_utility.py \
      --check \
      --report "$TMP_DIR/report-$sample.json"
  ) &
  REPLAY_PIDS+=("$!")
done

REPLAY_STATUS=0
for pid in "${REPLAY_PIDS[@]}"; do
  if ! wait "$pid"; then
    REPLAY_STATUS=1
  fi
done
if (( REPLAY_STATUS != 0 )); then
  echo "gc-repair-utility: one or more replay workers failed" >&2
  exit 1
fi

if [[ "$REPEATS" == "2" ]]; then
  cmp -s "$TMP_DIR/report-1.json" "$TMP_DIR/report-2.json" || {
    echo "gc-repair-utility: repeated executions were not byte-identical" >&2
    diff -u "$TMP_DIR/report-1.json" "$TMP_DIR/report-2.json" >&2 || true
    exit 1
  }
fi

mkdir -p "$(dirname "$REPORT_OUT")"
cp "$TMP_DIR/report-1.json" "$REPORT_OUT"
if [[ "$REPEATS" == "2" ]]; then
  echo "gc-repair-utility: deterministic replay ok"
else
  echo "gc-repair-utility: staged render complete; replay validation delegated"
fi
