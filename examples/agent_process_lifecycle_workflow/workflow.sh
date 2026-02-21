#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_process_lifecycle_workflow"
GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"

if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

WORK_DIR="$TMP_DIR/work"
cp -R "$EXAMPLE_DIR" "$WORK_DIR"
chmod +x "$WORK_DIR/tools/host_bridge.sh"

ART="$TMP_DIR/selfhost_toolchain.gc"
REPO_ART="$ROOT_DIR/selfhost/toolchain.gc"
if [[ -f "$REPO_ART" ]]; then
  cp "$REPO_ART" "$ART"
else
  "$GENESIS_BIN" selfhost-artifact --out "$ART" >/dev/null
fi

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
  echo "agent-process-lifecycle-workflow: run/replay mismatch: run=$run_out replay=$replay_out" >&2
  exit 1
fi

if [[ "$run_out" != *":ok true"* || "$run_out" != *":exit 0"* ]]; then
  echo "agent-process-lifecycle-workflow: expected ok=true and exit=0, got=$run_out" >&2
  exit 1
fi

echo "agent-process-lifecycle-workflow: ok replay=$run_out"
