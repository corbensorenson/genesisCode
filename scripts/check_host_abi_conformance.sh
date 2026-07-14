#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "host-abi-conformance" \
  root-host

RUNNER_FILES=(
  "crates/gc_effects/src/runner_capability_dispatch.rs"
  "crates/gc_effects/src/runner_browser_host.rs"
  "crates/gc_effects/src/runner_xr_host.rs"
  "crates/gc_effects/src/runner_task.rs"
  "crates/gc_effects/src/runner_cap_pkg_low.rs"
  "crates/gc_effects/src/runner_cap_vcs_low.rs"
  "crates/gc_effects/src/runner_cap_gc_gpk_low.rs"
)
DOC_FILE="docs/spec/HOST_ABI.md"
FFI_SIGNED_PROFILE="docs/policies/ffi_signed_runtime_caps_v0.1.toml"

for f in "${RUNNER_FILES[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "host-abi-conformance: missing dispatch file: $f"
    exit 1
  fi
done
if [[ ! -f "$DOC_FILE" ]]; then
  echo "host-abi-conformance: missing doc file: $DOC_FILE"
  exit 1
fi
if [[ ! -f "$FFI_SIGNED_PROFILE" ]]; then
  echo "host-abi-conformance: missing signed ffi profile template: $FFI_SIGNED_PROFILE"
  exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

IMPL_SORTED="$TMP_DIR/impl_sorted.txt"
DOC_RAW="$TMP_DIR/doc_raw.txt"
DOC_SORTED="$TMP_DIR/doc_sorted.txt"

extract_impl_ops() {
  if command -v rg >/dev/null 2>&1; then
    rg -o --no-filename --pcre2 '"([[:alnum:]_/-]+::[[:alnum:]_/:.-]+)"' "${RUNNER_FILES[@]}"
  else
    grep -Eho '"[[:alnum:]_/-]+::[[:alnum:]_/:.-]+"' "${RUNNER_FILES[@]}"
  fi
}

extract_impl_ops \
  | tr -d '"' \
  | sort -u >"$IMPL_SORTED"

awk '
  /HOST_ABI_OPS_BEGIN/ { in_doc = 1; next; }
  /HOST_ABI_OPS_END/ { in_doc = 0; next; }
  in_doc {
    if (match($0, /`[^`]+::[^`]+`/)) {
      line = substr($0, RSTART + 1, RLENGTH - 2);
      print line;
    }
  }
' "$DOC_FILE" >"$DOC_RAW"

if [[ ! -s "$DOC_RAW" ]]; then
  echo "host-abi-conformance: no documented ops found between HOST_ABI_OPS markers"
  exit 1
fi
if [[ ! -s "$IMPL_SORTED" ]]; then
  echo "host-abi-conformance: no implementation ops detected in capability dispatch"
  exit 1
fi

sort -u "$DOC_RAW" >"$DOC_SORTED"

if ! cmp -s "$DOC_RAW" "$DOC_SORTED"; then
  echo "host-abi-conformance: documented host ABI ops must be globally sorted and unique"
  echo "expected sorted unique list:"
  cat "$DOC_SORTED"
  echo "actual list:"
  cat "$DOC_RAW"
  exit 1
fi

if ! diff -u "$DOC_SORTED" "$IMPL_SORTED" >/dev/null; then
  echo "host-abi-conformance: documented and implemented host ABI op surfaces differ"
  diff -u "$DOC_SORTED" "$IMPL_SORTED" || true
  exit 1
fi

echo "host-abi-conformance: ok"

python3 - "$FFI_SIGNED_PROFILE" <<'PY'
import pathlib
import re
import sys

profile_path = pathlib.Path(sys.argv[1])
text = profile_path.read_text(encoding="utf-8")

required_ops = ["host/ffi::call", "host/ffi::buffer-pin", "host/ffi::buffer-unpin"]
for op in required_ops:
    if op not in text:
        raise SystemExit(f"host-abi-conformance: signed ffi profile missing `{op}`")

allow_block = re.search(r"allow\s*=\s*\[(?P<body>.*?)\]", text, flags=re.DOTALL)
if not allow_block:
    raise SystemExit("host-abi-conformance: signed ffi profile missing top-level allow list")
allow_body = allow_block.group("body")
for op in required_ops:
    if f"\"{op}\"" not in allow_body:
        raise SystemExit(f"host-abi-conformance: signed ffi profile allow list missing `{op}`")

def section_text(op: str) -> str:
    pat = rf"\[op\.\"{re.escape(op)}\"\](?P<body>.*?)(?:\n\s*\[op\.\"|\Z)"
    m = re.search(pat, text, flags=re.DOTALL)
    if not m:
        raise SystemExit(f"host-abi-conformance: signed ffi profile missing section [op.\"{op}\"]")
    return m.group("body")

required_common = [
    "bridge_cmd",
    "bridge_cmd_sha256",
    "signed_policy_required",
    "policy_artifact_h",
    "policy_signature_h",
    "policy_key_id",
    "evidence_mode",
    "allow_abi_ids",
]
for op in required_ops:
    body = section_text(op)
    for key in required_common:
        if not re.search(rf"^\s*{re.escape(key)}\s*=", body, flags=re.MULTILINE):
            raise SystemExit(f"host-abi-conformance: {op} missing required key `{key}`")
    if not re.search(r"^\s*signed_policy_required\s*=\s*true\s*$", body, flags=re.MULTILINE):
        raise SystemExit(f"host-abi-conformance: {op} must set signed_policy_required=true")
    if not re.search(r'^\s*evidence_mode\s*=\s*"deterministic"\s*$', body, flags=re.MULTILINE):
        raise SystemExit(f"host-abi-conformance: {op} must set evidence_mode=\"deterministic\"")
    if op == "host/ffi::call":
        for key in ("allow_libraries", "allow_symbols", "max_call_payload_bytes"):
            if not re.search(rf"^\s*{re.escape(key)}\s*=", body, flags=re.MULTILINE):
                raise SystemExit(f"host-abi-conformance: {op} missing required key `{key}`")
    if op == "host/ffi::buffer-pin":
        if not re.search(r"^\s*max_buffer_bytes\s*=", body, flags=re.MULTILINE):
            raise SystemExit(f"host-abi-conformance: {op} missing required key `max_buffer_bytes`")

print(f"host-abi-conformance: signed ffi profile validated {profile_path}")
PY

if [[ "${GENESIS_HOST_ABI_SKIP_POLICY_TESTS:-0}" != "1" ]]; then
  echo "host-abi-conformance: running ffi policy profile checks"
  cargo test -p gc_effects --lib extended_ffi --quiet
  echo "host-abi-conformance: running deny-by-default abuse guard checks"
  cargo test -p gc_effects --test untrusted_agent_safety --quiet
fi

echo "host-abi-conformance: runtime policy checks ok"
