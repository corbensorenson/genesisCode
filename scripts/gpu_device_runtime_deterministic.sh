#!/usr/bin/env bash
set -euo pipefail

IFS= read -r req_len
if [[ -z "${req_len:-}" || ! "$req_len" =~ ^[0-9]+$ ]]; then
  echo "gpu-device-runtime-deterministic: expected request length on stdin" >&2
  exit 64
fi

if [[ "$req_len" -gt 0 ]]; then
  dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
fi

checksum="$(
  python3 - "$req_len" <<'PY'
import sys

n = int(sys.argv[1])
acc = 2166136261
for i in range(50000 + n):
    acc = (acc ^ (i & 0xFF)) * 16777619
    acc &= 0xFFFFFFFF
print(acc)
PY
)"

response="{:ok true :kind \"gpu-compute-submit\" :backend \"device-runtime\" :adapter \"deterministic-ci-runner\" :checksum $checksum}"
response_len="$(printf '%s' "$response" | wc -c | tr -d '[:space:]')"
printf '%s\n%s' "$response_len" "$response"
