#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-hot-path-budgets" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_HOT_PATH_BUDGETS_CARGO_TARGET_DIR"

source "$ROOT_DIR/scripts/lib/measure.sh"
source "$ROOT_DIR/scripts/lib/gcpm_caps_fixture.sh"
source "$ROOT_DIR/scripts/lib/perf_disk_mode.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

START_MS="$(genesis_profile_gate_now_ms)"

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
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
DISK_STRICT_MODE="$(genesis_resolve_perf_disk_strict_mode)"
DISK_MIN_FREE_KB="${GENESIS_HOT_PATH_MIN_FREE_KB:-3145728}"
RUNTIME_REPORT="${GENESIS_HOT_PATH_RUNTIME_REPORT_OUT:-.genesis/perf/hot_path_runtime_report.json}"
RUNTIME_HISTORY="${GENESIS_HOT_PATH_RUNTIME_HISTORY_OUT:-.genesis/perf/hot_path_runtime_history.jsonl}"
RUNTIME_BASELINE_HISTORY="${GENESIS_HOT_PATH_RUNTIME_BASELINE_HISTORY_OUT:-policies/perf/hot_path_runtime_seed_history.jsonl}"
RUNTIME_BUDGET_MS="${GENESIS_HOT_PATH_RUNTIME_BUDGET_MS:-1200000}"
RUNTIME_MIN_HISTORY="${GENESIS_HOT_PATH_RUNTIME_MIN_HISTORY:-5}"
RUNTIME_REQUIRE_MIN_HISTORY="${GENESIS_HOT_PATH_RUNTIME_REQUIRE_MIN_HISTORY:-1}"

if [[ ! "$RUNTIME_MIN_HISTORY" =~ ^[0-9]+$ || "$RUNTIME_MIN_HISTORY" -le 0 ]]; then
  echo "hot-path-budgets: GENESIS_HOT_PATH_RUNTIME_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$RUNTIME_REQUIRE_MIN_HISTORY" != "0" && "$RUNTIME_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "hot-path-budgets: GENESIS_HOT_PATH_RUNTIME_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi

fail() {
  echo "hot-path-budgets: $*" >&2
  exit 1
}

profile_target_dir() {
  case "$1" in
    release) echo "release" ;;
    dev|test) echo "debug" ;;
    *) echo "$1" ;;
  esac
}

TARGET_PROFILE_DIR="$(profile_target_dir "$CARGO_PROFILE")"
GENESIS_BIN="$CARGO_TARGET_DIR/$TARGET_PROFILE_DIR/genesis"

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "hot-path-budgets" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --strict "$DISK_STRICT_MODE"

echo "hot-path-budgets: preparing genesis binary"
cargo build -p gc_cli --profile "$CARGO_PROFILE" >/dev/null
cargo test -p gc_effects --test sync_registry --no-run --quiet --profile "$CARGO_PROFILE" >/dev/null
SYNC_TEST_BIN="$(
  find "$CARGO_TARGET_DIR/$TARGET_PROFILE_DIR/deps" -maxdepth 1 -type f -name 'sync_registry-*' -perm -u+x \
    | sort \
    | tail -n 1
)"
[[ -x "$GENESIS_BIN" ]] || fail "unable to locate genesis binary at $GENESIS_BIN"
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

write_gcpm_low_caps_fixture "$TMP_DIR/gcpm_caps.toml"

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

echo "hot-path-budgets: measuring parser/canonicalizer path (fmt)"
genesis_measure_best_of_ms \
  fmt_canon_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" \
  fmt "$TMP_DIR/basic.gc" --engine selfhost
FMT_CANON_MS="$MEASURE_LAST_MS"

echo "hot-path-budgets: measuring evaluator path (pure eval)"
genesis_measure_best_of_ms \
  eval_pure_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" \
  eval "$TMP_DIR/basic.gc" --engine selfhost
EVAL_PURE_MS="$MEASURE_LAST_MS"

echo "hot-path-budgets: measuring effect runner path (run sys/time::now)"
genesis_measure_best_of_ms \
  effect_run_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$GENESIS_BIN" --selfhost-artifact "$TOOLCHAIN" \
  run "$TMP_DIR/time_effect.gc" --caps "$TMP_DIR/time_caps.toml" --log "$TMP_DIR/time.gclog"
EFFECT_RUN_MS="$MEASURE_LAST_MS"

echo "hot-path-budgets: measuring sync throughput path"
genesis_measure_best_of_ms \
  sync_pull_ms \
  "$MEASURE_WARMUPS" \
  "$MEASURE_REPEATS" \
  "$SYNC_TEST_BIN" \
  --exact sync_push_then_pull_transfers_full_closure_and_updates_refs --quiet
SYNC_PULL_MS="$MEASURE_LAST_MS"

echo "hot-path-budgets: measuring gcpm lock/install/update flows"
run_gcpm_tmp new --workspace "perf-hot-paths" --policy "policy:default-v0.1" --registry-default "gen://registry" >/dev/null
genesis_measure_best_of_ms gcpm_lock_ms "$MEASURE_WARMUPS" "$MEASURE_REPEATS" run_gcpm_tmp lock --strict
GCPM_LOCK_MS="$MEASURE_LAST_MS"
genesis_measure_best_of_ms gcpm_install_ms "$MEASURE_WARMUPS" "$MEASURE_REPEATS" run_gcpm_tmp install --frozen
GCPM_INSTALL_MS="$MEASURE_LAST_MS"
genesis_measure_best_of_ms gcpm_update_ms "$MEASURE_WARMUPS" "$MEASURE_REPEATS" run_gcpm_tmp update
GCPM_UPDATE_MS="$MEASURE_LAST_MS"

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
  "build_profile": "$CARGO_PROFILE",
  "build_mode": "release-equivalent",
  "build_target_dir": "$TARGET_PROFILE_DIR",
  "disk_strict_mode": "$DISK_STRICT_MODE",
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

genesis_profile_gate_emit_runtime_report \
  "hot-path-budgets" \
  "genesis/hot-path-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$RUNTIME_HISTORY" \
  "$START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "$RUNTIME_MIN_HISTORY" \
  "{\"metrics_report\":\"$ARTIFACT_JSON\",\"build_profile\":\"$CARGO_PROFILE\"}" \
  "" \
  "$RUNTIME_BASELINE_HISTORY" \
  "$RUNTIME_REQUIRE_MIN_HISTORY"

echo "hot-path-budgets: ok"
