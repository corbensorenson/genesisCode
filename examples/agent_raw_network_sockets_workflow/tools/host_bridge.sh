#!/usr/bin/env sh
set -eu

op="${GENESIS_HOST_BRIDGE_OP:-}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

case "$op" in
  "io/net::tcp-open")
    resp='{:ok true :stream-id "tcp-1"}'
    ;;
  "io/net::tcp-send")
    resp='{:ok true :sent true}'
    ;;
  "io/net::tcp-recv")
    resp='{:ok true :data "pong"}'
    ;;
  "io/net::tcp-close")
    resp='{:ok true :closed true}'
    ;;
  *)
    resp='{:ok false :code "bridge/unsupported-op"}'
    ;;
esac

printf '%s\n%s' "${#resp}" "$resp"
