#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OUTPUT="${1:?usage: scripts/render_reference_host_observation.sh <output.json>}"
exec python3 scripts/lib/reference_host_profiles.py probe --output "$OUTPUT"
