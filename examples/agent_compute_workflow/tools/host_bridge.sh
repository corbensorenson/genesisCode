#!/usr/bin/env sh
set -eu

backend="${GENESIS_AGENT_GPU_BACKEND:-deterministic-fallback}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

resp="{:ok true :backend \"$backend\" :id \"gpu-bridge-0\" :data b\"\\x01\\x02\\x03\\x04\" :written 4}"
printf '%s\n%s' "${#resp}" "$resp"
