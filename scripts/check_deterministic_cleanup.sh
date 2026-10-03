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
from unittest.mock import patch

source_root = Path(sys.argv[1]).resolve()
temp = Path(sys.argv[2]).resolve()
sys.path.insert(0, str(source_root / "scripts/lib"))
import deterministic_cleanup as cleanup
import generated_state as state
from generated_state_accounting import accounting_self_test
from cargo_metadata_admission import metadata_self_test
from allocated_resources_controls import allocation_self_test

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


def state_rejected(name, function, diagnostic=None):
    try:
        function()
    except state.GeneratedStateError as exc:
        require(diagnostic is None or diagnostic in str(exc), f"wrong {name} diagnostic: {exc}")
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

transient_tree = temp / "transient-metadata-tree"
transient_tree.mkdir()
(transient_tree / "payload.bin").write_bytes(b"payload")
original_rmtree = cleanup.shutil.rmtree
transient_attempts = 0


def transient_rmtree(path, *args, **kwargs):
    global transient_attempts
    transient_attempts += 1
    if transient_attempts == 1:
        raise OSError(66, "Directory not empty", str(path))
    return original_rmtree(path, *args, **kwargs)


transient_rmtree.avoids_symlink_attacks = original_rmtree.avoids_symlink_attacks
cleanup.shutil.rmtree = transient_rmtree
try:
    cleanup.remove_tree(transient_tree)
finally:
    cleanup.shutil.rmtree = original_rmtree
require(transient_attempts == 2 and not transient_tree.exists(), "transient metadata removal did not recover")
controls.append("transient-metadata-recreation-recovery")

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

# Generated-state lifecycle: every producer is declared, quota accounting is bounded,
# leases serialize cleanup, and interrupted reclamation recovers deterministically.
state_policy, _, _ = state.load_policy(source_root)
generated_schema_paths = [
    "docs/spec/GENERATED_STATE_POLICY_v0.1.schema.json",
    "docs/spec/GENERATED_STATE_REGISTRY_v0.1.schema.json",
    "docs/spec/GENERATED_STATE_REGISTRY_v0.2.schema.json",
]
for relative in generated_schema_paths:
    schema = cleanup.load_json(source_root / relative)
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
        and schema.get("additionalProperties") is False,
        f"generated-state schema is not closed: {relative}",
    )
declared_state_roots = {
    relative for producer in state_policy["producers"] for relative in producer["roots"]
}
require(
    {
        relative
        for relative, class_id in cleanup.root_classes(policy).items()
        if class_id in cleanup.DELETABLE_CLASSES
    }.issubset(declared_state_roots),
    "generated-state policy does not cover every cleanup root",
)
require(
    [producer["owner"] for producer in state_policy["producers"]]
    == sorted({producer["owner"] for producer in state_policy["producers"]}),
    "generated-state producers are not a closed sorted registry",
)
controls.append("generated-state-closed-authority")

lifecycle = temp / "lifecycle"
(lifecycle / "policies").mkdir(parents=True)
shutil.copyfile(source_root / cleanup.POLICY_REL, lifecycle / cleanup.POLICY_REL)
bounded_policy = copy.deepcopy(state_policy)
bounded_policy["limits"].update({
    "softBytes": 8192,
    "hardBytes": 16384,
    "minFreeBytes": 4096,
    "maxEntries": 32,
    "maxLeases": 8,
})
reservations = {
    "cargo-host": 8192,
    "cargo-host-slim": 4096,
    "cargo-verifier": 4096,
    "cargo-wasm": 8192,
    "node-install": 8192,
    "observed": 0,
    "protected": 0,
    "selfhost-cache": 4096,
    "temporary": 4096,
}
for size_class in bounded_policy["sizeClasses"]:
    size_class["reservationBytes"] = reservations[size_class["id"]]
(lifecycle / state.POLICY_REL).write_bytes(state.pretty_bytes(bounded_policy))
(lifecycle / ".gitignore").write_text(".genesis/\n.tmp/\n.cargo-install-target/\nnode_modules/\ntarget/\n", encoding="utf-8")
(lifecycle / "source.gc").write_text("fixture\n", encoding="utf-8")
subprocess.run(["git", "init", "-q"], cwd=lifecycle, check=True)
subprocess.run(["git", "add", ".gitignore", "source.gc"], cwd=lifecycle, check=True)
(lifecycle / ".genesis/build/legacy").mkdir(parents=True)
(lifecycle / ".genesis/build/legacy/payload.bin").write_bytes(b"legacy")
cleanup.initialize_root_marker(lifecycle, ".genesis/build", "lifecycle-fixture")

alpha_path = ".genesis/build/cargo-cache/v1/root/host/alpha"
alpha = state.admit(
    lifecycle, "cargo-cache", "a" * 64, alpha_path, "cargo-host",
    free_bytes_override=1 << 30,
)
require(not (lifecycle / ".genesis/build/legacy").exists(), "legacy build island was not reclaimed first")
controls.append("generated-state-legacy-reclamation")

beta = state.admit(
    lifecycle, "cargo-cache", "b" * 64,
    ".genesis/build/cargo-cache/v1/root/host/beta", "cargo-host",
    free_bytes_override=1 << 30,
)
state_rejected(
    "generated-state-hard-quota-denial",
    lambda: state.admit(
        lifecycle, "cargo-cache", "c" * 64,
        ".genesis/build/cargo-cache/v1/root/host/gamma", "cargo-host",
        free_bytes_override=1 << 30,
    ),
    "hard quota admission denied",
)
state.release(lifecycle, alpha["leaseToken"])
gamma = state.admit(
    lifecycle, "cargo-cache", "c" * 64,
    ".genesis/build/cargo-cache/v1/root/host/gamma", "cargo-host",
    free_bytes_override=1 << 30,
)
require(alpha["entryId"] in gamma["reclaimedEntryIds"], "least-recent inactive entry was not reclaimed")
controls.append("generated-state-lru-active-protection")

(lifecycle / ".genesis/dependency-mirrors/sha256-fixture").mkdir(parents=True)
protected = state.register_protected(
    lifecycle, "dependency-mirror", "d" * 64,
    ".genesis/dependency-mirrors/sha256-fixture",
)
require(protected["protected"], "dependency mirror was not registered as protected")
state.release(lifecycle, beta["leaseToken"])
state.release(lifecycle, gamma["leaseToken"])
state_rejected(
    "generated-state-low-disk-denial",
    lambda: state.admit(
        lifecycle, "cargo-cache", "e" * 64,
        ".genesis/build/cargo-cache/v1/root/host/low-disk", "cargo-host",
        free_bytes_override=0,
    ),
    "low-disk admission denied",
)
require(state.status(lifecycle)["protectedEntries"] == 1, "protected state entered quota accounting")
controls.append("generated-state-protected-retention")

stale = state.admit(
    lifecycle, "cargo-cache", "f" * 64,
    ".genesis/build/cargo-cache/v1/root/host/stale", "cargo-host",
    identity_fn=lambda _pid: "f" * 64,
    free_bytes_override=1 << 30,
)
recovered = state.status(lifecycle)
require(recovered["recoveredStaleLeases"] == 1 and recovered["activeLeases"] == 0, "stale lease was not recovered")
controls.append("generated-state-stale-lease-recovery")

crash_path = ".genesis/build/cargo-cache/v1/root/host/crash"
(lifecycle / crash_path).mkdir(parents=True)
(lifecycle / crash_path / "payload.bin").write_bytes(b"crash")
crash = state.admit(
    lifecycle, "cargo-cache", "1" * 64, crash_path, "cargo-host",
    free_bytes_override=1 << 30,
)
state.release(lifecycle, crash["leaseToken"])
loaded_policy, _, loaded_sha = state.load_policy(lifecycle)
with state.state_lock(lifecycle, loaded_policy) as state_root:
    require(state_root is not None, "generated-state registry disappeared")
    registry = state._load_registry(state_root, loaded_policy, loaded_sha)
    entry = next(item for item in registry["entries"] if item["id"] == crash["entryId"])
    registry["sequence"] += 1
    transaction_id = state._transaction_id(entry, registry["sequence"])
    quarantine_rel = f"{loaded_policy['stateRoot']}/quarantine/{transaction_id}"
    quarantine_path = lifecycle / quarantine_rel
    quarantine_path.parent.mkdir(parents=True, exist_ok=True)
    os.replace(lifecycle / crash_path, quarantine_path)
    registry["transaction"] = {
        "entryId": entry["id"],
        "id": transaction_id,
        "phase": "quarantined",
        "quarantinePath": quarantine_rel,
        "sourcePath": crash_path,
    }
    state._write_registry(state_root, loaded_policy, registry)
post_crash = state.status(lifecycle)
require(not quarantine_path.exists() and post_crash["entryCount"] >= 1, "quarantined transaction did not recover")
controls.append("generated-state-crash-recovery")

# Selfhost materializations are files, whereas Cargo materializations are trees.
# Exercise both normal reclamation and recovery after the journaled rename.
(lifecycle / ".genesis/cache/selfhost_toolchain").mkdir(parents=True)
cleanup.initialize_root_marker(lifecycle, ".genesis/cache", "fixture-cache")
for index, crash_after_rename in enumerate([False, True]):
    key = str(index + 5) * 64
    relative = f".genesis/cache/selfhost_toolchain/{key}.gc"
    (lifecycle / relative).write_bytes(b"rebuildable artifact")
    cached = state.admit(lifecycle, "selfhost-cache", key, relative, "selfhost-cache",
                         free_bytes_override=1 << 30)
    state.release(lifecycle, cached["leaseToken"])
    with state.state_lock(lifecycle, loaded_policy) as state_root:
        registry = state._load_registry(state_root, loaded_policy, loaded_sha)
        entry = next(item for item in registry["entries"] if item["id"] == cached["entryId"])
        if crash_after_rename:
            registry["sequence"] += 1
            transaction_id = state._transaction_id(entry, registry["sequence"])
            quarantined = f"{loaded_policy['stateRoot']}/quarantine/{transaction_id}"
            os.replace(lifecycle / relative, lifecycle / quarantined)
            registry["transaction"] = {"entryId": entry["id"], "id": transaction_id,
                "phase": "quarantined", "sourcePath": relative, "quarantinePath": quarantined}
            state._write_registry(state_root, loaded_policy, registry)
        else:
            state._reclaim_entry(lifecycle, state_root, loaded_policy, registry, entry)
    state.status(lifecycle)
    final = state._load_registry(state_root, loaded_policy, loaded_sha)
    require(not (lifecycle / relative).exists() and final["transaction"] is None
            and all(item["id"] != cached["entryId"] for item in final["entries"]),
            "file materialization reclamation/recovery left an unfinished transaction")
    controls.append("generated-state-file-crash-recovery" if crash_after_rename
                    else "generated-state-file-reclamation")

quarantine_link = temp / "quarantine-link"
quarantine_target = temp / "quarantine-target"
quarantine_target.write_bytes(b"protected target")
quarantine_link.symlink_to(quarantine_target)
state_rejected("generated-state-quarantine-link-rejection",
    lambda: state._remove_tree(quarantine_link), "not a regular file or directory")
require(quarantine_target.read_bytes() == b"protected target" and quarantine_link.is_symlink(),
        "quarantine link rejection changed the target or entry")

concurrent_path = ".genesis/build/cargo-cache/v1/root/host/concurrent"
command = [
    sys.executable, str(source_root / "scripts/lib/generated_state.py"),
    "--root", str(lifecycle), "acquire", "--owner", "cargo-cache",
    "--content-key", "2" * 64, "--path", concurrent_path,
    "--size-class", "cargo-host", "--pid", str(os.getpid()),
]
processes = [subprocess.Popen(command, cwd=lifecycle, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE) for _ in range(8)]
tokens = []
for process in processes:
    stdout, stderr = process.communicate(timeout=30)
    require(process.returncode == 0, f"concurrent admission failed: {stderr}")
    tokens.append(stdout.strip())
require(len(set(tokens)) == 8 and state.status(lifecycle)["activeLeases"] == 8, "concurrent leases were lost")
state_rejected(
    "generated-state-lease-bound-rejection",
    lambda: state.admit(
        lifecycle, "cargo-cache", "2" * 64, concurrent_path, "cargo-host",
        free_bytes_override=1 << 30,
    ),
    "lease bound exceeded",
)
try:
    with state.cleanup_guard(lifecycle, [".genesis/build"]):
        pass
except state.GeneratedStateError as exc:
    require("active generated-state lease" in str(exc), f"wrong cleanup race diagnostic: {exc}")
    controls.append("generated-state-cleanup-lease-race")
else:
    raise SystemExit("deterministic-cleanup-contract: active lease did not block cleanup")
for token in tokens:
    state.release(lifecycle, token)
require(state.status(lifecycle)["activeLeases"] == 0, "concurrent leases did not release")
controls.append("generated-state-concurrent-admission")

for index in range(20):
    target_family = "wasm32-wasip1" if index % 2 else "host"
    size_class = "cargo-wasm" if index % 2 else "cargo-host"
    result = state.admit(
        lifecycle, "cargo-cache", f"{index + 100:064x}",
        f".genesis/build/cargo-cache/v1/root/{target_family}/cycle-{index}", size_class,
        free_bytes_override=1 << 30,
    )
    cycle_path = lifecycle / f".genesis/build/cargo-cache/v1/root/{target_family}/cycle-{index}"
    cycle_path.mkdir(parents=True, exist_ok=True)
    (cycle_path / "payload").write_bytes(b"x" * 1024)
    state.release(lifecycle, result["leaseToken"])
steady = state.status(lifecycle)
require(steady["rebuildableEntries"] <= 1 and steady["accountingBytes"] <= 8192, "profile cycles did not reach bounded steady state")
controls.append("generated-state-bounded-steady-state")

# Preserve priority pressure across filesystems with different directory
# allocation. The second case is a finite backend model, not host qualification.
physical_allocation = state.allocated_bytes
for directory_minimum in (0, 4096):
    def allocation_with_directory_minimum(path, max_entries=2_000_000):
        total = physical_allocation(path, max_entries)
        if directory_minimum and path.exists():
            for item in [path, *path.rglob("*")]:
                if item.is_dir():
                    metadata = item.stat()
                    allocated = metadata.st_blocks * 512
                    total += max(0, directory_minimum - allocated)
        return total

    with patch.object(state, "allocated_bytes", side_effect=allocation_with_directory_minimum):
        priority = temp / f"priority-{directory_minimum}"
        (priority / "policies").mkdir(parents=True)
        shutil.copyfile(source_root / cleanup.POLICY_REL, priority / cleanup.POLICY_REL)
        priority_policy = copy.deepcopy(bounded_policy)
        priority_policy["limits"]["softBytes"] = 14335
        (priority / state.POLICY_REL).write_bytes(state.pretty_bytes(priority_policy))
        (priority / ".gitignore").write_text(".genesis/\n", encoding="utf-8")
        (priority / "source.gc").write_text("fixture\n", encoding="utf-8")
        subprocess.run(["git", "init", "-q"], cwd=priority, check=True)
        subprocess.run(["git", "add", ".gitignore", "source.gc"], cwd=priority, check=True)
        (priority / ".genesis/build").mkdir(parents=True)
        cleanup.initialize_root_marker(priority, ".genesis/build", "priority-fixture")
        host = state.admit(
            priority, "cargo-cache", "6" * 64,
            ".genesis/build/cargo-cache/v1/root/host/normal", "cargo-host",
            free_bytes_override=1 << 30,
        )
        host_path = priority / ".genesis/build/cargo-cache/v1/root/host/normal"
        host_path.mkdir(parents=True, exist_ok=True)
        (host_path / "payload").write_bytes(b"h" * max(0, 8192 - state.allocated_bytes(host_path)))
        state.release(priority, host["leaseToken"])
        slim = state.admit(
            priority, "cargo-cache", "7" * 64,
            ".genesis/build/cargo-cache/v1/root/host/slim", "cargo-host-slim",
            free_bytes_override=1 << 30,
        )
        slim_path = priority / ".genesis/build/cargo-cache/v1/root/host/slim"
        slim_path.mkdir(parents=True, exist_ok=True)
        (slim_path / "payload").write_bytes(b"s" * max(0, 4096 - state.allocated_bytes(slim_path)))
        state.release(priority, slim["leaseToken"])
        verifier_reservation = next(item["reservationBytes"] for item in priority_policy["sizeClasses"] if item["id"] == "cargo-verifier")
        require(state.status(priority)["accountingBytes"] + verifier_reservation > priority_policy["limits"]["softBytes"],
                "reclaim-priority fixture does not exercise allocation pressure")
        verifier = state.admit(
            priority, "cargo-cache", "8" * 64,
            ".genesis/build/cargo-cache/v1/tools-genesis-evidence-verifier/host/verifier",
            "cargo-verifier", free_bytes_override=1 << 30,
        )
        require(
            slim["entryId"] in verifier["reclaimedEntryIds"]
            and host["entryId"] not in verifier["reclaimedEntryIds"],
            "size-class reclaim priority did not preserve the warm host cache",
        )
        state.release(priority, verifier["leaseToken"])
controls.append("generated-state-size-class-reclaim-priority")

# Admission costs are measured from the actual journal and all live writers.
# Use a separate finite fixture so the low-space controls cannot reclaim history.
cost_root = temp / "operation-cost"
(cost_root / "policies").mkdir(parents=True)
shutil.copyfile(source_root / cleanup.POLICY_REL, cost_root / cleanup.POLICY_REL)
cost_policy = copy.deepcopy(bounded_policy)
cost_policy["limits"].update(softBytes=262144, hardBytes=262144, minFreeBytes=0)
for item in cost_policy["sizeClasses"]:
    if item["id"] == "cargo-host":
        item["reservationBytes"] = 65536
(cost_root / state.POLICY_REL).write_bytes(state.pretty_bytes(cost_policy))
(cost_root / ".gitignore").write_text(".genesis/\n", encoding="utf-8")
(cost_root / "source.gc").write_text("fixture\n", encoding="utf-8")
subprocess.run(["git", "init", "-q"], cwd=cost_root, check=True)
subprocess.run(["git", "add", ".gitignore", "source.gc"], cwd=cost_root, check=True)
(cost_root / ".genesis/build").mkdir(parents=True)
cleanup.initialize_root_marker(cost_root, ".genesis/build", "cost-fixture")
cost_a_path = ".genesis/build/cargo-cache/v1/root/host/cost-a"
cost_b_path = ".genesis/build/cargo-cache/v1/root/host/cost-b"
cost_a = state.admit(cost_root, "cargo-cache", "a" * 64, cost_a_path,
                     "cargo-host", free_bytes_override=1 << 30)
cost_loaded, _, cost_sha = state.load_policy(cost_root)
with state.state_lock(cost_root, cost_loaded) as cost_state:
    cost_registry = state._load_registry(cost_state, cost_loaded, cost_sha)
journal_cost = state._journal_growth_bytes(cost_root, cost_registry)
# Enough for the new writer alone; insufficient for both outstanding writers.
state_rejected(
    "generated-state-combined-pending-growth-denial",
    lambda: state.admit(cost_root, "cargo-cache", "b" * 64, cost_b_path,
                       "cargo-host", free_bytes_override=journal_cost + 65536 + 16384),
    "low-disk admission denied",
)
(cost_root / cost_a_path).mkdir(parents=True)
(cost_root / cost_a_path / "payload.bin").write_bytes(b"a" * 65536)
cost_b = state.admit(cost_root, "cargo-cache", "b" * 64, cost_b_path,
                     "cargo-host", free_bytes_override=journal_cost + 65536 + 16384)
require(cost_b["pendingGrowthBytes"] == 65536,
        "live target allocation was not refreshed before admission")
controls.append("generated-state-live-allocation-refresh")
shared_b = state.admit(cost_root, "cargo-cache", "b" * 64, cost_b_path,
                      "cargo-host", free_bytes_override=journal_cost + 65536 + 16384)
require(shared_b["pendingGrowthBytes"] == 65536,
        "leases on the same materialization double-counted its growth")
state.release(cost_root, shared_b["leaseToken"])
state.release(cost_root, cost_a["leaseToken"])
controls.append("generated-state-shared-target-growth-deduplication")
warm_bytes = (cost_root / cost_a_path / "payload.bin").read_bytes()
warm_a = state.admit(cost_root, "cargo-cache", "a" * 64, cost_a_path,
                    "cargo-host", free_bytes_override=journal_cost + 65536 + 16384)
require(not warm_a["reclaimedEntryIds"] and
        (cost_root / cost_a_path / "payload.bin").read_bytes() == warm_bytes,
        "a fitting warm target was deleted to satisfy an unrelated floor")
state.release(cost_root, warm_a["leaseToken"])
controls.append("generated-state-warm-cache-low-space-preservation")
with state.state_lock(cost_root, cost_loaded) as cost_state:
    probe_registry = state._load_registry(cost_state, cost_loaded, cost_sha)
# A retained clone/open handle can make allocated bytes disappear without making
# those bytes available to this writer. Model that filesystem observation.
original_reclaim = state._reclaim_entry
original_free = state._free_bytes
probe_journal = state._journal_growth_bytes(cost_root, probe_registry)
probe_free = probe_journal + 16384
reclaim_calls = []
def reclaim_without_physical_recovery(root, state_root, policy, registry, entry):
    reclaim_calls.append(entry["id"])
    registry["entries"] = [e for e in registry["entries"] if e["id"] != entry["id"]]
    return 2 * 1024 * 1024
state._reclaim_entry = reclaim_without_physical_recovery
state._free_bytes = lambda _root: probe_free
try:
    state_rejected(
        "generated-state-reclaim-requires-physical-space-recovery",
        lambda: state._enforce_limits(cost_root, cost_state, cost_loaded,
            probe_registry, cost_b["entryId"], 65536, cost_loaded["limits"], probe_free),
        "low-disk admission denied",
    )
    require(len(reclaim_calls) == 1, "physical-space control did not exercise reclamation")
finally:
    state._reclaim_entry = original_reclaim
    state._free_bytes = original_free
with state.state_lock(cost_root, cost_loaded) as cost_state:
    recovered_probe = state._load_registry(cost_state, cost_loaded, cost_sha)
reclaim_calls.clear()
state._reclaim_entry = reclaim_without_physical_recovery
try:
    recovered_ids = state._enforce_limits(cost_root, cost_state, cost_loaded,
        recovered_probe, cost_b["entryId"], 65536, cost_loaded["limits"],
        probe_free, free_bytes_fn=lambda: 262144)
    require(len(reclaim_calls) == 1 and recovered_ids == reclaim_calls,
            "genuine physical-space recovery did not admit the fitting writer")
    controls.append("generated-state-physical-space-recovery-admission")
finally:
    state._reclaim_entry = original_reclaim
# Escaped Unicode paths can be larger than longer ASCII paths in the journal.
# Check the estimate against every possible serialized recovery record.
journal_probe = copy.deepcopy(probe_registry)
ascii_entry = state._entry(cost_loaded, "cargo-cache", "c" * 64,
    ".genesis/build/cargo-cache/v1/" + "/".join(["a" * 200] * 10), "cargo-host", 1)
unicode_entry = state._entry(cost_loaded, "cargo-cache", "d" * 64,
    ".genesis/build/cargo-cache/v1/" + "/".join(["😀" * 50] * 18), "cargo-host", 1)
# Use the declared cache namespace, without materializing these bounded fixtures.
journal_probe["entries"] = [ascii_entry, unicode_entry]
block = state._allocation_unit_bytes(cost_root)
for source_entry in journal_probe["entries"]:
    possible = copy.deepcopy(journal_probe)
    possible["sequence"] += 2
    possible["leases"].append({"entryId": "0" * 64, "id": "0" * 32,
                              "pid": 2**63 - 1, "processIdentity": "0" * 64, "operation": "cargo-metadata", "growthBytes": 2**63 - 1})
    possible["transaction"] = {"entryId": "0" * 64, "id": "0" * 64,
        "phase": "quarantined", "sourcePath": source_entry["path"],
        "quarantinePath": ".genesis/build/.generated-state-v0.1/quarantine/" + "0" * 64}
    serialized_bytes = len(state.pretty_bytes(possible))
    required_journal = 2 * ((serialized_bytes + block - 1) // block) * block + 4 * block
    require(state._journal_growth_bytes(cost_root, journal_probe) >= required_journal,
            "journal estimate missed the largest escaped recovery path")
controls.append("generated-state-escaped-journal-size-accounting")
with patch.object(state.os, "statvfs", None):
    state_rejected("generated-state-unsupported-space-backend-rejection",
                   lambda: state._journal_growth_bytes(cost_root, journal_probe),
                   "physical-space backend unsupported")
with patch.object(state.os, "statvfs", lambda _root: type("Stats", (), {
        "f_frsize": 0, "f_bavail": 100,
})()):
    state_rejected("generated-state-invalid-space-unit-rejection",
                   lambda: state._free_bytes(cost_root), "invalid units")

state.release(cost_root, cost_b["leaseToken"])
state_rejected(
    "generated-state-recovery-journal-space-denial",
    lambda: state.admit(cost_root, "cargo-cache", "a" * 64, cost_a_path,
                       "cargo-host", free_bytes_override=1),
    "recovery journal cannot fit",
)
# Read-only work does not acquire a writer reservation or impose a universal floor.
mock_bin = temp / "disk-observation-bin"
mock_bin.mkdir()
mock_df = mock_bin / "df"
mock_df.write_text("#!/bin/sh\nprintf 'Filesystem 1024-blocks Used Available Capacity Mounted\nfixture 100 99 1 99%% /\n'\n", encoding="utf-8")
mock_df.chmod(0o700)
observation_env = dict(os.environ, PATH=str(mock_bin) + os.pathsep + os.environ["PATH"],
                       GENESIS_GATE_TELEMETRY_DISABLE="1", CI="true")
observation_env.pop("GENESIS_MIN_FREE_KB", None)
observation_cmd = ["bash", str(source_root / "scripts/check_disk_headroom.sh"),
                   "--path", str(cost_root), "--strict", "1"]
observed = subprocess.run(observation_cmd, env=observation_env, capture_output=True, text=True)
require(observed.returncode == 0 and "required_kb=0" in observed.stdout,
        "read-only observation was blocked by a blanket floor")
explicit = subprocess.run([*observation_cmd, "--min-kb", "2"], env=observation_env,
                          capture_output=True, text=True)
require(explicit.returncode == 2, "explicit operation requirement was ignored")
controls.append("generated-state-read-only-low-space-observation")

state_rejected(
    "generated-state-unknown-owner-rejection",
    lambda: state.admit(lifecycle, "unknown", "3" * 64, ".genesis/build/unknown", "cargo-host"),
    "undeclared generated-state producer",
)
state_rejected(
    "generated-state-ceiling-override-rejection",
    lambda: state.admit(
        lifecycle, "cargo-cache", "4" * 64,
        ".genesis/build/cargo-cache/v1/root/host/override", "cargo-host",
        environ={"GENESIS_GENERATED_STATE_HARD_BYTES": "16385"},
    ),
    "cannot exceed policy",
)

unbounded_policy = copy.deepcopy(bounded_policy)
unbounded_policy["limits"]["maxEntries"] = state.MAX_POLICY_ENTRIES + 1
unbounded_path = temp / "unbounded-generated-state-policy.json"
unbounded_path.write_bytes(state.pretty_bytes(unbounded_policy))
state_rejected(
    "generated-state-cardinality-policy-rejection",
    lambda: state.load_policy(lifecycle, unbounded_path),
    "cardinality is unbounded",
)

registry_path = lifecycle / bounded_policy["stateRoot"] / bounded_policy["registryFile"]
valid_registry = registry_path.read_bytes()
registry_path.write_text('{"kind":"a","kind":"b"}\n', encoding="utf-8")
state_rejected(
    "generated-state-duplicate-registry-rejection",
    lambda: state.status(lifecycle),
    "duplicate JSON key",
)
registry_path.write_bytes(valid_registry)

oversized_json = temp / "oversized-generated-state.json"
oversized_json.write_bytes(b" " * (state.MAX_JSON_BYTES + 1))
state_rejected(
    "generated-state-oversized-json-rejection",
    lambda: state.load_json(oversized_json),
    "JSON input exceeds",
)

unauthorized = temp / "unauthorized-state"
(unauthorized / "policies").mkdir(parents=True)
shutil.copyfile(source_root / cleanup.POLICY_REL, unauthorized / cleanup.POLICY_REL)
shutil.copyfile(lifecycle / state.POLICY_REL, unauthorized / state.POLICY_REL)
(unauthorized / ".gitignore").write_text(".genesis/\n", encoding="utf-8")
(unauthorized / "source.gc").write_text("fixture\n", encoding="utf-8")
subprocess.run(["git", "init", "-q"], cwd=unauthorized, check=True)
subprocess.run(["git", "add", ".gitignore", "source.gc"], cwd=unauthorized, check=True)
(unauthorized / ".genesis/build").mkdir(parents=True)
state_rejected(
    "generated-state-marker-authority-rejection",
    lambda: state.admit(
        unauthorized, "cargo-cache", "5" * 64,
        ".genesis/build/cargo-cache/v1/root/host/unmarked", "cargo-host",
    ),
    "cleanup authority marker",
)

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

require(accounting_self_test(source_root) == 11, "idle allocation control coverage drift")
controls.append("generated-state-idle-allocation-accounting")
require(metadata_self_test(source_root) == 23, "metadata operation control coverage drift")
controls.append("cargo-metadata-operation-admission")
require(allocation_self_test() == 16, "allocated-block control coverage drift")
controls.append("shared-allocated-block-observation")

require(len(controls) == 59 and len(set(controls)) == 59, f"control coverage drift: {controls}")
authorities = [
    "policies/deterministic_cleanup_v0.1.json",
    "policies/generated_state_v0.1.json",
    *schema_paths,
    *generated_schema_paths,
    "scripts/lib/deterministic_cleanup.py",
    "scripts/lib/generated_state.py",
    "scripts/lib/allocated_resources.py",
    "scripts/lib/allocated_resources_controls.py",
    "scripts/lib/supervisor_cancellation.py",
    "scripts/lib/gate_telemetry_darwin_inventory.py",
    "scripts/lib/generated_state_accounting.py",
    "scripts/lib/cargo_metadata_admission.py",
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
