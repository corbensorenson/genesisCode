#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "release-smoke" \
  root-host

for path in docs/spec/VERSIONING_v0.1.md docs/spec/RELEASE_SMOKE_v0.1.md CHANGELOG.md Cargo.toml; do
  [[ -f "$path" ]] || {
    echo "release-smoke: missing required file: $path" >&2
    exit 1
  }
done

bash scripts/check_release_notes.sh

META_JSON="$(mktemp)"
NATIVE_HELP="$(mktemp)"
WASI_HELP="$(mktemp)"
VERSION_FILE="$(mktemp)"
NATIVE_VERSION="$(mktemp)"
WASI_VERSION="$(mktemp)"
PACKAGE_LOG="$(mktemp)"
trap 'rm -f "$META_JSON" "$NATIVE_HELP" "$WASI_HELP" "$VERSION_FILE" "$NATIVE_VERSION" "$WASI_VERSION" "$PACKAGE_LOG"' EXIT

cargo metadata --no-deps --format-version 1 > "$META_JSON"
python3 - "$META_JSON" "$VERSION_FILE" <<'PY'
import json
import sys
from collections import defaultdict
metadata = json.loads(open(sys.argv[1], encoding="utf-8").read())
workspace_ids = set(metadata.get("workspace_members", []))
versions = defaultdict(list)
for pkg in metadata.get("packages", []):
    if pkg.get("id") in workspace_ids:
        versions[pkg.get("version")].append(pkg.get("name"))
if len(versions) != 1:
    rendered = "; ".join(f"{version}: {sorted(names)}" for version, names in sorted(versions.items()))
    raise SystemExit(f"release-smoke: workspace version drift: {rendered}")
version = next(iter(versions))
open(sys.argv[2], "w", encoding="utf-8").write(version + "\n")
print(f"release-smoke: workspace version {version}")
PY
EXPECTED_VERSION="$(tr -d '\n' < "$VERSION_FILE")"

if ! grep -Fq 'cargo install --path crates/gc_cli --locked --root .cargo-install-target' docs/spec/RELEASE_SMOKE_v0.1.md; then
  echo "release-smoke: intended cargo install path is not documented" >&2
  exit 1
fi

echo "release-smoke: package file selection"
if ! cargo package --workspace --allow-dirty --no-verify --list >/dev/null 2>"$PACKAGE_LOG"; then
  cat "$PACKAGE_LOG" >&2
  exit 1
fi

if [[ "${GENESIS_RELEASE_SMOKE_PACKAGE_DRY_RUN:-0}" == "1" ]]; then
  echo "release-smoke: package dry run"
  : > "$PACKAGE_LOG"
  if ! cargo package --workspace --allow-dirty --no-verify >/dev/null 2>"$PACKAGE_LOG"; then
    cat "$PACKAGE_LOG" >&2
    exit 1
  fi
fi

echo "release-smoke: native CLI help contract"
cargo run --quiet -p gc_cli -- --help > "$NATIVE_HELP"
grep -Fq 'Usage: genesis' "$NATIVE_HELP" || {
  echo "release-smoke: native CLI help missing 'Usage: genesis'" >&2
  exit 1
}
cargo run --quiet -p gc_cli -- --version > "$NATIVE_VERSION"
grep -Fxq "genesis $EXPECTED_VERSION" "$NATIVE_VERSION" || {
  echo "release-smoke: native CLI version drift: $(cat "$NATIVE_VERSION")" >&2
  exit 1
}

echo "release-smoke: WASI CLI help contract"
cargo run --quiet -p gc_wasi_cli --bin genesis_wasi -- --help > "$WASI_HELP"
grep -Fq 'Usage: genesis_wasi' "$WASI_HELP" || {
  echo "release-smoke: WASI CLI help missing 'Usage: genesis_wasi'" >&2
  exit 1
}
cargo run --quiet -p gc_wasi_cli -- --version > "$WASI_VERSION"
grep -Fxq "genesis $EXPECTED_VERSION" "$WASI_VERSION" || {
  echo "release-smoke: WASI CLI version drift: $(cat "$WASI_VERSION")" >&2
  exit 1
}

echo "release-smoke: ok"
