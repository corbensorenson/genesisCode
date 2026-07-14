#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "supply-chain" \
  root-host

DENY_CONFIG="deny.toml"
DUPLICATE_MAJOR_ALLOWLIST="policies/supply_chain_duplicate_major_allowlist.txt"

[[ -f "$DENY_CONFIG" ]] || {
  echo "supply-chain: missing $DENY_CONFIG" >&2
  exit 1
}
[[ -f "$DUPLICATE_MAJOR_ALLOWLIST" ]] || {
  echo "supply-chain: missing $DUPLICATE_MAJOR_ALLOWLIST" >&2
  exit 1
}

if ! command -v cargo-deny >/dev/null 2>&1; then
  echo "supply-chain: cargo-deny is required; install with 'cargo install cargo-deny --locked'" >&2
  exit 1
fi

cargo deny check --config "$DENY_CONFIG" advisories bans licenses sources

META_JSON="$(mktemp)"
trap 'rm -f "$META_JSON"' EXIT
cargo metadata --format-version 1 > "$META_JSON"

python3 - "$ROOT_DIR" "$META_JSON" "$DUPLICATE_MAJOR_ALLOWLIST" <<'PY'
from collections import defaultdict
from pathlib import Path
import json
import sys

root = Path(sys.argv[1])
metadata_path = Path(sys.argv[2])
allowlist_path = Path(sys.argv[3])

allowed = set()
for raw in allowlist_path.read_text().splitlines():
    line = raw.split("#", 1)[0].strip()
    if not line:
        continue
    name = line.split("|", 1)[0].strip()
    if not name:
        raise SystemExit(f"supply-chain: malformed duplicate-major allowlist line: {raw!r}")
    allowed.add(name)

metadata = json.loads(metadata_path.read_text())
workspace_ids = set(metadata.get("workspace_members", []))
by_name = defaultdict(lambda: defaultdict(list))
registry_sources = set()
git_sources = []
for pkg in metadata.get("packages", []):
    source = pkg.get("source")
    if pkg.get("id") in workspace_ids or source is None:
        continue
    if source.startswith("registry+"):
        registry_sources.add(source)
    elif source.startswith("git+"):
        git_sources.append((pkg["name"], source))
    version = pkg.get("version", "")
    major = version.split(".", 1)[0]
    by_name[pkg["name"]][major].append(version)

bad_sources = sorted(src for src in registry_sources if src != "registry+https://github.com/rust-lang/crates.io-index")
if bad_sources:
    raise SystemExit("supply-chain: disallowed registries:\n" + "\n".join(bad_sources))
if git_sources:
    rendered = "\n".join(f"{name}: {source}" for name, source in sorted(git_sources))
    raise SystemExit("supply-chain: git dependencies are not allowed by default:\n" + rendered)

violations = []
for name, majors in sorted(by_name.items()):
    if len(majors) <= 1 or name in allowed:
        continue
    rendered = ", ".join(f"{major}.x={sorted(set(versions))}" for major, versions in sorted(majors.items()))
    violations.append(f"{name}: {rendered}")

stale_allowlist = sorted(name for name in allowed if name not in by_name or len(by_name[name]) <= 1)
if stale_allowlist:
    raise SystemExit(
        "supply-chain: stale duplicate-major allowlist entries; remove or refresh reasons:\n"
        + "\n".join(stale_allowlist)
    )
if violations:
    raise SystemExit(
        "supply-chain: unapproved duplicate-major dependency families:\n"
        + "\n".join(violations)
        + "\nAdd only temporary, reasoned entries to policies/supply_chain_duplicate_major_allowlist.txt."
    )

print("supply-chain: duplicate-major policy ok")
PY

echo "supply-chain: ok"
