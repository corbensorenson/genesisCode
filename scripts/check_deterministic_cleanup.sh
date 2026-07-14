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
import generated_state as state

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
    state.release(lifecycle, result["leaseToken"])
steady = state.status(lifecycle)
require(steady["rebuildableEntries"] <= 1 and steady["accountingBytes"] <= 8192, "profile cycles did not reach bounded steady state")
controls.append("generated-state-bounded-steady-state")

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

require(len(controls) == 41 and len(set(controls)) == 41, f"control coverage drift: {controls}")
authorities = [
    "policies/deterministic_cleanup_v0.1.json",
    "policies/generated_state_v0.1.json",
    *schema_paths,
    *generated_schema_paths,
    "scripts/lib/deterministic_cleanup.py",
    "scripts/lib/generated_state.py",
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
