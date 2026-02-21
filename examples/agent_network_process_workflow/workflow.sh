#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_network_process_workflow"
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

acceptance_h="$(
  cd "$WORK_DIR"
  "$GENESIS_BIN" \
    --selfhost-only \
    --selfhost-artifact "$ART" \
    test --pkg "package.toml" \
    | tr -d '\n'
)"
if [[ ! "$acceptance_h" =~ ^[0-9a-f]{64}$ ]]; then
  echo "agent-network-process-workflow: expected acceptance hash, got: $acceptance_h" >&2
  exit 1
fi

run_out="$(
  cd "$WORK_DIR"
  "$GENESIS_BIN" \
    --selfhost-only \
    --selfhost-artifact "$ART" \
    run "workflow_run.gc" \
    --caps "caps.toml" \
    --log "workflow_run.gclog" \
    | tr -d '\n'
)"

replay_out="$(
  cd "$WORK_DIR"
  "$GENESIS_BIN" \
    --selfhost-only \
    --selfhost-artifact "$ART" \
    replay "workflow_run.gc" \
    --log "workflow_run.gclog" \
    | tr -d '\n'
)"

if [[ "$run_out" != "$replay_out" ]]; then
  echo "agent-network-process-workflow: run/replay mismatch: run=$run_out replay=$replay_out" >&2
  exit 1
fi

if [[ "$run_out" != *":http-status 200"* || "$run_out" != *":exit 0"* ]]; then
  echo "agent-network-process-workflow: expected http-status=200 and exit=0, got=$run_out" >&2
  exit 1
fi

vcs_h="$(
  cd "$WORK_DIR"
  "$GENESIS_BIN" \
    --selfhost-only \
    --selfhost-artifact "$ART" \
    vcs hash --in "service.gc" --engine selfhost \
    | tr -d '\n'
)"
if [[ ! "$vcs_h" =~ ^[0-9a-f]{64}$ ]]; then
  echo "agent-network-process-workflow: expected VCS hash, got: $vcs_h" >&2
  exit 1
fi

echo "agent-network-process-workflow: ok acceptance=$acceptance_h vcs=$vcs_h replay=$run_out"
