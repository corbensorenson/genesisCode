#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

extract_call_capability_ops() {
  local file="$1"
  if [[ ! -f "$file" ]]; then
    echo "capability-dispatch-ops: missing file: $file" >&2
    return 1
  fi

  awk '
    /fn call_capability\(/ { in_fn = 1; }
    in_fn && /match op(_eff)? \{/ { in_match = 1; next; }
    in_match {
      if ($0 ~ /^[[:space:]]*_[[:space:]]*=>/) {
        in_match = 0;
        in_fn = 0;
      }
      if ($0 ~ /^[[:space:]]*"[[:alnum:]\/:_-]+::[[:alnum:]\/:_-]+"[[:space:]]*=>/ || $0 ~ /^[[:space:]]*"[[:alnum:]\/:_-]+::[[:alnum:]\/:_-]+"[[:space:]]*$/ || $0 ~ /^[[:space:]]*\|[[:space:]]*"[[:alnum:]\/:_-]+::[[:alnum:]\/:_-]+"/) {
        line = $0;
        sub(/^[[:space:]]*\|?[[:space:]]*"/, "", line);
        sub(/".*$/, "", line);
        print line;
      }
    }
  ' "$file" | sort -u
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  mode="${1:-}"
  shift || true
  case "$mode" in
    --call-capability)
      if [[ "$#" -ne 1 ]]; then
        echo "usage: $(basename "$0") --call-capability <runner-capability-dispatch.rs>" >&2
        exit 2
      fi
      extract_call_capability_ops "$1"
      ;;
    *)
      echo "usage: $(basename "$0") --call-capability <runner-capability-dispatch.rs>" >&2
      exit 2
      ;;
  esac
fi
