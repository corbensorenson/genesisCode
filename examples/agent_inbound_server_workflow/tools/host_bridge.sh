#!/usr/bin/env sh
set -eu

op="${GENESIS_HOST_BRIDGE_OP:-}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

case "$op" in
  "io/net::tcp-listen")
    resp='{:ok true :listener-id "tcp-listener-1"}'
    ;;
  "io/net::tcp-accept")
    resp='{:ok true :request-id "req-1" :remote "tcp://127.0.0.1:54001" :data b"ping"}'
    ;;
  "io/net::http-listen")
    resp='{:ok true :listener-id "http-listener-1" :request-id "req-1" :method "GET" :path "/health" :body b""}'
    ;;
  "io/net::http-respond")
    resp='{:ok true :sent true :status 200}'
    ;;
  "io/net::ws-accept")
    resp='{:ok true :stream-id "ws-accepted-1"}'
    ;;
  *)
    resp='{:ok false :code "bridge/unsupported-op"}'
    ;;
esac

printf '%s\n%s' "${#resp}" "$resp"
