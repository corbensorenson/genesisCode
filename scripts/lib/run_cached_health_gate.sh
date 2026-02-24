#!/usr/bin/env bash
set -euo pipefail

PROFILE=""
KEY=""
FINGERPRINT=""
CMD=""
TTL_SEC="${GENESIS_HEALTH_PROFILE_GATE_CACHE_TTL_SEC:-21600}"
META_DIR="${GENESIS_HEALTH_PROFILE_GATE_CACHE_DIR:-.genesis/perf/health_gate_cache}"

usage() {
  cat <<'EOF'
Usage: scripts/lib/run_cached_health_gate.sh \
  --profile <name> \
  --key <cache-key> \
  --fingerprint <hex> \
  [--ttl-sec <seconds>] \
  [--meta-dir <path>] \
  (--cmd <shell command> | --cmd-b64 <base64>)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="${2:-}"
      shift 2
      ;;
    --key)
      KEY="${2:-}"
      shift 2
      ;;
    --fingerprint)
      FINGERPRINT="${2:-}"
      shift 2
      ;;
    --ttl-sec)
      TTL_SEC="${2:-}"
      shift 2
      ;;
    --meta-dir)
      META_DIR="${2:-}"
      shift 2
      ;;
    --cmd)
      CMD="${2:-}"
      shift 2
      ;;
    --cmd-b64)
      CMD="$(printf '%s' "${2:-}" | base64 --decode)"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "run-cached-health-gate: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$PROFILE" || -z "$KEY" || -z "$FINGERPRINT" || -z "$CMD" ]]; then
  echo "run-cached-health-gate: missing required arguments" >&2
  usage >&2
  exit 2
fi
if [[ ! "$TTL_SEC" =~ ^[0-9]+$ ]]; then
  echo "run-cached-health-gate: --ttl-sec must be a non-negative integer" >&2
  exit 2
fi

CACHE_ENABLED="${GENESIS_HEALTH_PROFILE_GATE_CACHE:-1}"
if [[ "$CACHE_ENABLED" != "1" ]]; then
  echo "run-cached-health-gate: cache disabled for key=${KEY}; executing gate"
  exec bash -lc "$CMD"
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
META_PATH="$ROOT_DIR/$META_DIR/$PROFILE/${KEY}.json"
mkdir -p "$(dirname "$META_PATH")"
NOW_UNIX="$(date +%s)"

if [[ -f "$META_PATH" ]]; then
  if python3 - "$META_PATH" "$FINGERPRINT" "$TTL_SEC" "$NOW_UNIX" "$CMD" <<'PY'
import json
import pathlib
import sys

meta_path = pathlib.Path(sys.argv[1])
fingerprint = sys.argv[2]
ttl_sec = int(sys.argv[3])
now_unix = int(sys.argv[4])
cmd = sys.argv[5]

try:
    doc = json.loads(meta_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)

if not isinstance(doc, dict):
    raise SystemExit(1)
if doc.get("kind") != "genesis/health-gate-cache-v0.1":
    raise SystemExit(1)
if doc.get("ok") is not True:
    raise SystemExit(1)
if doc.get("fingerprint") != fingerprint:
    raise SystemExit(1)
if doc.get("command") != cmd:
    raise SystemExit(1)
ran_at = doc.get("ran_at_unix_s")
if not isinstance(ran_at, int):
    raise SystemExit(1)
if ttl_sec > 0 and now_unix - ran_at > ttl_sec:
    raise SystemExit(1)
raise SystemExit(0)
PY
  then
    echo "run-cached-health-gate: cache-hit profile=${PROFILE} key=${KEY} fingerprint=${FINGERPRINT}"
    exit 0
  fi
fi

echo "run-cached-health-gate: cache-miss profile=${PROFILE} key=${KEY} fingerprint=${FINGERPRINT}"
bash -lc "$CMD"

python3 - "$META_PATH" "$PROFILE" "$KEY" "$FINGERPRINT" "$NOW_UNIX" "$TTL_SEC" "$CMD" <<'PY'
import json
import pathlib
import sys

meta_path = pathlib.Path(sys.argv[1])
profile = sys.argv[2]
key = sys.argv[3]
fingerprint = sys.argv[4]
now_unix = int(sys.argv[5])
ttl_sec = int(sys.argv[6])
cmd = sys.argv[7]

doc = {
    "kind": "genesis/health-gate-cache-v0.1",
    "profile": profile,
    "key": key,
    "fingerprint": fingerprint,
    "command": cmd,
    "ok": True,
    "ran_at_unix_s": now_unix,
    "ttl_sec": ttl_sec,
}
meta_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
