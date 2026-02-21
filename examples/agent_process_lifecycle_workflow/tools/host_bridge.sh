#!/usr/bin/env sh
set -eu

op="${GENESIS_HOST_BRIDGE_OP:-}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

case "$op" in
  "sys/process::spawn")
    resp='{:ok true :process-id "proc-1"}'
    ;;
  "sys/process::stdout-read")
    resp='{:ok true :stdout "hello" :stderr ""}'
    ;;
  "sys/process::stdin-write")
    resp='{:ok true :written true}'
    ;;
  "sys/process::wait")
    resp='{:ok true :exit 0}'
    ;;
  *)
    resp='{:ok false :code "bridge/unsupported-op"}'
    ;;
esac

printf '%s\n%s' "${#resp}" "$resp"
