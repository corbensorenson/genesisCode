#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

WORKSPACE_FILE="genesis.workspace.toml"
LOCK_FILE="genesis.lock"

for path in "$WORKSPACE_FILE" "$LOCK_FILE"; do
  [[ -f "$path" ]] || {
    echo "root-lock-policy: missing required root file: $path" >&2
    exit 1
  }
done

if git check-ignore -q "$LOCK_FILE"; then
  echo "root-lock-policy: $LOCK_FILE is ignored; root package lock must be committed evidence" >&2
  exit 1
fi

toml_required_scalar() {
  local file="$1"
  local section="$2"
  local key="$3"
  local scalar_type="$4"
  awk \
    -v file_label="$file" \
    -v target_section="$section" \
    -v target_key="$key" \
    -v scalar_type="$scalar_type" '
function trim(value) {
  sub(/^[[:space:]]+/, "", value)
  sub(/[[:space:]]+$/, "", value)
  return value
}
BEGIN {
  current_section = ""
  match_count = 0
  invalid = 0
  parsed = ""
}
{
  line = $0
  sub(/\r$/, "", line)
  stripped = trim(line)
  if (stripped == "" || substr(stripped, 1, 1) == "#") {
    next
  }
  if (stripped ~ /^\[\[[^]]+\]\]$/) {
    current_section = trim(substr(stripped, 3, length(stripped) - 4))
    next
  }
  if (stripped ~ /^\[[^]]+\]$/) {
    current_section = trim(substr(stripped, 2, length(stripped) - 2))
    next
  }
  equals = index(stripped, "=")
  if (equals == 0) {
    next
  }
  lhs = trim(substr(stripped, 1, equals - 1))
  if (current_section != target_section || lhs != target_key) {
    next
  }
  match_count++
  raw = trim(substr(stripped, equals + 1))
  if (scalar_type == "string") {
    if (raw !~ /^"[^"]*"$/) {
      invalid = 1
    } else {
      parsed = substr(raw, 2, length(raw) - 2)
    }
  } else if (scalar_type == "integer") {
    if (raw !~ /^[0-9]+$/) {
      invalid = 1
    } else {
      parsed = raw
    }
  } else {
    invalid = 1
  }
}
END {
  label = target_section == "" ? target_key : target_section "." target_key
  if (match_count == 0) {
    print "root-lock-policy: " file_label " missing required scalar " label > "/dev/stderr"
    exit 2
  }
  if (match_count != 1) {
    print "root-lock-policy: " file_label " contains duplicate scalar " label > "/dev/stderr"
    exit 2
  }
  if (invalid) {
    print "root-lock-policy: " file_label " has invalid " scalar_type " scalar " label > "/dev/stderr"
    exit 2
  }
  print parsed
}' "$file"
}

PARSER_FIXTURES="$(mktemp -d)"
cleanup() {
  rm -rf "$PARSER_FIXTURES"
}
trap cleanup EXIT

cat >"$PARSER_FIXTURES/duplicate.toml" <<'EOF'
workspace = "genesisCode"
workspace = "forged"
EOF
cat >"$PARSER_FIXTURES/missing.toml" <<'EOF'
policy = "policy:default-v0.1"
EOF
cat >"$PARSER_FIXTURES/wrong-type.toml" <<'EOF'
version = "2"
EOF

negative_controls=0
for fixture in duplicate missing; do
  if toml_required_scalar "$PARSER_FIXTURES/$fixture.toml" "" workspace string >/dev/null 2>&1; then
    echo "root-lock-policy: parser negative control unexpectedly passed: $fixture" >&2
    exit 1
  fi
  negative_controls=$((negative_controls + 1))
done
if toml_required_scalar "$PARSER_FIXTURES/wrong-type.toml" "" version integer >/dev/null 2>&1; then
  echo "root-lock-policy: parser negative control unexpectedly passed: wrong-type" >&2
  exit 1
fi
negative_controls=$((negative_controls + 1))

workspace_name="$(toml_required_scalar "$WORKSPACE_FILE" "" workspace string)"
workspace_policy="$(toml_required_scalar "$WORKSPACE_FILE" defaults policy string)"
lock_version="$(toml_required_scalar "$LOCK_FILE" "" version integer)"
lock_workspace="$(toml_required_scalar "$LOCK_FILE" "" workspace string)"
lock_policy="$(toml_required_scalar "$LOCK_FILE" "" policy string)"

errors=()
[[ "$workspace_name" == "genesisCode" ]] || errors+=("genesis.workspace.toml must set workspace = \"genesisCode\"")
[[ "$lock_workspace" == "$workspace_name" ]] || errors+=("genesis.lock workspace must match genesis.workspace.toml")
[[ "$lock_policy" == "$workspace_policy" ]] || errors+=("genesis.lock policy must match [defaults].policy")
[[ "$lock_version" == "2" ]] || errors+=("genesis.lock must use version = 2")
if [[ ${#errors[@]} -gt 0 ]]; then
  IFS='; '
  echo "root-lock-policy: ${errors[*]}" >&2
  exit 1
fi

echo "root-lock-policy: ok (parser=posix-awk negative_controls=$negative_controls)"
