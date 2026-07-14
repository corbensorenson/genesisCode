#!/usr/bin/env python3
"""Plan and execute deterministic, provenance-bound repository cleanup."""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
from typing import Any, Mapping, Sequence


POLICY_REL = "policies/deterministic_cleanup_v0.1.json"
POLICY_FIELDS = {
    "classes", "discoveryRoots", "kind", "limits", "markerFile",
    "profiles", "quarantineRoot", "version",
}
CLASS_FIELDS = {"deletable", "id", "roots"}
PROFILE_FIELDS = {"deleteClasses", "id"}
LIMIT_FIELDS = {"maxDepth", "maxEntries", "maxPlanRoots"}
MARKER_FIELDS = {"class", "kind", "path", "policySha256", "producer", "version"}
PLAN_FIELDS = {"entries", "kind", "policySha256", "profile", "summary", "version"}
ENTRY_FIELDS = {
    "action", "allocatedBytes", "class", "entries", "logicalBytes",
    "markerSha256", "path", "reason", "treeIdentitySha256",
}
SUMMARY_FIELDS = {
    "absentRoots", "deleteAllocatedBytes", "deleteLogicalBytes",
    "deleteRoots", "preserveRoots",
}
CLASS_IDS = ["dependency-mirror", "rebuildable-output", "retained-evidence", "user-authored"]
PROFILE_IDS = ["dev-clean", "generated-clean", "mirror-clean", "observations-clean"]
DELETABLE_CLASSES = set(CLASS_IDS) - {"user-authored"}
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
PRODUCER_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")


class CleanupError(ValueError):
    pass


def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise CleanupError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise CleanupError(f"cannot load JSON {path}: {exc}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def pretty_bytes(value: Any) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii")


def digest_bytes(value: bytes) -> str:
    return sha256(value).hexdigest()


def repo_path(raw: str, field: str) -> str:
    if not isinstance(raw, str):
        raise CleanupError(f"{field} must be a string")
    path = PurePosixPath(raw)
    if (
        not raw
        or path.is_absolute()
        or path.as_posix() != raw
        or ".." in path.parts
        or "." in path.parts
        or "\\" in raw
    ):
        raise CleanupError(f"{field} is not canonical repository-relative: {raw!r}")
    return path.as_posix()


def within_root(root: Path, path: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def require_safe_parent_chain(root: Path, rel: str) -> None:
    current = root.resolve()
    parts = PurePosixPath(rel).parts
    for part in parts[:-1]:
        current = current / part
        if current.is_symlink():
            raise CleanupError(f"cleanup path has a symlinked parent: {rel}")
    candidate = root.resolve() / rel
    if not candidate.is_symlink() and candidate.exists() and not within_root(root, candidate):
        raise CleanupError(f"cleanup path escapes the repository: {rel}")


def policy_file(root: Path, override: Path | None = None) -> Path:
    path = (override or root / POLICY_REL).resolve()
    if not within_root(root, path):
        raise CleanupError("cleanup policy must be inside the repository")
    return path


def load_policy(root: Path, override: Path | None = None) -> tuple[dict[str, Any], Path, str]:
    path = policy_file(root, override)
    policy = load_json(path)
    if not isinstance(policy, dict) or set(policy) != POLICY_FIELDS:
        raise CleanupError("cleanup policy fields mismatch")
    if policy["kind"] != "genesis/deterministic-cleanup-policy-v0.1" or policy["version"] != "0.1":
        raise CleanupError("cleanup policy identity mismatch")
    if policy["markerFile"] != ".genesis-clean-root-v0.1.json":
        raise CleanupError("cleanup marker filename drift")
    quarantine = repo_path(policy["quarantineRoot"], "quarantineRoot")
    if quarantine != ".genesis/cleanup-quarantine":
        raise CleanupError("cleanup quarantine root drift")
    discoveries = policy["discoveryRoots"]
    if not isinstance(discoveries, list) or discoveries != sorted(set(discoveries)) or discoveries != [".genesis"]:
        raise CleanupError("discoveryRoots must be the exact reviewed list")
    for value in discoveries:
        repo_path(value, "discovery root")
    limits = policy["limits"]
    if not isinstance(limits, dict) or set(limits) != LIMIT_FIELDS:
        raise CleanupError("cleanup limits fields mismatch")
    for field, value in limits.items():
        if not isinstance(value, int) or isinstance(value, bool) or value < 1:
            raise CleanupError(f"cleanup limit {field} must be positive")

    classes = policy["classes"]
    if not isinstance(classes, list) or len(classes) != len(CLASS_IDS):
        raise CleanupError("cleanup classes must contain exactly four entries")
    observed_ids = []
    all_roots = []
    for item in classes:
        if not isinstance(item, dict) or set(item) != CLASS_FIELDS:
            raise CleanupError("cleanup class fields mismatch")
        class_id = item["id"]
        observed_ids.append(class_id)
        expected_deletable = class_id in DELETABLE_CLASSES
        if not isinstance(item["deletable"], bool) or item["deletable"] != expected_deletable:
            raise CleanupError(f"cleanup class deletable contract drift: {class_id}")
        roots = item["roots"]
        if not isinstance(roots, list) or not roots or roots != sorted(set(roots)):
            raise CleanupError(f"cleanup roots must be sorted and unique: {class_id}")
        for raw in roots:
            all_roots.append(repo_path(raw, f"cleanup class {class_id} root"))
    if observed_ids != CLASS_IDS:
        raise CleanupError("cleanup class order/identity drift")
    if len(all_roots) != len(set(all_roots)) or len(all_roots) > limits["maxPlanRoots"]:
        raise CleanupError("cleanup roots overlap by identity or exceed policy")
    pure_roots = [PurePosixPath(value) for value in all_roots]
    for index, left in enumerate(pure_roots):
        for right in pure_roots[index + 1:]:
            if left in right.parents or right in left.parents:
                raise CleanupError(f"nested cleanup roots are forbidden: {left} and {right}")
    quarantine_path = PurePosixPath(quarantine)
    if any(root == quarantine_path or root in quarantine_path.parents or quarantine_path in root.parents for root in pure_roots):
        raise CleanupError("quarantine root must not overlap a classified root")

    profiles = policy["profiles"]
    if not isinstance(profiles, list) or len(profiles) != len(PROFILE_IDS):
        raise CleanupError("cleanup profiles must contain exactly four entries")
    observed_profiles = []
    for item in profiles:
        if not isinstance(item, dict) or set(item) != PROFILE_FIELDS:
            raise CleanupError("cleanup profile fields mismatch")
        observed_profiles.append(item["id"])
        values = item["deleteClasses"]
        if not isinstance(values, list) or not values or values != sorted(set(values)):
            raise CleanupError(f"cleanup profile classes must be sorted and unique: {item['id']}")
        if any(value not in DELETABLE_CLASSES for value in values):
            raise CleanupError(f"cleanup profile selects a non-deletable class: {item['id']}")
    if observed_profiles != PROFILE_IDS:
        raise CleanupError("cleanup profile order/identity drift")
    expected_profiles = {
        "dev-clean": ["rebuildable-output"],
        "generated-clean": ["dependency-mirror", "rebuildable-output", "retained-evidence"],
        "mirror-clean": ["dependency-mirror"],
        "observations-clean": ["retained-evidence"],
    }
    if {item["id"]: item["deleteClasses"] for item in profiles} != expected_profiles:
        raise CleanupError("cleanup profile class contract drift")
    return policy, path, digest_bytes(path.read_bytes())


def root_classes(policy: Mapping[str, Any]) -> dict[str, str]:
    return {
        path: item["id"]
        for item in policy["classes"]
        for path in item["roots"]
    }


def profile_classes(policy: Mapping[str, Any], profile: str) -> set[str]:
    by_id = {item["id"]: item["deleteClasses"] for item in policy["profiles"]}
    if profile not in by_id:
        raise CleanupError(f"unknown cleanup profile: {profile}")
    return set(by_id[profile])


def marker_document(path: str, class_id: str, policy_sha: str, producer: str) -> dict[str, str]:
    if class_id not in DELETABLE_CLASSES:
        raise CleanupError("user-authored roots cannot receive cleanup markers")
    if not PRODUCER_RE.fullmatch(producer):
        raise CleanupError(f"invalid cleanup marker producer: {producer!r}")
    return {
        "class": class_id,
        "kind": "genesis/deterministic-cleanup-root-v0.1",
        "path": path,
        "policySha256": policy_sha,
        "producer": producer,
        "version": "0.1",
    }


def validate_marker(value: Any, path: str, class_id: str, policy_sha: str) -> None:
    if not isinstance(value, dict) or set(value) != MARKER_FIELDS:
        raise CleanupError("cleanup marker fields mismatch")
    expected = marker_document(path, class_id, policy_sha, value.get("producer", ""))
    if value != expected:
        raise CleanupError("cleanup marker identity mismatch")


def atomic_write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(name, path)
    finally:
        try:
            os.unlink(name)
        except FileNotFoundError:
            pass


def tracked_paths(root: Path, rel: str) -> list[str]:
    proc = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--", rel],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise CleanupError("cleanup requires a Git worktree for tracked-file protection")
    try:
        return sorted(value for value in proc.stdout.decode("utf-8").split("\0") if value)
    except UnicodeError as exc:
        raise CleanupError("Git returned a non-UTF-8 tracked path") from exc


def initialize_root_marker(
    root: Path,
    rel: str,
    producer: str,
    policy_override: Path | None = None,
) -> dict[str, str]:
    root = root.resolve()
    policy, _, policy_sha = load_policy(root, policy_override)
    rel = repo_path(rel, "cleanup root")
    classes = root_classes(policy)
    if rel not in classes or classes[rel] not in DELETABLE_CLASSES:
        raise CleanupError(f"cleanup root is not a reviewed deletable root: {rel}")
    absolute = root / rel
    require_safe_parent_chain(root, rel)
    if absolute.is_symlink() or not absolute.is_dir():
        raise CleanupError(f"cleanup root must already be a non-symlink directory: {rel}")
    if absolute.lstat().st_dev != root.stat().st_dev:
        raise CleanupError(f"cleanup root is a filesystem boundary: {rel}")
    if tracked_paths(root, rel):
        raise CleanupError(f"cleanup root contains tracked files: {rel}")
    document = marker_document(rel, classes[rel], policy_sha, producer)
    marker = absolute / policy["markerFile"]
    if marker.exists():
        observed = load_json(marker)
        if observed == document:
            return document
        if not isinstance(observed, dict) or observed.get("path") != rel or observed.get("class") != classes[rel] or observed.get("producer") != producer:
            raise CleanupError(f"cleanup marker conflicts with reviewed producer identity: {rel}")
    atomic_write(marker, pretty_bytes(document))
    return document


def path_record_bytes(record: Mapping[str, Any]) -> bytes:
    return canonical_bytes(record)


def tree_stats(
    path: Path,
    limits: Mapping[str, int],
    expected_device: int | None = None,
) -> dict[str, Any]:
    digest = sha256()
    count = 0
    logical = 0
    allocated = 0
    stack: list[tuple[Path, str, int]] = [(path, "", 0)]
    while stack:
        current, relative, depth = stack.pop()
        if depth > limits["maxDepth"]:
            raise CleanupError("cleanup root exceeds maximum depth")
        try:
            info = current.lstat()
        except OSError as exc:
            raise CleanupError(f"cleanup root changed during inventory: {exc}") from exc
        if expected_device is not None and info.st_dev != expected_device:
            raise CleanupError("cleanup root crosses a filesystem boundary")
        count += 1
        if count > limits["maxEntries"]:
            raise CleanupError("cleanup root exceeds maximum entry count")
        allocated += max(0, int(getattr(info, "st_blocks", 0))) * 512
        mode = stat.S_IMODE(info.st_mode)
        if stat.S_ISDIR(info.st_mode):
            kind = "directory"
            size = 0
        elif stat.S_ISREG(info.st_mode):
            kind = "file"
            size = int(info.st_size)
            logical += size
        elif stat.S_ISLNK(info.st_mode):
            kind = "symlink"
            target = os.readlink(current)
            try:
                target_bytes = target.encode("utf-8")
            except UnicodeError as exc:
                raise CleanupError("cleanup symlink target is not UTF-8") from exc
            size = len(target_bytes)
            logical += size
        else:
            raise CleanupError("cleanup roots may contain only directories, files, and symlinks")
        record = {
            "kind": kind,
            "mode": mode,
            "mtimeNs": int(info.st_mtime_ns),
            "path": relative,
            "size": size,
        }
        if kind == "symlink":
            record["targetSha256"] = digest_bytes(target_bytes)
        digest.update(path_record_bytes(record))
        if kind == "directory":
            try:
                children = list(os.scandir(current))
            except OSError as exc:
                raise CleanupError(f"cannot scan cleanup root: {exc}") from exc
            names = []
            for child in children:
                try:
                    child.name.encode("utf-8")
                except UnicodeError as exc:
                    raise CleanupError("cleanup root contains a non-UTF-8 path") from exc
                names.append(child.name)
            for name in sorted(names, reverse=True):
                child_rel = name if not relative else f"{relative}/{name}"
                stack.append((current / name, child_rel, depth + 1))
    return {
        "allocatedBytes": allocated,
        "entries": count,
        "logicalBytes": logical,
        "treeIdentitySha256": digest.hexdigest(),
    }


def marker_state(
    absolute: Path,
    rel: str,
    class_id: str,
    policy: Mapping[str, Any],
    policy_sha: str,
) -> tuple[str, str | None]:
    marker = absolute / policy["markerFile"]
    if not marker.exists():
        return "missing-marker", None
    if marker.is_symlink() or not marker.is_file():
        return "invalid-marker", None
    try:
        content = marker.read_bytes()
        value = json.loads(content.decode("utf-8"), object_pairs_hook=reject_duplicate_keys)
        validate_marker(value, rel, class_id, policy_sha)
    except (OSError, UnicodeError, json.JSONDecodeError, CleanupError):
        return "invalid-marker", None
    return "valid", digest_bytes(content)


def unknown_roots(root: Path, policy: Mapping[str, Any]) -> list[str]:
    known = set(root_classes(policy))
    quarantine = policy["quarantineRoot"]
    unknown = []
    for discovery in policy["discoveryRoots"]:
        absolute = root / discovery
        if not absolute.exists():
            continue
        if absolute.is_symlink() or not absolute.is_dir():
            unknown.append(discovery)
            continue
        covered_names = {
            PurePosixPath(value).relative_to(PurePosixPath(discovery)).parts[0]
            for value in known
            if PurePosixPath(discovery) in PurePosixPath(value).parents
        }
        quarantine_name = PurePosixPath(quarantine).relative_to(PurePosixPath(discovery)).parts[0]
        for child in os.scandir(absolute):
            child.name.encode("utf-8")
            if child.name in covered_names:
                continue
            if child.name == quarantine_name:
                quarantine_path = Path(child.path)
                if quarantine_path.is_symlink() or any(quarantine_path.iterdir()):
                    raise CleanupError("cleanup quarantine contains unresolved state")
                continue
            unknown.append(f"{discovery}/{child.name}")
    return sorted(set(unknown))


def render_plan(
    root: Path,
    profile: str,
    policy_override: Path | None = None,
) -> dict[str, Any]:
    root = root.resolve()
    policy, _, policy_sha = load_policy(root, policy_override)
    repository_device = root.stat().st_dev
    selected = profile_classes(policy, profile)
    classes = root_classes(policy)
    entries = []
    for rel, class_id in sorted(classes.items()):
        absolute = root / rel
        require_safe_parent_chain(root, rel)
        if not absolute.exists() and not absolute.is_symlink():
            entries.append({
                "action": "absent", "allocatedBytes": 0, "class": class_id,
                "entries": 0, "logicalBytes": 0, "markerSha256": None,
                "path": rel, "reason": "missing", "treeIdentitySha256": None,
            })
            continue
        stats = tree_stats(absolute, policy["limits"], repository_device)
        marker_sha = None
        if class_id == "user-authored":
            action, reason = "preserve", "policy-user-authored"
        elif class_id not in selected:
            action, reason = "preserve", "class-not-selected"
        elif absolute.is_symlink():
            action, reason = "preserve", "symlink-root"
        elif tracked_paths(root, rel):
            action, reason = "preserve", "tracked-content"
        else:
            marker_status, marker_sha = marker_state(absolute, rel, class_id, policy, policy_sha)
            if marker_status == "valid":
                action, reason = "delete", "eligible"
            else:
                action, reason = "preserve", marker_status
        entries.append({
            "action": action,
            "allocatedBytes": stats["allocatedBytes"],
            "class": class_id,
            "entries": stats["entries"],
            "logicalBytes": stats["logicalBytes"],
            "markerSha256": marker_sha,
            "path": rel,
            "reason": reason,
            "treeIdentitySha256": stats["treeIdentitySha256"],
        })
    for rel in unknown_roots(root, policy):
        absolute = root / rel
        stats = tree_stats(absolute, policy["limits"], repository_device)
        entries.append({
            "action": "preserve",
            "allocatedBytes": stats["allocatedBytes"],
            "class": "user-authored",
            "entries": stats["entries"],
            "logicalBytes": stats["logicalBytes"],
            "markerSha256": None,
            "path": repo_path(rel, "unknown cleanup root"),
            "reason": "unknown-untracked-root",
            "treeIdentitySha256": stats["treeIdentitySha256"],
        })
    entries.sort(key=lambda item: item["path"])
    if len(entries) > policy["limits"]["maxPlanRoots"]:
        raise CleanupError("cleanup plan exceeds maximum root count")
    summary = {
        "absentRoots": sum(item["action"] == "absent" for item in entries),
        "deleteAllocatedBytes": sum(item["allocatedBytes"] for item in entries if item["action"] == "delete"),
        "deleteLogicalBytes": sum(item["logicalBytes"] for item in entries if item["action"] == "delete"),
        "deleteRoots": sum(item["action"] == "delete" for item in entries),
        "preserveRoots": sum(item["action"] == "preserve" for item in entries),
    }
    return {
        "entries": entries,
        "kind": "genesis/deterministic-cleanup-plan-v0.1",
        "policySha256": policy_sha,
        "profile": profile,
        "summary": summary,
        "version": "0.1",
    }


def validate_plan_shape(plan: Any) -> None:
    if not isinstance(plan, dict) or set(plan) != PLAN_FIELDS:
        raise CleanupError("cleanup plan fields mismatch")
    if plan["kind"] != "genesis/deterministic-cleanup-plan-v0.1" or plan["version"] != "0.1":
        raise CleanupError("cleanup plan identity mismatch")
    if plan["profile"] not in PROFILE_IDS or not SHA_RE.fullmatch(str(plan["policySha256"])):
        raise CleanupError("cleanup plan profile or policy identity is invalid")
    entries = plan["entries"]
    if not isinstance(entries, list):
        raise CleanupError("cleanup plan entries must be an array")
    paths = []
    for item in entries:
        if not isinstance(item, dict) or set(item) != ENTRY_FIELDS:
            raise CleanupError("cleanup plan entry fields mismatch")
        paths.append(repo_path(item["path"], "cleanup plan path"))
        if item["class"] not in CLASS_IDS or item["action"] not in {"absent", "delete", "preserve"}:
            raise CleanupError("cleanup plan entry class or action is invalid")
        for field in ("allocatedBytes", "entries", "logicalBytes"):
            if not isinstance(item[field], int) or isinstance(item[field], bool) or item[field] < 0:
                raise CleanupError(f"cleanup plan entry {field} is invalid")
        for field in ("markerSha256", "treeIdentitySha256"):
            value = item[field]
            if value is not None and (not isinstance(value, str) or not SHA_RE.fullmatch(value)):
                raise CleanupError(f"cleanup plan entry {field} is invalid")
    if paths != sorted(set(paths)):
        raise CleanupError("cleanup plan paths must be sorted and unique")
    summary = plan["summary"]
    if not isinstance(summary, dict) or set(summary) != SUMMARY_FIELDS:
        raise CleanupError("cleanup plan summary fields mismatch")
    for value in summary.values():
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise CleanupError("cleanup plan summary values must be non-negative integers")


def remove_tree(path: Path) -> None:
    if not shutil.rmtree.avoids_symlink_attacks:
        raise CleanupError("platform lacks symlink-safe recursive removal")
    def repair_and_retry(function, failed, _exc):
        os.chmod(failed, stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
        function(failed)

    shutil.rmtree(path, onerror=repair_and_retry)


def execute_plan(
    root: Path,
    plan_path: Path,
    confirm_sha: str,
    policy_override: Path | None = None,
) -> dict[str, Any]:
    root = root.resolve()
    if within_root(root, plan_path):
        raise CleanupError("cleanup execution plan must be outside the repository")
    plan = load_json(plan_path)
    validate_plan_shape(plan)
    plan_sha = digest_bytes(canonical_bytes(plan))
    if not SHA_RE.fullmatch(confirm_sha) or confirm_sha != plan_sha:
        raise CleanupError("cleanup confirmation SHA-256 does not match the canonical plan")
    policy, _, policy_sha = load_policy(root, policy_override)
    if plan["policySha256"] != policy_sha:
        raise CleanupError("cleanup plan policy identity is stale")
    current = render_plan(root, plan["profile"], policy_override)
    if current != plan:
        raise CleanupError("cleanup plan is stale; rerun dry-run and review the new plan")
    selected = [item for item in plan["entries"] if item["action"] == "delete"]
    classes = root_classes(policy)
    for item in selected:
        if item["path"] not in classes or classes[item["path"]] != item["class"] or item["class"] not in DELETABLE_CLASSES:
            raise CleanupError("cleanup plan attempts to delete an undeclared or protected root")
        if tracked_paths(root, item["path"]):
            raise CleanupError("cleanup root gained tracked content")

    quarantine_root = root / policy["quarantineRoot"]
    require_safe_parent_chain(root, policy["quarantineRoot"])
    if quarantine_root.is_symlink():
        raise CleanupError("cleanup quarantine root must not be a symlink")
    if quarantine_root.exists() and any(quarantine_root.iterdir()):
        raise CleanupError("cleanup quarantine contains unresolved state")
    blocked = {signal.SIGINT, signal.SIGTERM}
    if hasattr(signal, "SIGHUP"):
        blocked.add(signal.SIGHUP)
    previous_mask = None
    if hasattr(signal, "pthread_sigmask"):
        previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, blocked)
    batch = quarantine_root / plan_sha
    renamed: list[tuple[Path, Path, Mapping[str, Any]]] = []
    try:
        batch.mkdir(parents=True, exist_ok=False)
        for index, item in enumerate(selected):
            source = root / item["path"]
            destination = batch / f"{index:04d}"
            os.rename(source, destination)
            renamed.append((source, destination, item))
        for _, destination, item in renamed:
            stats = tree_stats(destination, policy["limits"], root.stat().st_dev)
            if stats["treeIdentitySha256"] != item["treeIdentitySha256"]:
                raise CleanupError("cleanup root changed during quarantine")
            marker_status, marker_sha = marker_state(
                destination, item["path"], item["class"], policy, policy_sha
            )
            if marker_status != "valid" or marker_sha != item["markerSha256"]:
                raise CleanupError("cleanup marker changed during quarantine")
    except Exception:
        for source, destination, _ in reversed(renamed):
            if destination.exists() and not source.exists():
                os.rename(destination, source)
        shutil.rmtree(batch, ignore_errors=True)
        try:
            quarantine_root.rmdir()
        except OSError:
            pass
        raise
    else:
        deleted = []
        for _, destination, item in renamed:
            remove_tree(destination)
            deleted.append(item["path"])
        batch.rmdir()
        try:
            quarantine_root.rmdir()
        except OSError:
            pass
    finally:
        if previous_mask is not None:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
    return {
        "deletedAllocatedBytes": plan["summary"]["deleteAllocatedBytes"],
        "deletedLogicalBytes": plan["summary"]["deleteLogicalBytes"],
        "deletedRoots": deleted,
        "kind": "genesis/deterministic-cleanup-result-v0.1",
        "planSha256": plan_sha,
        "status": "executed",
        "version": "0.1",
    }


def safe_output_path(root: Path, output: Path, policy: Mapping[str, Any]) -> Path:
    resolved = output.expanduser().resolve()
    if within_root(root, resolved):
        raise CleanupError("cleanup plan output must be outside the repository")
    return resolved


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--policy", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true")
    mode.add_argument("--execute", action="store_true")
    mode.add_argument("--initialize-root")
    parser.add_argument("--profile", choices=PROFILE_IDS, default="dev-clean")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--plan", type=Path)
    parser.add_argument("--confirm-sha256", default="")
    parser.add_argument("--producer", default="explicit-initialize")
    args = parser.parse_args(argv)
    try:
        root = args.root.resolve()
        policy, _, _ = load_policy(root, args.policy)
        if args.initialize_root is not None:
            if args.out or args.plan or args.confirm_sha256 or args.execute or args.dry_run:
                raise CleanupError("initialize-root cannot be combined with planning/execution options")
            document = initialize_root_marker(root, args.initialize_root, args.producer, args.policy)
            print(json.dumps(document, sort_keys=True, separators=(",", ":")))
            return 0
        if args.execute:
            if args.out or not args.plan or not args.confirm_sha256:
                raise CleanupError("execute requires --plan and --confirm-sha256 and forbids --out")
            result = execute_plan(root, args.plan, args.confirm_sha256, args.policy)
            print(json.dumps(result, sort_keys=True, separators=(",", ":")))
            return 0
        if args.plan or args.confirm_sha256:
            raise CleanupError("dry-run accepts --profile and optional --out only")
        plan = render_plan(root, args.profile, args.policy)
        rendered = pretty_bytes(plan)
        plan_sha = digest_bytes(canonical_bytes(plan))
        if args.out:
            destination = safe_output_path(root, args.out, policy)
            atomic_write(destination, rendered)
            print(f"deterministic-cleanup: dry-run profile={args.profile} plan_sha256={plan_sha} output=written")
        else:
            sys.stdout.buffer.write(rendered)
            print(f"deterministic-cleanup: plan-sha256={plan_sha}", file=sys.stderr)
        return 0
    except (CleanupError, OSError, UnicodeError) as exc:
        print(f"deterministic-cleanup: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
