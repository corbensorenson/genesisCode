#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp="$(mktemp "${TMPDIR:-/tmp}/genesis-gc-agent-profile.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
python3 scripts/lib/gc_agent_profile.py --render > "$tmp"
mv "$tmp" docs/spec/GC_AGENT_PROFILE_v0.3.json
bash scripts/check_gc_agent_profile.sh
echo "update-gc-agent-profile: wrote docs/spec/GC_AGENT_PROFILE_v0.3.json"
