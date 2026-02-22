#!/usr/bin/env sh
set -eu

op="${GENESIS_HOST_BRIDGE_OP:-}"
IFS= read -r req_len
if [ -z "${req_len:-}" ]; then
  exit 1
fi
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true

case "$op" in
  "io/db::connect")
    resp='{:ok true :connection-id "db-1"}'
    ;;
  "io/db::tx-begin")
    resp='{:ok true :tx-id "tx-1"}'
    ;;
  "io/db::query")
    resp='{:ok true :rows [{:id 1}] :row-count 1}'
    ;;
  "io/db::exec")
    resp='{:ok true :affected-rows 1}'
    ;;
  "io/db::tx-commit")
    resp='{:ok true :committed true}'
    ;;
  "io/db::tx-rollback")
    resp='{:ok true :rolled-back true}'
    ;;
  "io/db::kv-open")
    resp='{:ok true :store-id "kv-1"}'
    ;;
  "io/db::kv-get")
    resp='{:ok true :found true :value "v1"}'
    ;;
  "io/db::kv-put")
    resp='{:ok true :written true}'
    ;;
  "io/db::kv-delete")
    resp='{:ok true :deleted true}'
    ;;
  *)
    resp='{:ok false :code "bridge/unsupported-op"}'
    ;;
esac

printf '%s\n%s' "${#resp}" "$resp"
