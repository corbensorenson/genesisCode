#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-cleanup-contract.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

PYTHONDONTWRITEBYTECODE=1 GENESIS_GATE_TELEMETRY_DISABLE=1 python3 - "$ROOT_DIR" "$TMP_DIR" <<'PY'
import copy
from hashlib import sha256
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

source_root = Path(sys.argv[1]).resolve()
temp = Path(sys.argv[2]).resolve()
sys.path.insert(0, str(source_root / "scripts/lib"))
import deterministic_cleanup as cleanup

controls = []


def require(value, message):
    if not value:
        raise SystemExit(f"deterministic-cleanup-contract: {message}")


def rejected(name, function):
    try:
        function()
    except cleanup.CleanupError:
        controls.append(name)
        return
    raise SystemExit(f"deterministic-cleanup-contract: accepted invalid control: {name}")


def run_cli(root, *args):
    return subprocess.run(
        [sys.executable, str(source_root / "scripts/lib/deterministic_cleanup.py"), "--root", str(root), *map(str, args)],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def write_plan(path, plan):
    path.write_bytes(cleanup.pretty_bytes(plan))
    return cleanup.digest_bytes(cleanup.canonical_bytes(plan))


policy, _, policy_sha = cleanup.load_policy(source_root)
schema_paths = [
    "docs/spec/DETERMINISTIC_CLEANUP_POLICY_v0.1.schema.json",
    "docs/spec/DETERMINISTIC_CLEANUP_MARKER_v0.1.schema.json",
    "docs/spec/DETERMINISTIC_CLEANUP_PLAN_v0.1.schema.json",
    "docs/spec/DETERMINISTIC_CLEANUP_RESULT_v0.1.schema.json",
]
for relative in schema_paths:
    schema = cleanup.load_json(source_root / relative)
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
        and schema.get("additionalProperties") is False,
        f"schema is not closed: {relative}",
    )
require([item["id"] for item in policy["classes"]] == cleanup.CLASS_IDS, "class identity drift")
require([item["id"] for item in policy["profiles"]] == cleanup.PROFILE_IDS, "profile identity drift")
controls.append("closed-authority-contract")

repo = temp / "repo"
(repo / "policies").mkdir(parents=True)
shutil.copyfile(source_root / cleanup.POLICY_REL, repo / cleanup.POLICY_REL)
(repo / ".gitignore").write_text(".genesis/\n.tmp/\n.cargo-install-target/\nnode_modules/\ntarget/\n", encoding="utf-8")
(repo / "source.gc").write_text("(module fixture)\n", encoding="utf-8")
subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
subprocess.run(["git", "add", ".gitignore", "source.gc"], cwd=repo, check=True)

for relative in (
    ".genesis/build",
    ".genesis/cache",
    ".genesis/dependency-mirrors",
    ".genesis/perf",
    ".genesis/store",
    ".genesis/custom",
    "node_modules",
):
    (repo / relative).mkdir(parents=True)
(repo / ".genesis/build/payload.bin").write_bytes(b"build" * 128)
(repo / ".genesis/cache/cache.bin").write_bytes(b"cache")
(repo / ".genesis/dependency-mirrors/mirror.bin").write_bytes(b"mirror")
(repo / ".genesis/perf/history.jsonl").write_text("{}\n", encoding="utf-8")
(repo / ".genesis/store/user.gc").write_text("user-authored\n", encoding="utf-8")
(repo / ".genesis/custom/notes.gc").write_text("unknown-user-data\n", encoding="utf-8")
(repo / "node_modules/generated.js").write_text("generated\n", encoding="utf-8")
outside = temp / "outside"
outside.mkdir()
(repo / "target").symlink_to(outside, target_is_directory=True)

initial = cleanup.render_plan(repo, "dev-clean")
by_path = {item["path"]: item for item in initial["entries"]}
require(by_path[".genesis/build"]["reason"] == "missing-marker", "unmarked build root was eligible")
require(by_path[".genesis/store"]["reason"] == "policy-user-authored", "store was not protected")
require(by_path[".genesis/custom"]["reason"] == "unknown-untracked-root", "unknown root was not protected")
require(by_path["target"]["reason"] == "symlink-root", "symlink root was not protected")
require(initial["summary"]["deleteRoots"] == 0, "unmarked fixture planned deletion")
controls.append("unmarked-and-user-data-protection")

for relative, producer in (
    (".genesis/build", "fixture-build"),
    (".genesis/cache", "fixture-cache"),
    (".genesis/dependency-mirrors", "fixture-mirror"),
    (".genesis/perf", "fixture-evidence"),
    ("node_modules", "fixture-node"),
):
    cleanup.initialize_root_marker(repo, relative, producer)
controls.append("reviewed-producer-markers")

subprocess.run(["git", "add", "-f", "node_modules/generated.js"], cwd=repo, check=True)
cache_marker = repo / ".genesis/cache" / policy["markerFile"]
tampered = cleanup.load_json(cache_marker)
tampered["policySha256"] = "0" * 64
cache_marker.write_bytes(cleanup.pretty_bytes(tampered))

before = cleanup.tree_stats(repo / ".genesis/build", policy["limits"])
plan_a = cleanup.render_plan(repo, "dev-clean")
plan_b = cleanup.render_plan(repo, "dev-clean")
after = cleanup.tree_stats(repo / ".genesis/build", policy["limits"])
require(cleanup.canonical_bytes(plan_a) == cleanup.canonical_bytes(plan_b), "dry-run is nondeterministic")
require(before == after, "dry-run mutated a cleanup root")
controls.append("deterministic-read-only-dry-run")

by_path = {item["path"]: item for item in plan_a["entries"]}
require(by_path[".genesis/build"]["action"] == "delete", "marked build root was not eligible")
require(by_path[".genesis/cache"]["reason"] == "invalid-marker", "tampered marker was accepted")
require(by_path["node_modules"]["reason"] == "tracked-content", "tracked generated-root content was not protected")
require(by_path[".genesis/dependency-mirrors"]["reason"] == "class-not-selected", "mirror class selection drift")
require(by_path[".genesis/perf"]["reason"] == "class-not-selected", "evidence class selection drift")
controls.append("class-marker-and-tracked-selection")

cleanup.validate_plan_shape(plan_a)
rendered = cleanup.canonical_bytes(plan_a).decode("ascii")
require(str(repo) not in rendered and str(temp) not in rendered, "plan leaked a host path")
require(all(not (item["class"] == "user-authored" and item["action"] == "delete") for item in plan_a["entries"]), "plan deletes user data")
controls.append("closed-portable-plan-shape")

rejected(
    "repository-output-rejection",
    lambda: cleanup.safe_output_path(repo, repo / "plan.json", policy),
)
inside_plan = repo / "inside-plan.json"
inside_sha = write_plan(inside_plan, plan_a)
rejected(
    "repository-execution-plan-rejection",
    lambda: cleanup.execute_plan(repo, inside_plan, inside_sha),
)
inside_plan.unlink()

linked_repo = temp / "linked-repo"
(linked_repo / "policies").mkdir(parents=True)
shutil.copyfile(source_root / cleanup.POLICY_REL, linked_repo / cleanup.POLICY_REL)
(linked_repo / ".gitignore").write_text(".genesis/\n", encoding="utf-8")
(linked_repo / "source.gc").write_text("fixture\n", encoding="utf-8")
subprocess.run(["git", "init", "-q"], cwd=linked_repo, check=True)
subprocess.run(["git", "add", ".gitignore", "source.gc"], cwd=linked_repo, check=True)
linked_target = temp / "linked-genesis"
linked_target.mkdir()
(linked_repo / ".genesis").symlink_to(linked_target, target_is_directory=True)
rejected(
    "symlinked-parent-rejection",
    lambda: cleanup.render_plan(linked_repo, "dev-clean"),
)

duplicate = temp / "duplicate.json"
duplicate.write_text('{"kind":"a","kind":"b"}\n', encoding="utf-8")
rejected("duplicate-key-rejection", lambda: cleanup.load_json(duplicate))

bad_policy = copy.deepcopy(policy)
bad_policy["classes"][1]["roots"][0] = "../escape"
bad_path = repo / "policies/bad-cleanup.json"
bad_path.write_bytes(cleanup.pretty_bytes(bad_policy))
rejected("noncanonical-policy-path-rejection", lambda: cleanup.load_policy(repo, bad_path))

rejected(
    "user-authored-marker-rejection",
    lambda: cleanup.initialize_root_marker(repo, ".genesis/store", "fixture-user"),
)
rejected(
    "tracked-root-marker-rejection",
    lambda: cleanup.initialize_root_marker(repo, "node_modules", "fixture-node"),
)

plan_path = temp / "dev-plan.json"
plan_sha = write_plan(plan_path, plan_a)
wrong = run_cli(repo, "--execute", "--plan", plan_path, "--confirm-sha256", "0" * 64)
require(wrong.returncode == 2 and (repo / ".genesis/build").is_dir(), "wrong confirmation did not fail closed")
controls.append("confirmation-binding")

(repo / ".genesis/build/drift.bin").write_bytes(b"drift")
stale = run_cli(repo, "--execute", "--plan", plan_path, "--confirm-sha256", plan_sha)
require(stale.returncode == 2 and "plan is stale" in stale.stderr and (repo / ".genesis/build/drift.bin").is_file(), "stale plan was not rejected")
controls.append("post-plan-drift-rejection")

fresh = cleanup.render_plan(repo, "dev-clean")
fresh_path = temp / "fresh-plan.json"
fresh_sha = write_plan(fresh_path, fresh)
original_tree_stats = cleanup.tree_stats


def corrupt_after_quarantine(path, limits, expected_device=None):
    value = original_tree_stats(path, limits, expected_device)
    if "cleanup-quarantine" in path.parts:
        value["treeIdentitySha256"] = "f" * 64
    return value


cleanup.tree_stats = corrupt_after_quarantine
try:
    cleanup.execute_plan(repo, fresh_path, fresh_sha)
except cleanup.CleanupError:
    pass
else:
    raise SystemExit("deterministic-cleanup-contract: quarantine mismatch was accepted")
finally:
    cleanup.tree_stats = original_tree_stats
require((repo / ".genesis/build/drift.bin").is_file(), "quarantine rollback lost the source root")
require(not (repo / policy["quarantineRoot"]).exists(), "quarantine rollback left unresolved state")
controls.append("transactional-quarantine-rollback")

executed = run_cli(repo, "--execute", "--plan", fresh_path, "--confirm-sha256", fresh_sha)
require(executed.returncode == 0, f"valid dev cleanup failed: {executed.stderr}")
result = json.loads(executed.stdout, object_pairs_hook=cleanup.reject_duplicate_keys)
require(
    set(result) == {
        "deletedAllocatedBytes", "deletedLogicalBytes", "deletedRoots",
        "kind", "planSha256", "status", "version",
    },
    "execution result fields drift",
)
require(result["deletedRoots"] == [".genesis/build"], "dev cleanup deleted the wrong roots")
require(not (repo / ".genesis/build").exists(), "dev cleanup retained the selected root")
for relative in (".genesis/perf", ".genesis/dependency-mirrors", ".genesis/store", ".genesis/custom", "node_modules", "target"):
    require((repo / relative).exists() or (repo / relative).is_symlink(), f"dev cleanup deleted protected root: {relative}")
controls.append("selective-dev-execution")

evidence_plan = cleanup.render_plan(repo, "observations-clean")
evidence_path = temp / "evidence-plan.json"
evidence_sha = write_plan(evidence_path, evidence_plan)
executed = run_cli(repo, "--execute", "--plan", evidence_path, "--confirm-sha256", evidence_sha)
require(executed.returncode == 0 and not (repo / ".genesis/perf").exists(), "explicit evidence cleanup failed")
require((repo / ".genesis/dependency-mirrors").is_dir() and (repo / ".genesis/store/user.gc").is_file(), "evidence cleanup crossed class boundary")
controls.append("explicit-retained-evidence-execution")

quarantine = repo / policy["quarantineRoot"]
quarantine.mkdir(parents=True)
(quarantine / "orphan").write_text("state\n", encoding="utf-8")
rejected("unresolved-quarantine-rejection", lambda: cleanup.render_plan(repo, "dev-clean"))
shutil.rmtree(quarantine)

legacy = (source_root / "scripts/reclaim_build_space.sh").read_text(encoding="utf-8")
forbidden = ["rm " + "-rf", "cargo" + " clean", "max-age-days", "--build-root", "--aggressive"]
require(not any(value in legacy for value in forbidden), "legacy destructive cleanup behavior remains")
require("deterministic_cleanup.py" in legacy and "exec python3" in legacy, "cleanup shell is not a sealed entrypoint")
controls.append("legacy-destructive-path-retirement")

cargo_source = (source_root / "scripts/lib/cargo_cache.py").read_text(encoding="utf-8")
mirror_source = (source_root / "scripts/lib/dependency_mirror.py").read_text(encoding="utf-8")
require('".genesis/build", "cargo-cache"' in cargo_source, "Cargo producer marker is absent")
require('"dependency-mirror"' in mirror_source and "initialize_root_marker" in mirror_source, "mirror producer marker is absent")
controls.append("producer-bound-provenance")

ignore = (source_root / ".gitignore").read_text(encoding="utf-8").splitlines()
for relative in cleanup.root_classes(policy):
    top = relative.split("/", 1)[0]
    require(top in {".genesis", ".tmp", ".cargo-install-target", "node_modules", "target"}, f"cleanup root is outside ignored ownership: {relative}")
require({".genesis/refs", ".genesis/store", ".genesis/pins.toml"}.issubset(cleanup.root_classes(policy)), "user-authored roots are incomplete")
require(".genesis/" in ignore and "node_modules/" in ignore and "target/" in ignore, "ignore ownership drift")
controls.append("complete-ignored-root-ownership")

require(len(controls) == 22 and len(set(controls)) == 22, f"control coverage drift: {controls}")
authorities = [
    "policies/deterministic_cleanup_v0.1.json",
    *schema_paths,
    "scripts/lib/deterministic_cleanup.py",
    "scripts/reclaim_build_space.sh",
    "scripts/lib/cargo_cache.py",
    "scripts/lib/dependency_mirror.py",
    "scripts/check_deterministic_cleanup.sh",
]
digest = sha256()
for relative in authorities:
    path_bytes = relative.encode("utf-8")
    content = (source_root / relative).read_bytes()
    digest.update(len(path_bytes).to_bytes(8, "big"))
    digest.update(path_bytes)
    digest.update(len(content).to_bytes(8, "big"))
    digest.update(content)
print(
    "deterministic-cleanup-contract: ok "
    f"(classes=4 profiles=4 controls={len(controls)} bundle={digest.hexdigest()})"
)
PY
