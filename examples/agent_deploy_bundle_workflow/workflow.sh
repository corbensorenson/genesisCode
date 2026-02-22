#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_deploy_bundle_workflow"
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

g() {
  "$GENESIS_BIN" --selfhost-only --selfhost-artifact "$ART" "$@"
}

g pack --pkg "$WORK_DIR/package.toml" >/dev/null

web_a="$(g gcpm --caps "$WORK_DIR/caps.toml" build --pkg "$WORK_DIR/package.toml" --target web --out-dir "$WORK_DIR/.genesis/build-targets" | tr -d '\n')"
web_b="$(g gcpm --caps "$WORK_DIR/caps.toml" build --pkg "$WORK_DIR/package.toml" --target web --out-dir "$WORK_DIR/.genesis/build-targets" | tr -d '\n')"
desktop_h="$(g gcpm --caps "$WORK_DIR/caps.toml" build --pkg "$WORK_DIR/package.toml" --target desktop --out-dir "$WORK_DIR/.genesis/build-targets" | tr -d '\n')"
service_h="$(g gcpm --caps "$WORK_DIR/caps.toml" build --pkg "$WORK_DIR/package.toml" --target service --out-dir "$WORK_DIR/.genesis/build-targets" | tr -d '\n')"

if [[ "$web_a" != "$web_b" ]]; then
  echo "agent-deploy-bundle-workflow: reproducibility mismatch for web target: a=$web_a b=$web_b" >&2
  exit 1
fi

manifest_count="$(find "$WORK_DIR/.genesis/build-targets" -name 'build_manifest.gc' | wc -l | tr -d ' ')"
provenance_count="$(find "$WORK_DIR/.genesis/build-targets" -name 'provenance.gc' | wc -l | tr -d ' ')"
if [[ "$manifest_count" -lt 3 || "$provenance_count" -lt 3 ]]; then
  echo "agent-deploy-bundle-workflow: expected build manifest/provenance artifacts for web+desktop+service, got manifests=$manifest_count provenance=$provenance_count" >&2
  exit 1
fi

replay_token="$(printf '%s|%s|%s' "$web_a" "$desktop_h" "$service_h")"
echo "agent-deploy-bundle-workflow: ok replay=$replay_token"
