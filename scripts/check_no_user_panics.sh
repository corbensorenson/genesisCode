#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-no-user-panics" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_NO_USER_PANICS_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"
REPORT_PATH="${GENESIS_NO_USER_PANICS_REPORT:-.genesis/perf/no_user_panics_report.json}"
HISTORY_PATH="${GENESIS_NO_USER_PANICS_HISTORY:-.genesis/perf/no_user_panics_history.jsonl}"
BUDGET_MS="${GENESIS_NO_USER_PANICS_BUDGET_MS:-900000}"

POLICY_FILE="${GENESIS_PANIC_GUARD_POLICY:-policies/panic_guard.toml}"
if [[ ! -f "$POLICY_FILE" ]]; then
  echo "panic-guard: missing policy file: $POLICY_FILE" >&2
  exit 1
fi

parse_array_tokens() {
  local key="$1"
  local line
  line="$(awk -v k="$key" '$0 ~ ("^" k "[[:space:]]*=") {print; exit}' "$POLICY_FILE")"
  if [[ -z "$line" ]]; then
    return 0
  fi
  printf "%s\n" "$line" | grep -oE '"[^"]+"' | tr -d '"'
}

EXCLUDE_PACKAGES=()
while IFS= read -r token; do
  [[ -n "$token" ]] && EXCLUDE_PACKAGES+=("$token")
done < <(parse_array_tokens exclude_packages || true)

LIB_EXEMPT_PACKAGES=()
while IFS= read -r token; do
  [[ -n "$token" ]] && LIB_EXEMPT_PACKAGES+=("$token")
done < <(parse_array_tokens lib_exempt_packages || true)

BIN_EXEMPT_TARGETS=()
while IFS= read -r token; do
  [[ -n "$token" ]] && BIN_EXEMPT_TARGETS+=("$token")
done < <(parse_array_tokens bin_exempt_targets || true)

csv_join() {
  local IFS=","
  printf '%s' "$*"
}

EXCLUDE_PACKAGES_CSV="$(csv_join "${EXCLUDE_PACKAGES[@]:-}")"
LIB_EXEMPT_PACKAGES_CSV="$(csv_join "${LIB_EXEMPT_PACKAGES[@]:-}")"
BIN_EXEMPT_TARGETS_CSV="$(csv_join "${BIN_EXEMPT_TARGETS[@]:-}")"

PLAN_TMP="$(mktemp)"
cleanup() {
  rm -f "$PLAN_TMP"
}
trap cleanup EXIT

EXCLUDE_PACKAGES_CSV="$EXCLUDE_PACKAGES_CSV" \
LIB_EXEMPT_PACKAGES_CSV="$LIB_EXEMPT_PACKAGES_CSV" \
BIN_EXEMPT_TARGETS_CSV="$BIN_EXEMPT_TARGETS_CSV" \
python3 >"$PLAN_TMP" <<'PY'
import json
import os
import subprocess


def parse_csv(name: str) -> set[str]:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return set()
    return {v.strip() for v in raw.split(",") if v.strip()}


exclude_packages = parse_csv("EXCLUDE_PACKAGES_CSV")
lib_exempt_packages = parse_csv("LIB_EXEMPT_PACKAGES_CSV")
bin_exempt_targets = parse_csv("BIN_EXEMPT_TARGETS_CSV")

meta = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"], text=True
    )
)

packages_by_id = {p["id"]: p for p in meta["packages"]}
workspace_packages = {
    packages_by_id[pkg_id]["name"] for pkg_id in meta["workspace_members"]
}

unknown_pkg_refs = sorted((exclude_packages | lib_exempt_packages) - workspace_packages)
if unknown_pkg_refs:
    raise SystemExit(
        "panic-guard: policy references unknown workspace packages: "
        + ", ".join(unknown_pkg_refs)
    )

all_bin_targets: dict[str, tuple[str, str]] = {}
package_targets: dict[str, list[dict]] = {}
for p in meta["packages"]:
    if p["name"] not in workspace_packages:
        continue
    package_targets[p["name"]] = p["targets"]
    for t in p["targets"]:
        if "bin" in t["kind"]:
            all_bin_targets[t["name"]] = (p["name"], t["name"])

unknown_bin_refs = sorted(bin_exempt_targets - set(all_bin_targets))
if unknown_bin_refs:
    raise SystemExit(
        "panic-guard: policy references unknown bin targets: " + ", ".join(unknown_bin_refs)
    )

production_packages = sorted(workspace_packages - exclude_packages)
lib_target_kinds = {"lib", "rlib", "cdylib", "staticlib", "dylib", "proc-macro"}

lib_packages = []
bin_targets: list[tuple[str, str]] = []

for pkg in production_packages:
    targets = package_targets[pkg]
    has_lib = any(lib_target_kinds.intersection(t["kind"]) for t in targets)
    if has_lib and pkg not in lib_exempt_packages:
        lib_packages.append(pkg)
    for t in targets:
        if "bin" not in t["kind"]:
            continue
        if t["name"] in bin_exempt_targets:
            continue
        bin_targets.append((pkg, t["name"]))

covered_packages = set(lib_packages) | {pkg for (pkg, _) in bin_targets}
uncovered_packages = sorted(set(production_packages) - covered_packages)

for pkg in production_packages:
    print(f"PRODUCTION\t{pkg}")
for pkg in sorted(exclude_packages):
    print(f"EXCLUDE\t{pkg}")
for pkg in sorted(lib_exempt_packages):
    print(f"LIB_EXEMPT\t{pkg}")
for target in sorted(bin_exempt_targets):
    print(f"BIN_EXEMPT\t{target}")
for pkg in sorted(lib_packages):
    print(f"LIB\t{pkg}")
for pkg, target in sorted(bin_targets):
    print(f"BIN\t{pkg}\t{target}")
for pkg in uncovered_packages:
    print(f"UNCOVERED\t{pkg}")
PY

PRODUCTION_PACKAGES=()
EXCLUDED_PACKAGES=()
LIB_EXEMPT_PACKAGES=()
BIN_EXEMPT_TARGETS=()
LIB_PACKAGES=()
BIN_TARGETS=()
UNCOVERED_PACKAGES=()

while IFS=$'\t' read -r kind a b; do
  case "$kind" in
    PRODUCTION) PRODUCTION_PACKAGES+=("$a") ;;
    EXCLUDE) EXCLUDED_PACKAGES+=("$a") ;;
    LIB_EXEMPT) LIB_EXEMPT_PACKAGES+=("$a") ;;
    BIN_EXEMPT) BIN_EXEMPT_TARGETS+=("$a") ;;
    LIB) LIB_PACKAGES+=("$a") ;;
    BIN) BIN_TARGETS+=("${a}:${b}") ;;
    UNCOVERED) UNCOVERED_PACKAGES+=("$a") ;;
    *) ;;
  esac
done < "$PLAN_TMP"

echo "panic-guard: policy=$POLICY_FILE"
echo "panic-guard: production packages=${#PRODUCTION_PACKAGES[@]} excluded=${#EXCLUDED_PACKAGES[@]} lib_exempt=${#LIB_EXEMPT_PACKAGES[@]} bin_exempt=${#BIN_EXEMPT_TARGETS[@]}"

if [[ ${#UNCOVERED_PACKAGES[@]} -gt 0 ]]; then
  echo "panic-guard: uncovered production packages (no linted lib/bin target and no explicit exemption):" >&2
  for pkg in "${UNCOVERED_PACKAGES[@]}"; do
    echo "  - $pkg" >&2
  done
  exit 1
fi

LINT_FLAGS=(
  -D clippy::unwrap_used
  -D clippy::expect_used
  -D clippy::panic
)

if [[ ${#LIB_PACKAGES[@]} -gt 0 ]]; then
  echo "panic-guard: checking ${#LIB_PACKAGES[@]} production libraries for unwrap/expect/panic usage"
  LIB_CMD=(cargo clippy)
  for pkg in "${LIB_PACKAGES[@]}"; do
    LIB_CMD+=(-p "$pkg")
  done
  LIB_CMD+=(--lib -- "${LINT_FLAGS[@]}")
  "${LIB_CMD[@]}"
fi

if [[ ${#BIN_TARGETS[@]} -gt 0 ]]; then
  echo "panic-guard: checking ${#BIN_TARGETS[@]} production binaries for unwrap/expect/panic usage"
  for spec in "${BIN_TARGETS[@]}"; do
    pkg="${spec%%:*}"
    bin="${spec#*:}"
    cargo clippy -p "$pkg" --bin "$bin" -- "${LINT_FLAGS[@]}"
  done
fi

echo "panic-guard: checking production source paths for unreachable! macros"
if command -v rg >/dev/null 2>&1; then
  UNREACHABLE_HITS="$(
    rg -n "unreachable!\\(" crates --glob '!**/tests/**' --glob '!**/benches/**' || true
  )"
else
  UNREACHABLE_HITS="$(
    grep -R -n --exclude-dir=tests --exclude-dir=benches "unreachable!(" crates 2>/dev/null || true
  )"
fi
if [[ -n "$UNREACHABLE_HITS" ]]; then
  echo "panic-guard: unreachable! is not allowed in production user paths" >&2
  echo "$UNREACHABLE_HITS" >&2
  exit 1
fi

genesis_profile_gate_emit_runtime_report \
  "no-user-panics" \
  "genesis/no-user-panics-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS"

echo "panic-guard: ok"
