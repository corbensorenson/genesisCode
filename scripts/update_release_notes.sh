#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

json_tmp="$(mktemp "${TMPDIR:-/tmp}/genesis-release-notes-json.XXXXXX")"
changelog_tmp="$(mktemp "${TMPDIR:-/tmp}/genesis-release-notes-changelog.XXXXXX")"
trap 'rm -f "$json_tmp" "$changelog_tmp"' EXIT

python3 scripts/lib/release_notes.py --render-json > "$json_tmp"
python3 scripts/lib/release_notes.py --render-changelog > "$changelog_tmp"

mkdir -p docs/program
mv "$json_tmp" docs/program/RELEASE_NOTES_v0.2.0.json
mv "$changelog_tmp" CHANGELOG.md
bash scripts/check_release_notes.sh
echo "update-release-notes: wrote docs/program/RELEASE_NOTES_v0.2.0.json and CHANGELOG.md"
