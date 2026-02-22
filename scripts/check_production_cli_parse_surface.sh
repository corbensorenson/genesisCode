#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/lib/cargo_target_dir.sh"
source "$ROOT/scripts/lib/profile_gate_timing.sh"
genesis_configure_cargo_target_dir \
  "$ROOT" \
  "check-production-cli-parse-surface" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_PRODUCTION_CLI_PARSE_SURFACE_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"
REPORT_PATH="${GENESIS_PRODUCTION_CLI_PARSE_SURFACE_REPORT:-.genesis/perf/production_cli_parse_surface_report.json}"
HISTORY_PATH="${GENESIS_PRODUCTION_CLI_PARSE_SURFACE_HISTORY:-.genesis/perf/production_cli_parse_surface_history.jsonl}"
BUDGET_MS="${GENESIS_PRODUCTION_CLI_PARSE_SURFACE_BUDGET_MS:-300000}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

CAPS="$TMP/caps.toml"
cat >"$CAPS" <<'TOML'
allow = ["core/refs::list"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
TOML

assert_rejects_rust() {
  local bin="$1"
  local err="$TMP/${bin}.err"
  set +e
  cargo run --release -q -p "$2" --bin "$bin" -- \
    --coreform-frontend rust \
    refs \
    --caps "$CAPS" \
    list >"$TMP/${bin}.out" 2>"$err"
  local code=$?
  set -e
  if [[ $code -eq 0 ]]; then
    echo "parse-surface: expected $bin to reject --coreform-frontend rust"
    cat "$TMP/${bin}.out"
    cat "$err"
    exit 1
  fi
  if [[ $code -ne 2 ]]; then
    echo "parse-surface: expected exit code 2 from $bin, got $code"
    cat "$err"
    exit 1
  fi
  grep -Fq "invalid value 'rust' for '--coreform-frontend <COREFORM_FRONTEND>'" "$err" || {
    echo "parse-surface: $bin stderr missing parse rejection detail"
    cat "$err"
    exit 1
  }
  grep -Fq 'expected `selfhost`' "$err" || {
    echo "parse-surface: $bin stderr missing expected selfhost hint"
    cat "$err"
    exit 1
  }
}

assert_accepts_rust_parity() {
  local bin="$1"
  cargo run --release -q -p "$2" --bin "$bin" -- \
    --coreform-frontend rust \
    refs \
    --caps "$CAPS" \
    list >/dev/null
}

assert_rejects_rust genesis gc_cli
assert_rejects_rust genesis_wasi gc_wasi_cli
assert_accepts_rust_parity genesis_parity gc_cli
assert_accepts_rust_parity genesis_wasi_parity gc_wasi_cli

genesis_profile_gate_emit_runtime_report \
  "production-cli-parse-surface" \
  "genesis/production-cli-parse-surface-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS"

echo "parse-surface: ok"
