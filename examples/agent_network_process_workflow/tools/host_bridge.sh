#!/usr/bin/env sh
set -eu

op="${GENESIS_HOST_BRIDGE_OP:-}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

case "$op" in
  "io/net::http-request")
    resp='{:ok true :status 200 :headers [] :body "ok" :backend "bridge-net"}'
    ;;
  "sys/process::exec")
    resp='{:ok true :exit 0 :stdout "pong" :stderr "" :backend "bridge-process"}'
    ;;
  *)
    resp='{:ok false :code "bridge/unsupported-op"}'
    ;;
esac

printf '%s\n%s' "${#resp}" "$resp"
