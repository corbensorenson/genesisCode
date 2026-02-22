#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CI_FILE=".github/workflows/ci.yml"
if [[ ! -f "$CI_FILE" ]]; then
  echo "webxr-browser-conformance-lane: missing $CI_FILE" >&2
  exit 1
fi

require_pattern() {
  local pattern="$1"
  local message="$2"
  if command -v rg >/dev/null 2>&1; then
    if rg -q --fixed-strings -- "$pattern" "$CI_FILE"; then
      return 0
    fi
  elif grep -Fq -- "$pattern" "$CI_FILE"; then
    return 0
  fi
  echo "webxr-browser-conformance-lane: $message" >&2
  exit 1
}

require_pattern "webxr_browser_conformance:" "missing webxr browser conformance job"
require_pattern "github.event_name == 'pull_request'" "webxr lane must run on pull_request events"
require_pattern "bash scripts/check_webxr_browser_conformance.sh" "missing webxr browser conformance checker invocation"
require_pattern "webxr-browser-conformance-artifacts" "missing webxr browser conformance artifact upload"

echo "webxr-browser-conformance-lane: ok"
