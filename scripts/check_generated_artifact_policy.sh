#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

ALLOWLIST="policies/generated_artifact_allowlist.txt"
[[ -f "$ALLOWLIST" ]] || {
  echo "generated-artifact-policy: missing allowlist: $ALLOWLIST" >&2
  exit 1
}

for pattern in '.genesis/' '**/.genesis/'; do
  if ! grep -Fxq "$pattern" .gitignore; then
    echo "generated-artifact-policy: .gitignore must keep $pattern ignored" >&2
    exit 1
  fi
done

tracked_tmp="$(mktemp)"
allow_tmp="$(mktemp)"
trap 'rm -f "$tracked_tmp" "$allow_tmp"' EXIT

git ls-files '.genesis/perf/*.json' '.genesis/perf/*.jsonl' > "$tracked_tmp"
sed -E 's/[[:space:]]*#.*$//; s/^[[:space:]]+//; s/[[:space:]]+$//' "$ALLOWLIST" \
  | awk 'NF > 0 { print }' \
  | sort -u > "$allow_tmp"

violations=0
while IFS= read -r path; do
  [[ -n "$path" ]] || continue
  if ! grep -Fxq "$path" "$allow_tmp"; then
    echo "generated-artifact-policy: tracked generated perf artifact is not allowlisted: $path" >&2
    violations=1
  fi
done < "$tracked_tmp"

if [[ "$violations" -ne 0 ]]; then
  echo "generated-artifact-policy: generated perf reports belong in CI artifacts or local .genesis/, not source control" >&2
  echo "generated-artifact-policy: add a narrowly-scoped exception to $ALLOWLIST only for intentional release evidence" >&2
  exit 1
fi

echo "generated-artifact-policy: ok"
