#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_durable_data_workflow"
GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"

cargo build -p gc_cli >/dev/null

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

WORK_DIR="$TMP_DIR/work"
cp -R "$EXAMPLE_DIR" "$WORK_DIR"
chmod +x "$WORK_DIR/tools/host_bridge.sh"

ART="$TMP_DIR/selfhost_toolchain.gc"
"$GENESIS_BIN" selfhost-artifact --out "$ART" >/dev/null

run_log="$WORK_DIR/workflow_run.gclog"
run_out="$("$GENESIS_BIN" \
  --selfhost-only \
  --selfhost-artifact "$ART" \
  run "$WORK_DIR/workflow_run.gc" \
  --caps "$WORK_DIR/caps.toml" \
  --log "$run_log" \
  | tr -d '\n')"

replay_out="$("$GENESIS_BIN" \
  --selfhost-only \
  --selfhost-artifact "$ART" \
  replay "$WORK_DIR/workflow_run.gc" \
  --log "$run_log" \
  | tr -d '\n')"

if [[ "$run_out" != "$replay_out" ]]; then
  echo "agent-durable-data-workflow: run/replay mismatch: run=$run_out replay=$replay_out" >&2
  exit 1
fi

if [[ "$run_out" != *":ok true"* || "$run_out" != *"affected-rows"* || "$run_out" != *"store-id \"kv-1\""* ]]; then
  echo "agent-durable-data-workflow: expected durable-data fields, got=$run_out" >&2
  exit 1
fi

echo "agent-durable-data-workflow: ok replay=$run_out"
