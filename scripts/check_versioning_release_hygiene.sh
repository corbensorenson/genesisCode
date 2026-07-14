#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "versioning-release-hygiene" \
  root-host

META_JSON="$(mktemp)"
trap 'rm -f "$META_JSON"' EXIT

cargo metadata --no-deps --format-version 1 > "$META_JSON"
bash scripts/check_version_surfaces.sh
bash scripts/check_v1_compatibility.sh

python3 - "$ROOT_DIR" "$META_JSON" <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path(sys.argv[1])
metadata_path = Path(sys.argv[2])
root_manifest = root / "Cargo.toml"
registry = json.loads((root / "genesis.version-surfaces.json").read_text())
version = registry["release_train"]
if version != "0.2.0":
    raise SystemExit(f"versioning-release-hygiene: expected release train 0.2.0, found {version!r}")
root_text = root_manifest.read_text()
section_match = re.search(r"(?ms)^\[workspace\.package\]\s*$\n(.*?)(?=^\[|\Z)", root_text)
if not section_match:
    raise SystemExit("versioning-release-hygiene: missing [workspace.package]")
workspace_package = section_match.group(1)
if f'version = "{version}"' not in workspace_package:
    raise SystemExit("versioning-release-hygiene: workspace version does not match release train")
if "publish = false" not in workspace_package:
    raise SystemExit("versioning-release-hygiene: workspace package publish boundary must remain false before registry GA")

metadata = json.loads(metadata_path.read_text())
workspace_ids = set(metadata.get("workspace_members", []))
packages = [pkg for pkg in metadata.get("packages", []) if pkg.get("id") in workspace_ids]
if not packages:
    raise SystemExit("versioning-release-hygiene: cargo metadata returned no workspace packages")

bad_versions = [(pkg["name"], pkg["version"]) for pkg in packages if pkg.get("version") != version]
if bad_versions:
    rendered = ", ".join(f"{name}={pkg_version}" for name, pkg_version in sorted(bad_versions))
    raise SystemExit(f"versioning-release-hygiene: workspace package version drift: {rendered}")

metadata_failures = []
for pkg in packages:
    if pkg.get("edition") != "2024":
        metadata_failures.append(f"{pkg['name']}: edition={pkg.get('edition')!r}")
    if pkg.get("license") != "MIT OR Apache-2.0":
        metadata_failures.append(f"{pkg['name']}: license={pkg.get('license')!r}")
    if pkg.get("publish") != []:
        metadata_failures.append(f"{pkg['name']}: publish={pkg.get('publish')!r}")
if metadata_failures:
    raise SystemExit("versioning-release-hygiene: cargo metadata boundary drift:\n" + "\n".join(metadata_failures))

manifest_failures = []
for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
    text = manifest.read_text()
    rel = manifest.relative_to(root)
    for field in ("version", "edition", "license", "publish"):
        if not re.search(rf"(?m)^{field}\.workspace\s*=\s*true\s*$", text):
            manifest_failures.append(f"{rel}: package.{field} must be workspace-inherited")
if manifest_failures:
    raise SystemExit("versioning-release-hygiene:\n" + "\n".join(manifest_failures))

required_docs = [
    "docs/spec/VERSIONING_v0.1.md",
    "docs/spec/RELEASE_SMOKE_v0.1.md",
    "CHANGELOG.md",
]
for rel in required_docs:
    if not (root / rel).is_file():
        raise SystemExit(f"versioning-release-hygiene: missing required release doc: {rel}")

changelog = (root / "CHANGELOG.md").read_text()
if "## [0.2.0] - 2026-07-02" not in changelog:
    raise SystemExit("versioning-release-hygiene: CHANGELOG.md must contain dated 0.2.0 entry")
if "## [Unreleased]" not in changelog:
    raise SystemExit("versioning-release-hygiene: CHANGELOG.md must keep an Unreleased section")

versioning_doc = (root / "docs/spec/VERSIONING_v0.1.md").read_text()
for needle in [
    "Current workspace package version: `0.2.0`",
    "[workspace.package].version",
    "version.workspace = true",
    "publish = false",
    "publish.workspace = true",
    "selfhost/toolchain.gc",
]:
    if needle not in versioning_doc:
        raise SystemExit(f"versioning-release-hygiene: VERSIONING doc missing {needle!r}")

release_doc = (root / "docs/spec/RELEASE_SMOKE_v0.1.md").read_text()
install_cmd = "cargo install --path crates/gc_cli --locked --root .cargo-install-target"
if install_cmd not in release_doc:
    raise SystemExit("versioning-release-hygiene: RELEASE_SMOKE doc must include intended cargo install path")

index_doc = (root / "docs/INDEX.md").read_text()
for rel in ["docs/spec/VERSIONING_v0.1.md", "docs/spec/RELEASE_SMOKE_v0.1.md", "CHANGELOG.md"]:
    if rel not in index_doc:
        raise SystemExit(f"versioning-release-hygiene: docs/INDEX.md missing {rel}")

topology_doc = (root / "docs/spec/DOC_TOPOLOGY_v0.1.md").read_text()
for rel in ["docs/spec/VERSIONING_v0.1.md", "docs/spec/RELEASE_SMOKE_v0.1.md", "CHANGELOG.md"]:
    if rel not in topology_doc:
        raise SystemExit(f"versioning-release-hygiene: DOC_TOPOLOGY missing {rel}")

artifact = root / "selfhost/toolchain.gc"
if not artifact.is_file():
    raise SystemExit("versioning-release-hygiene: missing selfhost/toolchain.gc")
needle = f':generated-by "genesis {version}"'.encode()
if needle not in artifact.read_bytes():
    raise SystemExit(f"versioning-release-hygiene: selfhost/toolchain.gc must contain {needle.decode()}")

print("versioning-release-hygiene: ok")
PY
