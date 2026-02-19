#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

# Conservative defaults for shared CI runners.
BUDGET_FMT_CANON_MS="${GENESIS_BUDGET_FMT_CANON_MS:-15000}"
BUDGET_EVAL_PURE_MS="${GENESIS_BUDGET_EVAL_PURE_MS:-15000}"
BUDGET_EFFECT_RUN_MS="${GENESIS_BUDGET_EFFECT_RUN_MS:-20000}"
BUDGET_SYNC_PULL_MS="${GENESIS_BUDGET_SYNC_PULL_MS:-30000}"
BUDGET_GCPM_LOCK_MS="${GENESIS_BUDGET_GCPM_LOCK_MS:-20000}"
BUDGET_GCPM_INSTALL_MS="${GENESIS_BUDGET_GCPM_INSTALL_MS:-15000}"
BUDGET_GCPM_UPDATE_MS="${GENESIS_BUDGET_GCPM_UPDATE_MS:-15000}"
MEASURE_WARMUPS="${GENESIS_BUDGET_WARMUPS:-1}"
MEASURE_REPEATS="${GENESIS_BUDGET_REPEATS:-3}"

now_ns() {
  python3 - <<'PY'
import time
print(time.time_ns())
PY
}

measure_ms() {
  local label="$1"
  shift
  local i start_ns end_ns elapsed_ms best_ms

  for ((i = 0; i < MEASURE_WARMUPS; i++)); do
    "$@" >/dev/null
  done

  best_ms=""
  for ((i = 0; i < MEASURE_REPEATS; i++)); do
    start_ns="$(now_ns)"
    "$@" >/dev/null
    end_ns="$(now_ns)"
    elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"
    if [[ -z "$best_ms" || "$elapsed_ms" -lt "$best_ms" ]]; then
      best_ms="$elapsed_ms"
    fi
  done

  echo "$label=$best_ms"
}

fail() {
  echo "hot-path-budgets: $*" >&2
  exit 1
}

echo "hot-path-budgets: preparing genesis binary"
cargo build -p gc_cli >/dev/null
cargo test -p gc_effects --test sync_registry --no-run --quiet >/dev/null
GENESIS_BIN="$ROOT_DIR/target/debug/genesis"
SYNC_TEST_BIN="$(
  find "$ROOT_DIR/target/debug/deps" -maxdepth 1 -type f -name 'sync_registry-*' -perm -u+x \
    | sort \
    | tail -n 1
)"
[[ -x "${SYNC_TEST_BIN:-}" ]] || fail "unable to locate compiled sync_registry test binary"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$ROOT_DIR/.genesis/perf"
ARTIFACT_JSON="$ROOT_DIR/.genesis/perf/hot_path_metrics.json"

cp tests/spec/pkg_basic/basic.gc "$TMP_DIR/basic.gc"
cp tests/spec/pkg_basic/package.toml "$TMP_DIR/package.toml"
cp tests/spec/pkg_basic/pure.gcpatch "$TMP_DIR/pure.gcpatch"

cat > "$TMP_DIR/time_effect.gc" <<'EOF'
(def prog
  (core/effect::perform
    'sys/time::now
    nil
    (fn (r) (core/effect::pure r))))
prog
EOF

cat > "$TMP_DIR/time_caps.toml" <<'EOF'
allow = ["sys/time::now"]
EOF

cat > "$TMP_DIR/gcpm_caps.toml" <<'EOF'
allow = [
  "core/pkg::new",
  "core/pkg::lock",
  "core/pkg::install",
  "core/pkg::update"
]

[op."core/pkg::new"]
base_dir = "."
create_dirs = true

[op."core/pkg::lock"]
base_dir = "."
create_dirs = true

[op."core/pkg::install"]
base_dir = "."
create_dirs = true

[op."core/pkg::update"]
base_dir = "."
create_dirs = true
EOF

echo "hot-path-budgets: building selfhost artifact"
TOOLCHAIN="$TMP_DIR/toolchain.gc"
"$GENESIS_BIN" selfhost-artifact --out "$TOOLCHAIN" >/dev/null

run_gcpm_tmp() {
  (
    cd "$TMP_DIR"
    "$GENESIS_BIN" \
      --selfhost-artifact "$TOOLCHAIN" \
      gcpm --caps "$TMP_DIR/gcpm_caps.toml" "$@"
  )
}

echo "hot-path-budgets: measuring parser/canonicalizer path (fmt --check)"
FMT_LINE="$(
  measure_ms fmt_canon_ms \
    "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" \
    fmt "$TMP_DIR/basic.gc" --engine selfhost --check
)"
FMT_CANON_MS="${FMT_LINE#*=}"

echo "hot-path-budgets: measuring evaluator path (pure eval)"
EVAL_LINE="$(
  measure_ms eval_pure_ms \
    "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" \
    eval "$TMP_DIR/basic.gc" --engine selfhost
)"
EVAL_PURE_MS="${EVAL_LINE#*=}"

echo "hot-path-budgets: measuring effect runner path (run sys/time::now)"
EFFECT_LINE="$(
  measure_ms effect_run_ms \
    "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" \
    run "$TMP_DIR/time_effect.gc" --caps "$TMP_DIR/time_caps.toml" --log "$TMP_DIR/time.gclog"
)"
EFFECT_RUN_MS="${EFFECT_LINE#*=}"

echo "hot-path-budgets: measuring sync throughput path"
SYNC_LINE="$(
  measure_ms sync_pull_ms \
    "$SYNC_TEST_BIN" \
    --exact sync_push_then_pull_transfers_full_closure_and_updates_refs --quiet
)"
SYNC_PULL_MS="${SYNC_LINE#*=}"

echo "hot-path-budgets: measuring gcpm lock/install/update flows"
run_gcpm_tmp new --workspace "perf-hot-paths" --policy "policy:default-v0.1" --registry-default "gen://registry" >/dev/null
LOCK_LINE="$(measure_ms gcpm_lock_ms run_gcpm_tmp lock --strict)"
GCPM_LOCK_MS="${LOCK_LINE#*=}"
INSTALL_LINE="$(measure_ms gcpm_install_ms run_gcpm_tmp install --frozen)"
GCPM_INSTALL_MS="${INSTALL_LINE#*=}"
UPDATE_LINE="$(measure_ms gcpm_update_ms run_gcpm_tmp update)"
GCPM_UPDATE_MS="${UPDATE_LINE#*=}"

echo "hot-path-budgets: metrics"
echo "  fmt_canon_ms=$FMT_CANON_MS (budget=$BUDGET_FMT_CANON_MS)"
echo "  eval_pure_ms=$EVAL_PURE_MS (budget=$BUDGET_EVAL_PURE_MS)"
echo "  effect_run_ms=$EFFECT_RUN_MS (budget=$BUDGET_EFFECT_RUN_MS)"
echo "  sync_pull_ms=$SYNC_PULL_MS (budget=$BUDGET_SYNC_PULL_MS)"
echo "  gcpm_lock_ms=$GCPM_LOCK_MS (budget=$BUDGET_GCPM_LOCK_MS)"
echo "  gcpm_install_ms=$GCPM_INSTALL_MS (budget=$BUDGET_GCPM_INSTALL_MS)"
echo "  gcpm_update_ms=$GCPM_UPDATE_MS (budget=$BUDGET_GCPM_UPDATE_MS)"
echo "  warmups=$MEASURE_WARMUPS"
echo "  repeats=$MEASURE_REPEATS"

cat > "$ARTIFACT_JSON" <<EOF
{
  "kind": "genesis/hot-path-budgets-v0.1",
  "fmt_canon_ms": $FMT_CANON_MS,
  "eval_pure_ms": $EVAL_PURE_MS,
  "effect_run_ms": $EFFECT_RUN_MS,
  "sync_pull_ms": $SYNC_PULL_MS,
  "gcpm_lock_ms": $GCPM_LOCK_MS,
  "gcpm_install_ms": $GCPM_INSTALL_MS,
  "gcpm_update_ms": $GCPM_UPDATE_MS,
  "measure_warmups": $MEASURE_WARMUPS,
  "measure_repeats": $MEASURE_REPEATS,
  "budgets": {
    "fmt_canon_ms": $BUDGET_FMT_CANON_MS,
    "eval_pure_ms": $BUDGET_EVAL_PURE_MS,
    "effect_run_ms": $BUDGET_EFFECT_RUN_MS,
    "sync_pull_ms": $BUDGET_SYNC_PULL_MS,
    "gcpm_lock_ms": $BUDGET_GCPM_LOCK_MS,
    "gcpm_install_ms": $BUDGET_GCPM_INSTALL_MS,
    "gcpm_update_ms": $BUDGET_GCPM_UPDATE_MS
  }
}
EOF

[[ "$FMT_CANON_MS" -le "$BUDGET_FMT_CANON_MS" ]] || fail "fmt regression: $FMT_CANON_MS > $BUDGET_FMT_CANON_MS"
[[ "$EVAL_PURE_MS" -le "$BUDGET_EVAL_PURE_MS" ]] || fail "eval regression: $EVAL_PURE_MS > $BUDGET_EVAL_PURE_MS"
[[ "$EFFECT_RUN_MS" -le "$BUDGET_EFFECT_RUN_MS" ]] || fail "effect-run regression: $EFFECT_RUN_MS > $BUDGET_EFFECT_RUN_MS"
[[ "$SYNC_PULL_MS" -le "$BUDGET_SYNC_PULL_MS" ]] || fail "sync regression: $SYNC_PULL_MS > $BUDGET_SYNC_PULL_MS"
[[ "$GCPM_LOCK_MS" -le "$BUDGET_GCPM_LOCK_MS" ]] || fail "gcpm lock regression: $GCPM_LOCK_MS > $BUDGET_GCPM_LOCK_MS"
[[ "$GCPM_INSTALL_MS" -le "$BUDGET_GCPM_INSTALL_MS" ]] || fail "gcpm install regression: $GCPM_INSTALL_MS > $BUDGET_GCPM_INSTALL_MS"
[[ "$GCPM_UPDATE_MS" -le "$BUDGET_GCPM_UPDATE_MS" ]] || fail "gcpm update regression: $GCPM_UPDATE_MS > $BUDGET_GCPM_UPDATE_MS"

echo "hot-path-budgets: ok"
