#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

BUDGET_INCREMENTAL_WARM_MS="${GENESIS_BUDGET_INCREMENTAL_WARM_MS:-60000}"
BUDGET_CORE_SUITE_MS="${GENESIS_BUDGET_CORE_SUITE_MS:-300000}"
BUDGET_GCPM_LOCK_MS="${GENESIS_BUDGET_GCPM_LOCK_MS:-20000}"
BUDGET_GCPM_ENV_MS="${GENESIS_BUDGET_GCPM_ENV_MS:-15000}"

now_ns() {
  python3 - <<'PY'
import time
print(time.time_ns())
PY
}

measure_ms() {
  local label="$1"
  shift
  local start_ns end_ns elapsed_ms
  start_ns="$(now_ns)"
  "$@" >/dev/null
  end_ns="$(now_ns)"
  elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"
  echo "$label=$elapsed_ms"
}

fail() {
  echo "ai-iteration-slo: $*" >&2
  exit 1
}

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "ai-iteration-slo: preparing genesis binary"
cargo build -p gc_cli >/dev/null
GENESIS_BIN="$ROOT_DIR/target/debug/genesis"

for name in basic.gc caps.toml package.toml pure.gcpatch; do
  cp "tests/spec/pkg_basic/$name" "$TMP_DIR/$name"
done

TOOLCHAIN="$TMP_DIR/toolchain.gc"
"$GENESIS_BIN" selfhost-artifact --out "$TOOLCHAIN" >/dev/null

run_incremental_loop() {
  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    fmt "$TMP_DIR/basic.gc" --engine selfhost

  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    eval "$TMP_DIR/basic.gc" --engine selfhost

  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    typecheck --pkg "$TMP_DIR/package.toml"

  "$GENESIS_BIN" \
    --selfhost-artifact "$TOOLCHAIN" \
    test --pkg "$TMP_DIR/package.toml"
}

cat > "$TMP_DIR/gcpm_caps.toml" <<'EOF'
allow = ["core/pkg-low::init", "core/pkg-low::lock"]

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::lock"]
base_dir = "."
create_dirs = true
EOF

run_gcpm_tmp() {
  (
    cd "$TMP_DIR"
    "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" gcpm --caps "$TMP_DIR/gcpm_caps.toml" "$@"
  )
}

# One warm-up pass to amortize startup and artifact load effects.
run_incremental_loop >/dev/null

echo "ai-iteration-slo: measuring warm incremental loop"
INC_LINE="$(measure_ms incremental_warm_ms run_incremental_loop)"
INCREMENTAL_WARM_MS="${INC_LINE#*=}"

echo "ai-iteration-slo: measuring core suite wall-time"
CORE_LINE="$(measure_ms core_suite_ms cargo test -p gc_coreform -p gc_kernel -p gc_prelude -p gc_cli --test cli_smoke --quiet)"
CORE_SUITE_MS="${CORE_LINE#*=}"

echo "ai-iteration-slo: measuring gcpm lock/env iteration path"
run_gcpm_tmp new --workspace "slo" --policy "policy:default-v0.1" --registry-default "gen://registry" >/dev/null
LOCK_LINE="$(measure_ms gcpm_lock_ms run_gcpm_tmp lock --strict)"
GCPM_LOCK_MS="${LOCK_LINE#*=}"
ENV_LINE="$(measure_ms gcpm_env_ms run_gcpm_tmp env --profile dev)"
GCPM_ENV_MS="${ENV_LINE#*=}"

echo "ai-iteration-slo: metrics"
echo "  incremental_warm_ms=$INCREMENTAL_WARM_MS (budget=$BUDGET_INCREMENTAL_WARM_MS)"
echo "  core_suite_ms=$CORE_SUITE_MS (budget=$BUDGET_CORE_SUITE_MS)"
echo "  gcpm_lock_ms=$GCPM_LOCK_MS (budget=$BUDGET_GCPM_LOCK_MS)"
echo "  gcpm_env_ms=$GCPM_ENV_MS (budget=$BUDGET_GCPM_ENV_MS)"

[[ "$INCREMENTAL_WARM_MS" -le "$BUDGET_INCREMENTAL_WARM_MS" ]] || fail "warm incremental loop regression: $INCREMENTAL_WARM_MS > $BUDGET_INCREMENTAL_WARM_MS"
[[ "$CORE_SUITE_MS" -le "$BUDGET_CORE_SUITE_MS" ]] || fail "core suite regression: $CORE_SUITE_MS > $BUDGET_CORE_SUITE_MS"
[[ "$GCPM_LOCK_MS" -le "$BUDGET_GCPM_LOCK_MS" ]] || fail "gcpm lock regression: $GCPM_LOCK_MS > $BUDGET_GCPM_LOCK_MS"
[[ "$GCPM_ENV_MS" -le "$BUDGET_GCPM_ENV_MS" ]] || fail "gcpm env regression: $GCPM_ENV_MS > $BUDGET_GCPM_ENV_MS"

echo "ai-iteration-slo: ok"
