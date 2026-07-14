#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir "$ROOT_DIR" changed-impact-contract root-host

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-changed-impact.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT
cargo metadata --locked --offline --no-deps --format-version 1 >"$TMP_DIR/metadata.json"

python3 - "$ROOT_DIR" "$TMP_DIR" <<'PY'
import copy
from hashlib import sha256
import json
from pathlib import Path
import subprocess
import sys

root = Path(sys.argv[1]).resolve()
temp = Path(sys.argv[2])
sys.path.insert(0, str(root / "scripts/lib"))
import changed_impact as impact

metadata_path = temp / "metadata.json"
controls = []

def require(value, message):
    if not value:
        raise SystemExit(f"changed-impact-contract: {message}")

def plan(paths):
    return impact.resolve(root, paths, "cargo", metadata_path)

core = plan(["crates/gc_coreform/src/lib.rs"])
metadata = impact.load_json(metadata_path)
workspace_names = sorted(p["name"] for p in metadata["packages"] if p["id"] in set(metadata["workspace_members"]))
require(core["affectedCrates"] == workspace_names, "gc_coreform must reach the full reverse dependency closure")
require(
    core["mode"] == "profile-fallback"
    and core["commands"] == ["bash scripts/test_fast_full.sh"]
    and "targeted-cardinality-limit" in core["fallbackReasons"],
    "large reverse dependency closure must use the bounded full-fast profile",
)
controls.append("root-crate-bounded-fallback")

leaf = plan(["crates/gc_wasm/src/lib.rs"])
require(leaf["affectedCrates"] == ["gc_wasm"], "leaf crate closure drift")
controls.append("leaf-crate-precision")

schema = plan(["docs/spec/CARGO_CACHE_POLICY_v0.1.schema.json"])
require(schema["mode"] == "profile-fallback" and schema["fallbackProfile"] == "prepush-standard", "schema change did not escalate")
require(schema["generatedImpacts"] and schema["directGates"], "schema lacks generated/gate mapping")
controls.append("generated-schema-escalation")

unknown = plan(["new-top-level/unknown.asset"])
require(unknown["mode"] == "profile-fallback" and "unclassified-path" in unknown["fallbackReasons"], "unknown path narrowed coverage")
controls.append("unknown-path-escalation")

for bad in ("../escape", "/tmp/escape", "./README.md", "a\\b"):
    try:
        plan([bad])
    except impact.ImpactError:
        pass
    else:
        raise SystemExit(f"changed-impact-contract: accepted malformed path: {bad}")
controls.append("noncanonical-path-rejection")

first = plan(["crates/gc_wasm/src/lib.rs", "README.md"])
second = plan(["README.md", "crates/gc_wasm/src/lib.rs", "README.md"])
require(first == second, "selection is order/duplicate sensitive")
require(str(root) not in json.dumps(first, sort_keys=True), "plan leaked checkout path")
controls.append("deterministic-path-free-plan")

repo = temp / "git"
repo.mkdir()
subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
subprocess.run(["git", "config", "user.email", "impact@example.invalid"], cwd=repo, check=True)
subprocess.run(["git", "config", "user.name", "Impact Test"], cwd=repo, check=True)
(repo / "tracked").write_text("one\n", encoding="utf-8")
(repo / "rename-me").write_text("rename\n", encoding="utf-8")
subprocess.run(["git", "add", "."], cwd=repo, check=True)
subprocess.run(["git", "commit", "-qm", "base"], cwd=repo, check=True)
base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
(repo / "tracked").write_text("two\n", encoding="utf-8")
subprocess.run(["git", "mv", "rename-me", "renamed"], cwd=repo, check=True)
(repo / "untracked").write_text("new\n", encoding="utf-8")
observed = impact.git_changed_files(repo, base)
require(observed == ["rename-me", "renamed", "tracked", "untracked"], f"Git state coverage drift: {observed}")
controls.append("complete-git-state-collection")

policy = impact.load_policy(root)
schema = impact.load_json(root / "docs/spec/CHANGED_IMPACT_POLICY_v0.1.schema.json")
require(
    schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
    and schema.get("$id") == "https://genesiscode.dev/schemas/changed-impact-policy-v0.1.json"
    and schema.get("additionalProperties") is False
    and set(schema.get("required", [])) == impact.POLICY_FIELDS,
    "policy schema identity/closure drift",
)
controls.append("schema-closure")
mutated = copy.deepcopy(policy)
mutated["unknown"] = True
bad_policy = temp / "bad-policy.json"
bad_policy.write_text(json.dumps(mutated), encoding="utf-8")
try:
    doc = impact.load_json(bad_policy)
    if set(doc) != impact.POLICY_FIELDS:
        raise impact.ImpactError("policy fields mismatch")
except impact.ImpactError:
    pass
else:
    raise SystemExit("changed-impact-contract: unknown policy field accepted")
controls.append("closed-policy-rejection")

duplicate = temp / "duplicate-policy.json"
duplicate.write_text('{"kind":"a","kind":"b"}\n', encoding="utf-8")
try:
    impact.load_json(duplicate)
except impact.ImpactError:
    pass
else:
    raise SystemExit("changed-impact-contract: duplicate policy key accepted")
controls.append("duplicate-key-rejection")

require(len(controls) == 10 and len(set(controls)) == 10, "control coverage drift")
authorities = [
    "policies/changed_impact_v0.1.json",
    "docs/spec/CHANGED_IMPACT_POLICY_v0.1.schema.json",
    "scripts/lib/changed_impact.py",
    "scripts/check_changed_impact.sh",
    "scripts/test_changed_fast.sh",
    "docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md",
    "docs/spec/TEST_EXECUTION_PROFILES_v0.1.md",
    ".github/workflows/ci.yml",
]
identity = [
    {"path": path, "sha256": sha256((root / path).read_bytes()).hexdigest()}
    for path in sorted(authorities)
]
bundle = sha256((json.dumps(identity, sort_keys=True, separators=(",", ":")) + "\n").encode()).hexdigest()
print(
    "changed-impact-contract: ok "
    f"(workspace_crates={len(workspace_names)} gates={len(impact.load_json(root / impact.GATES_REL)['gates'])} "
    f"controls={len(controls)} bundle={bundle})"
)
PY
