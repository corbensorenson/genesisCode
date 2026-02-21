#!/usr/bin/env sh
set -eu

backend="${GENESIS_AGENT_GPU_BACKEND:-deterministic-fallback}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

resp="{:ok true :kind \"gpu-compute-submit\" :backend \"$backend\" :checksum 424242}"
printf '%s\n%s' "${#resp}" "$resp"
