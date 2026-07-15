#!/usr/bin/env bash
set -euo pipefail

# Full fast suite for local use.
# Default local/CI iteration should use scripts/test_changed_fast.sh.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "test-fast-full" \
  root-host

bash scripts/check_disk_headroom.sh --path "$ROOT_DIR" --context "test-fast-full"

SECONDS=0
if cargo nextest --version >/dev/null 2>&1; then
  RUNNER="nextest"
else
  RUNNER="cargo"
fi

echo "[test-fast-full] selfhost artifact freshness"
./scripts/check_selfhost_artifact_fresh.sh

if [[ "$RUNNER" == "nextest" ]]; then
  echo "[test-fast-full] cargo nextest run (core libs)"
  cargo nextest run \
    -p gc_coreform \
    -p gc_kernel \
    -p gc_prelude \
    -p gc_obligations \
    -p gc_patches \
    --profile ci

  echo "[test-fast-full] cargo nextest run (selected CLI integration tests)"
  cargo nextest run -p gc_cli \
    --test cli_smoke \
    --test cli_selfhost_only \
    --test cli_apply_patch_determinism \
    --test cli_typecheck_apply_patch_engine \
    --profile ci
else
  echo "[test-fast-full] cargo test (core libs)"
  cargo test \
    -p gc_coreform \
    -p gc_kernel \
    -p gc_prelude \
    -p gc_obligations \
    -p gc_patches

  echo "[test-fast-full] cargo test (selected CLI integration tests, parallel)"
  CLI_TESTS=(
    cli_smoke
    cli_selfhost_only
    cli_apply_patch_determinism
    cli_typecheck_apply_patch_engine
  )
  CLI_TEST_LOG_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-fast-cli.XXXXXX")"
  trap 'rm -rf "$CLI_TEST_LOG_DIR"' EXIT
  cargo test -p gc_cli --no-run --message-format=json-render-diagnostics \
    --test cli_smoke \
    --test cli_selfhost_only \
    --test cli_apply_patch_determinism \
    --test cli_typecheck_apply_patch_engine \
    | python3 -c '
import json
import sys

wanted = {
    "cli_smoke",
    "cli_selfhost_only",
    "cli_apply_patch_determinism",
    "cli_typecheck_apply_patch_engine",
}
executables = {}
for line in sys.stdin:
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    target = event.get("target", {})
    name = target.get("name")
    if (
        event.get("reason") == "compiler-artifact"
        and name in wanted
        and "test" in target.get("kind", [])
        and event.get("executable")
    ):
        executables[name] = event["executable"]

missing = sorted(wanted - executables.keys())
if missing:
    raise SystemExit("missing CLI test executables: " + ", ".join(missing))
for name in sorted(wanted):
    print(f"{name}\t{executables[name]}")
' >"$CLI_TEST_LOG_DIR/executables.tsv"

  CLI_TEST_PIDS=()
  for test_name in "${CLI_TESTS[@]}"; do
    test_executable="$(awk -F '\t' -v name="$test_name" '$1 == name { print $2 }' "$CLI_TEST_LOG_DIR/executables.tsv")"
    if [[ -z "$test_executable" || ! -x "$test_executable" ]]; then
      echo "[test-fast-full] missing executable for $test_name" >&2
      exit 1
    fi
    (
      cd "$ROOT_DIR/crates/gc_cli"
      GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT="$ROOT_DIR/selfhost/toolchain.gc" \
        "$test_executable"
    ) >"$CLI_TEST_LOG_DIR/$test_name.log" 2>&1 &
    CLI_TEST_PIDS+=("$!")
  done

  CLI_TEST_STATUS=0
  for index in "${!CLI_TESTS[@]}"; do
    if ! wait "${CLI_TEST_PIDS[$index]}"; then
      CLI_TEST_STATUS=1
    fi
    cat "$CLI_TEST_LOG_DIR/${CLI_TESTS[$index]}.log"
  done
  rm -rf "$CLI_TEST_LOG_DIR"
  trap - EXIT
  if (( CLI_TEST_STATUS != 0 )); then
    echo "[test-fast-full] selected CLI integration tests failed" >&2
    exit "$CLI_TEST_STATUS"
  fi
fi

echo "[test-fast-full] ok in ${SECONDS}s (runner=${RUNNER})"
