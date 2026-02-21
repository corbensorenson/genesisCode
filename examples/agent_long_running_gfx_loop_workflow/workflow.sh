#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_long_running_gfx_loop_workflow"
GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"

if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

WORK_DIR="$TMP_DIR/work"
cp -R "$EXAMPLE_DIR" "$WORK_DIR"

ART="$TMP_DIR/selfhost_toolchain.gc"
REPO_ART="$ROOT_DIR/selfhost/toolchain.gc"
if [[ -f "$REPO_ART" ]]; then
  cp "$REPO_ART" "$ART"
else
  "$GENESIS_BIN" selfhost-artifact --out "$ART" >/dev/null
fi

acceptance_h="$("$GENESIS_BIN" \
  --selfhost-only \
  --selfhost-artifact "$ART" \
  test --pkg "$WORK_DIR/package.toml" \
  | tr -d '\n')"
if [[ ! "$acceptance_h" =~ ^[0-9a-f]{64}$ ]]; then
  echo "agent-long-running-gfx-loop-workflow: expected acceptance hash, got: $acceptance_h" >&2
  exit 1
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
  echo "agent-long-running-gfx-loop-workflow: run/replay mismatch: run=$run_out replay=$replay_out" >&2
  exit 1
fi

if [[ "$run_out" != *":frames 64"* ]]; then
  echo "agent-long-running-gfx-loop-workflow: expected 64 frames in output, got=$run_out" >&2
  exit 1
fi

vcs_h="$("$GENESIS_BIN" \
  --selfhost-only \
  --selfhost-artifact "$ART" \
  vcs hash --in "$WORK_DIR/loop.gc" --engine selfhost \
  | tr -d '\n')"
if [[ ! "$vcs_h" =~ ^[0-9a-f]{64}$ ]]; then
  echo "agent-long-running-gfx-loop-workflow: expected VCS hash, got: $vcs_h" >&2
  exit 1
fi

echo "agent-long-running-gfx-loop-workflow: ok acceptance=$acceptance_h vcs=$vcs_h replay=$run_out"
