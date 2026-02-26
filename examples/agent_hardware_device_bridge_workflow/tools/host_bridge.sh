#!/usr/bin/env sh
set -eu

op="${GENESIS_HOST_BRIDGE_OP:-}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

case "$op" in
  "host/plugin::command")
    resp='{:ok true :status "host-ok" :plugin "demo"}'
    ;;
  "editor/plugin::command")
    resp='{:ok true :status "editor-ok" :plugin "demo"}'
    ;;
  *)
    resp='{:ok false :code "bridge/unsupported-op"}'
    ;;
esac

printf '%s\n%s' "${#resp}" "$resp"
