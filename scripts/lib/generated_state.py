#!/usr/bin/env python3
"""Concurrency-safe admission, leasing, and reclamation for generated state."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import errno
from hashlib import sha256
import json
import os
from pathlib import Path, PurePosixPath
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any, Callable, Iterator, Mapping, MutableMapping, Sequence


POLICY_REL = "policies/generated_state_v0.1.json"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
TOKEN_RE = re.compile(r"^[0-9a-f]{32}$")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
POLICY_FIELDS = {
    "kind", "version", "stateRoot", "registryFile", "limits", "sizeClasses", "producers",
}
LIMIT_FIELDS = {
    "softBytes", "hardBytes", "minFreeBytes", "maxEntries", "maxLeases", "lockTimeoutMs",
}
PRODUCER_FIELDS = {
    "owner", "roots", "contentKey", "sizeClasses", "retentionClass", "leaseMode", "reclaimOrder",
}
ENTRY_FIELDS = {
    "id", "owner", "path", "contentKey", "sizeClass", "retentionClass", "reclaimOrder",
    "reservationBytes", "observedAllocatedBytes", "lastUseSequence",
}
LEASE_FIELDS = {"id", "entryId", "pid", "processIdentity"}
TRANSACTION_FIELDS = {"id", "entryId", "sourcePath", "quarantinePath", "phase"}
REGISTRY_FIELDS = {
    "kind", "version", "policySha256", "sequence", "entries", "leases", "transaction",
}
PROTECTED_RETENTION = {"dependency-mirror", "retained-evidence", "rollback-quarantine"}
MAX_JSON_BYTES = 8 * 1024 * 1024
MAX_POLICY_ENTRIES = 4096
MAX_POLICY_LEASES = 16384


class GeneratedStateError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise GeneratedStateError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        if path.stat().st_size > MAX_JSON_BYTES:
            raise GeneratedStateError(f"JSON input exceeds {MAX_JSON_BYTES} bytes: {path}")
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys)
    except FileNotFoundError as exc:
        raise GeneratedStateError(f"missing file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise GeneratedStateError(
            f"invalid JSON in {path}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n"
    ).encode("ascii")


def pretty_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
    ).encode("ascii")


def digest_bytes(value: bytes) -> str:
    return sha256(value).hexdigest()


def repo_path(raw: str, field: str) -> str:
    if not isinstance(raw, str) or not raw or "\\" in raw:
        raise GeneratedStateError(f"{field} must be a canonical repository-relative path")
    path = PurePosixPath(raw)
    if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
        raise GeneratedStateError(f"{field} must be a canonical repository-relative path")
    if path.as_posix() != raw:
        raise GeneratedStateError(f"{field} must be a canonical repository-relative path")
    return raw


def _positive_int(value: Any, field: str, allow_zero: bool = False) -> int:
    minimum = 0 if allow_zero else 1
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise GeneratedStateError(f"{field} must be an integer >= {minimum}")
    return value


def load_policy(root: Path, override: Path | None = None) -> tuple[dict[str, Any], Path, str]:
    path = (override or (root / POLICY_REL)).resolve()
    policy = load_json(path)
    if not isinstance(policy, dict) or set(policy) != POLICY_FIELDS:
        raise GeneratedStateError("generated-state policy fields mismatch")
    if policy["kind"] != "genesis/generated-state-policy-v0.1" or policy["version"] != "0.1":
        raise GeneratedStateError("generated-state policy identity mismatch")
    if policy["stateRoot"] != ".genesis/build/.generated-state-v0.1":
        raise GeneratedStateError("generated-state root drift")
    if policy["registryFile"] != "registry.json":
        raise GeneratedStateError("generated-state registry filename drift")
    limits = policy["limits"]
    if not isinstance(limits, dict) or set(limits) != LIMIT_FIELDS:
        raise GeneratedStateError("generated-state limit fields mismatch")
    for field in LIMIT_FIELDS:
        _positive_int(limits[field], f"limits.{field}", allow_zero=field == "minFreeBytes")
    if limits["softBytes"] > limits["hardBytes"] or limits["hardBytes"] > 8 * 1024**3:
        raise GeneratedStateError("generated-state soft/hard quota contract drift")
    if limits["maxEntries"] > MAX_POLICY_ENTRIES or limits["maxLeases"] > MAX_POLICY_LEASES:
        raise GeneratedStateError("generated-state registry cardinality is unbounded")
    if limits["lockTimeoutMs"] > 300_000:
        raise GeneratedStateError("generated-state lock timeout is unbounded")

    size_classes = policy["sizeClasses"]
    if not isinstance(size_classes, list) or not size_classes or len(size_classes) > 64:
        raise GeneratedStateError("generated-state size classes must be non-empty")
    size_ids: list[str] = []
    for item in size_classes:
        if not isinstance(item, dict) or set(item) != {"id", "reservationBytes"}:
            raise GeneratedStateError("generated-state size-class fields mismatch")
        if not isinstance(item["id"], str) or not ID_RE.fullmatch(item["id"]):
            raise GeneratedStateError("invalid generated-state size-class id")
        reservation = _positive_int(
            item["reservationBytes"], "sizeClass.reservationBytes", allow_zero=True
        )
        if reservation > limits["hardBytes"]:
            raise GeneratedStateError("size-class reservation exceeds hard quota")
        size_ids.append(item["id"])
    if size_ids != sorted(set(size_ids)):
        raise GeneratedStateError("generated-state size classes must be sorted and unique")

    producers = policy["producers"]
    if not isinstance(producers, list) or not producers or len(producers) > 64:
        raise GeneratedStateError("generated-state producers must be non-empty")
    owners: list[str] = []
    declared_roots: list[str] = []
    for item in producers:
        if not isinstance(item, dict) or set(item) != PRODUCER_FIELDS:
            raise GeneratedStateError("generated-state producer fields mismatch")
        owner = item["owner"]
        if not isinstance(owner, str) or not ID_RE.fullmatch(owner):
            raise GeneratedStateError("invalid generated-state producer owner")
        roots = item["roots"]
        if not isinstance(roots, list) or not roots:
            raise GeneratedStateError(f"producer {owner} roots must be non-empty")
        normalized_roots = [repo_path(raw, f"producer {owner} root") for raw in roots]
        if normalized_roots != sorted(set(normalized_roots)):
            raise GeneratedStateError(f"producer {owner} roots must be sorted and unique")
        classes = item["sizeClasses"]
        if (
            not isinstance(classes, list)
            or not classes
            or classes != sorted(set(classes))
            or any(value not in size_ids for value in classes)
        ):
            raise GeneratedStateError(f"producer {owner} size-class contract drift")
        if item["retentionClass"] not in {
            "rebuildable-output", "dependency-mirror", "retained-evidence", "rollback-quarantine"
        }:
            raise GeneratedStateError(f"producer {owner} retention class is invalid")
        expected_mode = (
            "protected" if item["retentionClass"] in PROTECTED_RETENTION else "process"
        )
        if item["leaseMode"] != expected_mode:
            raise GeneratedStateError(f"producer {owner} lease mode is unsafe")
        if not isinstance(item["contentKey"], str) or not ID_RE.fullmatch(item["contentKey"]):
            raise GeneratedStateError(f"producer {owner} content-key strategy is invalid")
        reclaim_order = _positive_int(
            item["reclaimOrder"], f"producer {owner} reclaimOrder", allow_zero=True
        )
        if reclaim_order > 1000:
            raise GeneratedStateError(f"producer {owner} reclaim order is invalid")
        owners.append(owner)
        declared_roots.extend(normalized_roots)
    if owners != sorted(set(owners)):
        raise GeneratedStateError("generated-state producer owners must be sorted and unique")
    required_roots = {
        ".cargo-install-target", ".genesis/build", ".genesis/cache",
        ".genesis/cleanup-quarantine", ".genesis/dependency-mirrors", ".genesis/logs",
        ".genesis/perf", ".genesis/selfhost", ".genesis/tmp", ".tmp", "node_modules", "target",
    }
    if not required_roots.issubset(declared_roots):
        raise GeneratedStateError("generated-state producer declarations do not cover cleanup roots")
    return policy, path, digest_bytes(path.read_bytes())


def _safe_absolute(root: Path, rel: str, allow_absent: bool = True) -> Path:
    rel = repo_path(rel, "generated-state path")
    current = root.resolve()
    for part in PurePosixPath(rel).parts:
        current = current / part
        if current.is_symlink():
            raise GeneratedStateError(f"generated-state path contains a symlink: {rel}")
        if not current.exists():
            if allow_absent:
                break
            raise GeneratedStateError(f"generated-state path is absent: {rel}")
    absolute = root.resolve() / rel
    try:
        absolute.relative_to(root.resolve())
    except ValueError as exc:  # pragma: no cover - guarded by canonical rel paths
        raise GeneratedStateError("generated-state path escaped repository") from exc
    return absolute


def _atomic_write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def _lock_file(handle: Any, timeout_ms: int) -> None:
    deadline = time.monotonic() + timeout_ms / 1000
    while True:
        try:
            if os.name == "nt":  # pragma: no cover - exercised by Windows CI
                import msvcrt

                handle.seek(0)
                if handle.read(1) == b"":
                    handle.write(b"0")
                    handle.flush()
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            return
        except (BlockingIOError, OSError) as exc:
            if isinstance(exc, OSError) and exc.errno not in (
                errno.EACCES, errno.EAGAIN, errno.EDEADLK,
            ):
                raise
            if time.monotonic() >= deadline:
                raise GeneratedStateError("generated-state lock acquisition timed out") from exc
            time.sleep(0.01)


def _unlock_file(handle: Any) -> None:
    if os.name == "nt":  # pragma: no cover - exercised by Windows CI
        import msvcrt

        handle.seek(0)
        msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
    else:
        import fcntl

        fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def _control_lock_path(root: Path) -> Path:
    try:
        proc = subprocess.run(
            ["git", "rev-parse", "--git-path", "genesis-generated-state-v0.1.lock"],
            cwd=root,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise GeneratedStateError(f"cannot resolve generated-state control lock: {exc}") from exc
    if proc.returncode != 0 or not proc.stdout.strip():
        raise GeneratedStateError("generated-state lifecycle requires a Git worktree")
    path = Path(proc.stdout.strip())
    return path if path.is_absolute() else (root / path).resolve()


@contextmanager
def state_lock(root: Path, policy: Mapping[str, Any], create: bool = True) -> Iterator[Path | None]:
    state_root = _safe_absolute(root, str(policy["stateRoot"]))
    lock_path = _control_lock_path(root)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    with lock_path.open("a+b") as handle:
        _lock_file(handle, int(policy["limits"]["lockTimeoutMs"]))
        try:
            if not state_root.exists() and not create:
                yield None
                return
            if create:
                _require_cleanup_authority(root, str(policy["stateRoot"]))
                state_root.mkdir(parents=True, exist_ok=True)
            yield state_root
        finally:
            _unlock_file(handle)


def _new_registry(policy_sha: str) -> dict[str, Any]:
    return {
        "entries": [],
        "kind": "genesis/generated-state-registry-v0.1",
        "leases": [],
        "policySha256": policy_sha,
        "sequence": 0,
        "transaction": None,
        "version": "0.1",
    }


def _validate_registry(registry: Any, policy: Mapping[str, Any], policy_sha: str) -> None:
    if not isinstance(registry, dict) or set(registry) != REGISTRY_FIELDS:
        raise GeneratedStateError("generated-state registry fields mismatch")
    if (
        registry["kind"] != "genesis/generated-state-registry-v0.1"
        or registry["version"] != "0.1"
        or registry["policySha256"] != policy_sha
    ):
        raise GeneratedStateError("generated-state registry identity mismatch")
    _positive_int(registry["sequence"], "registry.sequence", allow_zero=True)
    entries = registry["entries"]
    leases = registry["leases"]
    if not isinstance(entries, list) or len(entries) > policy["limits"]["maxEntries"]:
        raise GeneratedStateError("generated-state entry bound exceeded")
    if not isinstance(leases, list) or len(leases) > policy["limits"]["maxLeases"]:
        raise GeneratedStateError("generated-state lease bound exceeded")
    entry_ids: list[str] = []
    entry_paths: list[str] = []
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != ENTRY_FIELDS:
            raise GeneratedStateError("generated-state entry fields mismatch")
        if not isinstance(entry["id"], str) or not SHA_RE.fullmatch(entry["id"]):
            raise GeneratedStateError("generated-state entry id is invalid")
        repo_path(entry["path"], "registry entry path")
        for field in (
            "reclaimOrder", "reservationBytes", "observedAllocatedBytes", "lastUseSequence"
        ):
            _positive_int(entry[field], f"entry.{field}", allow_zero=True)
        expected = _entry_id(entry["owner"], entry["contentKey"], entry["path"])
        if entry["id"] != expected:
            raise GeneratedStateError("generated-state entry identity mismatch")
        entry_ids.append(entry["id"])
        entry_paths.append(entry["path"])
    if entry_ids != sorted(set(entry_ids)) or len(entry_paths) != len(set(entry_paths)):
        raise GeneratedStateError("generated-state entries must be sorted with unique paths")
    known_entries = set(entry_ids)
    lease_ids: list[str] = []
    for lease in leases:
        if not isinstance(lease, dict) or set(lease) != LEASE_FIELDS:
            raise GeneratedStateError("generated-state lease fields mismatch")
        if not isinstance(lease["id"], str) or not TOKEN_RE.fullmatch(lease["id"]):
            raise GeneratedStateError("generated-state lease id is invalid")
        if lease["entryId"] not in known_entries:
            raise GeneratedStateError("generated-state lease references an unknown entry")
        _positive_int(lease["pid"], "lease.pid")
        if not isinstance(lease["processIdentity"], str) or not SHA_RE.fullmatch(
            lease["processIdentity"]
        ):
            raise GeneratedStateError("generated-state process identity is invalid")
        lease_ids.append(lease["id"])
    if lease_ids != sorted(set(lease_ids)):
        raise GeneratedStateError("generated-state leases must be sorted and unique")
    transaction = registry["transaction"]
    if transaction is not None:
        if not isinstance(transaction, dict) or set(transaction) != TRANSACTION_FIELDS:
            raise GeneratedStateError("generated-state transaction fields mismatch")
        if transaction["entryId"] not in known_entries:
            raise GeneratedStateError("generated-state transaction references an unknown entry")
        if transaction["phase"] not in ("planned", "quarantined"):
            raise GeneratedStateError("generated-state transaction phase is invalid")
        for field in ("id", "entryId"):
            if not isinstance(transaction[field], str) or not SHA_RE.fullmatch(transaction[field]):
                raise GeneratedStateError("generated-state transaction identity is invalid")
        repo_path(transaction["sourcePath"], "transaction source path")
        repo_path(transaction["quarantinePath"], "transaction quarantine path")


def _load_registry(state_root: Path, policy: Mapping[str, Any], policy_sha: str) -> dict[str, Any]:
    path = state_root / str(policy["registryFile"])
    registry = _new_registry(policy_sha) if not path.exists() else load_json(path)
    _validate_registry(registry, policy, policy_sha)
    return registry


def _write_registry(state_root: Path, policy: Mapping[str, Any], registry: Mapping[str, Any]) -> None:
    _atomic_write(state_root / str(policy["registryFile"]), pretty_bytes(registry))


def _entry_id(owner: str, content_key: str, path: str) -> str:
    return digest_bytes(f"generated-state-entry-v0.1\0{owner}\0{content_key}\0{path}".encode())


def _transaction_id(entry: Mapping[str, Any], sequence: int) -> str:
    return digest_bytes(
        f"generated-state-reclaim-v0.1\0{entry['id']}\0{sequence}".encode()
    )


def _producer(policy: Mapping[str, Any], owner: str) -> Mapping[str, Any]:
    for item in policy["producers"]:
        if item["owner"] == owner:
            return item
    raise GeneratedStateError(f"undeclared generated-state producer: {owner}")


def _size_reservation(policy: Mapping[str, Any], size_class: str) -> int:
    for item in policy["sizeClasses"]:
        if item["id"] == size_class:
            return int(item["reservationBytes"])
    raise GeneratedStateError(f"undeclared generated-state size class: {size_class}")


def _path_matches(path: str, roots: Sequence[str]) -> bool:
    candidate = PurePosixPath(path)
    return any(candidate == PurePosixPath(root) or PurePosixPath(root) in candidate.parents for root in roots)


def _entry(
    policy: Mapping[str, Any], owner: str, content_key: str, path: str, size_class: str,
    sequence: int, observed_bytes: int = 0,
) -> dict[str, Any]:
    producer = _producer(policy, owner)
    path = repo_path(path, "generated-state entry path")
    if not _path_matches(path, producer["roots"]):
        raise GeneratedStateError(f"producer {owner} does not own path {path}")
    if size_class not in producer["sizeClasses"]:
        raise GeneratedStateError(f"producer {owner} cannot use size class {size_class}")
    if not isinstance(content_key, str) or not content_key or len(content_key) > 256:
        raise GeneratedStateError("generated-state content key is invalid")
    return {
        "contentKey": content_key,
        "id": _entry_id(owner, content_key, path),
        "lastUseSequence": sequence,
        "observedAllocatedBytes": observed_bytes,
        "owner": owner,
        "path": path,
        "reclaimOrder": producer["reclaimOrder"],
        "reservationBytes": _size_reservation(policy, size_class),
        "retentionClass": producer["retentionClass"],
        "sizeClass": size_class,
    }


def allocated_bytes(path: Path, max_entries: int = 2_000_000) -> int:
    if not path.exists():
        return 0
    total = 0
    seen = 0
    stack = [path]
    while stack:
        current = stack.pop()
        stat = current.lstat()
        if current.is_symlink():
            raise GeneratedStateError("generated-state materialization contains a symlink")
        seen += 1
        if seen > max_entries:
            raise GeneratedStateError("generated-state materialization entry bound exceeded")
        total += int(getattr(stat, "st_blocks", 0)) * 512 or int(stat.st_size)
        if current.is_dir():
            with os.scandir(current) as iterator:
                stack.extend(Path(item.path) for item in iterator)
    return total


def process_identity(pid: int) -> str | None:
    if pid <= 0:
        return None
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return None
    except PermissionError:
        pass
    payload: bytes
    proc_stat = Path(f"/proc/{pid}/stat")
    if proc_stat.is_file():
        try:
            fields = proc_stat.read_text(encoding="utf-8").split()
            boot = Path("/proc/sys/kernel/random/boot_id").read_text(encoding="ascii").strip()
            payload = f"linux\0{pid}\0{fields[21]}\0{boot}".encode()
        except (OSError, IndexError):
            return None
    else:
        try:
            result = subprocess.run(
                ["ps", "-o", "lstart=", "-p", str(pid)],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                timeout=5,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return None
        started = result.stdout.strip()
        if result.returncode != 0 or not started:
            return None
        payload = f"posix\0{pid}\0{started}".encode()
    return digest_bytes(payload)


def _entry_by_id(registry: Mapping[str, Any], entry_id: str) -> MutableMapping[str, Any] | None:
    return next((entry for entry in registry["entries"] if entry["id"] == entry_id), None)


def _entry_by_path(registry: Mapping[str, Any], path: str) -> MutableMapping[str, Any] | None:
    return next((entry for entry in registry["entries"] if entry["path"] == path), None)


def _sort_registry(registry: MutableMapping[str, Any]) -> None:
    registry["entries"].sort(key=lambda item: item["id"])
    registry["leases"].sort(key=lambda item: item["id"])


def _remove_tree(path: Path) -> None:
    if not shutil.rmtree.avoids_symlink_attacks:
        raise GeneratedStateError("platform lacks symlink-safe recursive removal")

    def retry(function: Callable[..., Any], failed: str, _error: BaseException) -> None:
        os.chmod(failed, 0o700)
        function(failed)

    # Indexers can recreate metadata between scandir and rmdir. Retry the whole
    # bounded operation; a continuously mutating producer still fails closed.
    for attempt in range(8):
        try:
            shutil.rmtree(path, onerror=retry)
            return
        except OSError:
            if not path.exists():
                return
            if attempt == 7:
                raise
            time.sleep(0.025 * (attempt + 1))


def _require_cleanup_authority(root: Path, path: str) -> None:
    import deterministic_cleanup

    policy, _, policy_sha = deterministic_cleanup.load_policy(root)
    candidate = PurePosixPath(path)
    classified = [
        (relative, class_id)
        for relative, class_id in deterministic_cleanup.root_classes(policy).items()
        if candidate == PurePosixPath(relative) or PurePosixPath(relative) in candidate.parents
    ]
    if not classified:
        raise GeneratedStateError("generated-state path has no cleanup authority")
    relative, class_id = max(classified, key=lambda item: len(PurePosixPath(item[0]).parts))
    if class_id != "rebuildable-output":
        raise GeneratedStateError("generated-state reclamation crossed a retention boundary")
    status, _ = deterministic_cleanup.marker_state(
        root / relative, relative, class_id, policy, policy_sha
    )
    if status != "valid":
        raise GeneratedStateError("generated-state cleanup authority marker is absent or invalid")


def _recover_transaction(
    root: Path, state_root: Path, policy: Mapping[str, Any], registry: MutableMapping[str, Any]
) -> bool:
    transaction = registry["transaction"]
    if transaction is None:
        return False
    source = _safe_absolute(root, transaction["sourcePath"])
    quarantine = _safe_absolute(root, transaction["quarantinePath"])
    if source.exists() and quarantine.exists():
        raise GeneratedStateError("generated-state recovery found source and quarantine")
    if quarantine.exists():
        _remove_tree(quarantine)
        registry["entries"] = [
            entry for entry in registry["entries"] if entry["id"] != transaction["entryId"]
        ]
        registry["leases"] = [
            lease for lease in registry["leases"] if lease["entryId"] != transaction["entryId"]
        ]
    elif not source.exists():
        registry["entries"] = [
            entry for entry in registry["entries"] if entry["id"] != transaction["entryId"]
        ]
        registry["leases"] = [
            lease for lease in registry["leases"] if lease["entryId"] != transaction["entryId"]
        ]
    registry["transaction"] = None
    _sort_registry(registry)
    _write_registry(state_root, policy, registry)
    return True


def _recover_leases(
    root: Path,
    registry: MutableMapping[str, Any],
    identity_fn: Callable[[int], str | None],
) -> int:
    retained = []
    stale_entry_ids: set[str] = set()
    for lease in registry["leases"]:
        if identity_fn(lease["pid"]) == lease["processIdentity"]:
            retained.append(lease)
        else:
            stale_entry_ids.add(lease["entryId"])
    removed = len(registry["leases"]) - len(retained)
    registry["leases"] = retained
    for entry_id in stale_entry_ids:
        entry = _entry_by_id(registry, entry_id)
        if entry is not None:
            entry["observedAllocatedBytes"] = allocated_bytes(
                _safe_absolute(root, entry["path"])
            )
    return removed


def _register_entry(
    registry: MutableMapping[str, Any], candidate: Mapping[str, Any]
) -> MutableMapping[str, Any]:
    existing = _entry_by_path(registry, candidate["path"])
    if existing is not None:
        if existing["id"] != candidate["id"]:
            raise GeneratedStateError("generated-state path ownership or content key changed")
        return existing
    if any(entry["id"] == candidate["id"] for entry in registry["entries"]):
        raise GeneratedStateError("generated-state entry identity collision")
    registry["entries"].append(dict(candidate))
    _sort_registry(registry)
    return _entry_by_id(registry, candidate["id"])  # type: ignore[return-value]


def _cargo_size_class(scope: str) -> str:
    if scope == "evidence-verifier-host":
        return "cargo-verifier"
    if scope in ("root-wasi", "root-wasm"):
        return "cargo-wasm"
    return "cargo-host"


def _discover_legacy_build_entries(
    root: Path, policy: Mapping[str, Any], registry: MutableMapping[str, Any]
) -> None:
    build = root / ".genesis/build"
    if not build.is_dir() or build.is_symlink():
        return
    registry["sequence"] += 1
    sequence = registry["sequence"]
    for child in sorted(build.iterdir(), key=lambda item: item.name):
        if child.name in (".generated-state-v0.1", ".genesis-clean-root-v0.1.json"):
            continue
        if child.name == "cargo-cache":
            cache_root = child / "v1"
            if not cache_root.is_dir() or cache_root.is_symlink():
                continue
            for metadata in sorted(cache_root.glob("*/*/*/.genesis-cargo-cache-key.json")):
                target = metadata.parent
                relative = target.relative_to(root).as_posix()
                if _entry_by_path(registry, relative) is not None:
                    continue
                try:
                    document = load_json(metadata)
                    key = document["cacheKeySha256"]
                    scope = document["cacheKey"]["scope"]
                except (GeneratedStateError, KeyError, TypeError):
                    raise GeneratedStateError("Cargo cache metadata cannot be registered safely")
                if not isinstance(key, str) or not SHA_RE.fullmatch(key):
                    raise GeneratedStateError("Cargo cache content key is invalid")
                candidate = _entry(
                    policy, "cargo-cache", key, relative, _cargo_size_class(scope), sequence,
                    allocated_bytes(target),
                )
                _register_entry(registry, candidate)
            continue
        if not child.is_dir() or child.is_symlink():
            continue
        relative = child.relative_to(root).as_posix()
        if _entry_by_path(registry, relative) is not None:
            continue
        content_key = digest_bytes(f"legacy-build-island-v0.1\0{relative}".encode())
        candidate = _entry(
            policy, "legacy-build-island", content_key, relative, "observed", sequence,
            allocated_bytes(child),
        )
        _register_entry(registry, candidate)


def _accounting_bytes(entry: Mapping[str, Any]) -> int:
    return max(int(entry["reservationBytes"]), int(entry["observedAllocatedBytes"]))


def _active_entry_ids(registry: Mapping[str, Any]) -> set[str]:
    return {lease["entryId"] for lease in registry["leases"]}


def _reclaim_entry(
    root: Path,
    state_root: Path,
    policy: Mapping[str, Any],
    registry: MutableMapping[str, Any],
    entry: Mapping[str, Any],
) -> int:
    if entry["retentionClass"] != "rebuildable-output":
        raise GeneratedStateError("protected generated state cannot be reclaimed")
    if entry["id"] in _active_entry_ids(registry):
        raise GeneratedStateError("active generated state cannot be reclaimed")
    _require_cleanup_authority(root, entry["path"])
    source = _safe_absolute(root, entry["path"])
    if not source.exists():
        observed = 0
        registry["entries"] = [item for item in registry["entries"] if item["id"] != entry["id"]]
        _sort_registry(registry)
        _write_registry(state_root, policy, registry)
        return observed
    observed = allocated_bytes(source)
    registry["sequence"] += 1
    transaction_id = _transaction_id(entry, registry["sequence"])
    quarantine_rel = f"{policy['stateRoot']}/quarantine/{transaction_id}"
    quarantine = _safe_absolute(root, quarantine_rel)
    quarantine.parent.mkdir(parents=True, exist_ok=True)
    if quarantine.exists():
        raise GeneratedStateError("generated-state quarantine collision")
    registry["transaction"] = {
        "entryId": entry["id"],
        "id": transaction_id,
        "phase": "planned",
        "quarantinePath": quarantine_rel,
        "sourcePath": entry["path"],
    }
    _write_registry(state_root, policy, registry)
    os.replace(source, quarantine)
    registry["transaction"]["phase"] = "quarantined"
    _write_registry(state_root, policy, registry)
    _remove_tree(quarantine)
    registry["entries"] = [item for item in registry["entries"] if item["id"] != entry["id"]]
    registry["leases"] = [lease for lease in registry["leases"] if lease["entryId"] != entry["id"]]
    registry["transaction"] = None
    _sort_registry(registry)
    _write_registry(state_root, policy, registry)
    return observed


def _effective_limits(policy: Mapping[str, Any], environ: Mapping[str, str]) -> dict[str, int]:
    limits = {key: int(value) for key, value in policy["limits"].items()}
    mappings = {
        "GENESIS_GENERATED_STATE_SOFT_BYTES": "softBytes",
        "GENESIS_GENERATED_STATE_HARD_BYTES": "hardBytes",
        "GENESIS_GENERATED_STATE_MIN_FREE_BYTES": "minFreeBytes",
    }
    for variable, field in mappings.items():
        raw = environ.get(variable)
        if raw is None:
            continue
        if not raw.isdigit():
            raise GeneratedStateError(f"{variable} must be a non-negative integer")
        limits[field] = int(raw)
    if limits["hardBytes"] > policy["limits"]["hardBytes"]:
        raise GeneratedStateError("generated-state hard quota cannot exceed policy GB-5 ceiling")
    if limits["softBytes"] <= 0 or limits["softBytes"] > limits["hardBytes"]:
        raise GeneratedStateError("generated-state effective soft quota is invalid")
    return limits


def _free_bytes(root: Path) -> int:
    stats = os.statvfs(root)
    return int(stats.f_bavail) * int(stats.f_frsize)


def _enforce_limits(
    root: Path,
    state_root: Path,
    policy: Mapping[str, Any],
    registry: MutableMapping[str, Any],
    protected_entry_id: str | None,
    needed_growth: int,
    limits: Mapping[str, int],
    free_bytes: int,
) -> list[str]:
    reclaimed: list[str] = []

    def accounting() -> int:
        return sum(
            _accounting_bytes(entry)
            for entry in registry["entries"]
            if entry["retentionClass"] == "rebuildable-output"
        )

    def candidates() -> list[MutableMapping[str, Any]]:
        active = _active_entry_ids(registry)
        return sorted(
            (
                entry
                for entry in registry["entries"]
                if entry["retentionClass"] == "rebuildable-output"
                and entry["id"] not in active
                and entry["id"] != protected_entry_id
            ),
            key=lambda item: (item["reclaimOrder"], item["lastUseSequence"], item["id"]),
        )

    projected_free = free_bytes
    while (
        accounting() > limits["softBytes"]
        or accounting() > limits["hardBytes"]
        or projected_free < limits["minFreeBytes"] + needed_growth
    ):
        available = candidates()
        if not available:
            break
        selected = available[0]
        freed = _reclaim_entry(root, state_root, policy, registry, selected)
        projected_free += freed
        reclaimed.append(selected["id"])
    if accounting() > limits["hardBytes"]:
        raise GeneratedStateError("generated-state hard quota admission denied")
    if projected_free < limits["minFreeBytes"] + needed_growth:
        raise GeneratedStateError("generated-state low-disk admission denied")
    return reclaimed


def admit(
    root: Path,
    owner: str,
    content_key: str,
    path: str,
    size_class: str,
    pid: int | None = None,
    policy_path: Path | None = None,
    environ: Mapping[str, str] | None = None,
    identity_fn: Callable[[int], str | None] = process_identity,
    free_bytes_override: int | None = None,
) -> dict[str, Any]:
    root = root.resolve()
    policy, _, policy_sha = load_policy(root, policy_path)
    env = dict(os.environ if environ is None else environ)
    limits = _effective_limits(policy, env)
    lease_pid = pid or os.getppid()
    identity = identity_fn(lease_pid)
    if identity is None:
        raise GeneratedStateError("cannot bind generated-state lease to process identity")
    producer = _producer(policy, owner)
    if producer["leaseMode"] != "process":
        raise GeneratedStateError("protected generated state does not use admission leases")
    requested_path = _safe_absolute(root, path)
    with state_lock(root, policy) as state_root_value:
        assert state_root_value is not None
        state_root = state_root_value
        registry = _load_registry(state_root, policy, policy_sha)
        _recover_transaction(root, state_root, policy, registry)
        _recover_leases(root, registry, identity_fn)
        _discover_legacy_build_entries(root, policy, registry)
        available_free = _free_bytes(root) if free_bytes_override is None else free_bytes_override
        reclaimed: list[str] = []
        existing = _entry_by_path(registry, path)
        if existing is not None and existing["id"] not in _active_entry_ids(registry):
            requested_reservation = _size_reservation(policy, size_class)
            requested_growth = max(
                0, requested_reservation - int(existing["observedAllocatedBytes"])
            )
            if (
                _accounting_bytes(existing) > limits["hardBytes"]
                or available_free < limits["minFreeBytes"] + requested_growth
            ):
                freed = _reclaim_entry(root, state_root, policy, registry, existing)
                available_free += freed
                reclaimed.append(existing["id"])
                existing = None
        registry["sequence"] += 1
        candidate = _entry(
            policy,
            owner,
            content_key,
            path,
            size_class,
            registry["sequence"],
            allocated_bytes(requested_path),
        )
        was_new = existing is None
        current = _register_entry(registry, candidate)
        current["lastUseSequence"] = registry["sequence"]
        needed_growth = max(
            0, int(current["reservationBytes"]) - int(current["observedAllocatedBytes"])
        )
        try:
            reclaimed.extend(_enforce_limits(
                root,
                state_root,
                policy,
                registry,
                current["id"],
                needed_growth,
                limits,
                available_free,
            ))
        except GeneratedStateError:
            if was_new:
                registry["entries"] = [
                    item for item in registry["entries"] if item["id"] != current["id"]
                ]
            _sort_registry(registry)
            _write_registry(state_root, policy, registry)
            raise
        if len(registry["leases"]) >= policy["limits"]["maxLeases"]:
            if was_new:
                registry["entries"] = [
                    item for item in registry["entries"] if item["id"] != current["id"]
                ]
            _sort_registry(registry)
            _write_registry(state_root, policy, registry)
            raise GeneratedStateError("generated-state lease bound exceeded")
        token = secrets.token_hex(16)
        while any(lease["id"] == token for lease in registry["leases"]):
            token = secrets.token_hex(16)
        registry["leases"].append(
            {
                "entryId": current["id"],
                "id": token,
                "pid": lease_pid,
                "processIdentity": identity,
            }
        )
        _sort_registry(registry)
        _write_registry(state_root, policy, registry)
        accounting = sum(
            _accounting_bytes(entry)
            for entry in registry["entries"]
            if entry["retentionClass"] == "rebuildable-output"
        )
        return {
            "accountingBytes": accounting,
            "entryId": current["id"],
            "hardBytes": limits["hardBytes"],
            "leasePid": lease_pid,
            "leaseToken": token,
            "reclaimedEntryIds": reclaimed,
            "softBytes": limits["softBytes"],
        }


def release(
    root: Path,
    token: str,
    policy_path: Path | None = None,
    identity_fn: Callable[[int], str | None] = process_identity,
) -> dict[str, Any]:
    if not TOKEN_RE.fullmatch(token):
        raise GeneratedStateError("generated-state lease token is invalid")
    root = root.resolve()
    policy, _, policy_sha = load_policy(root, policy_path)
    with state_lock(root, policy, create=False) as state_root_value:
        if state_root_value is None:
            raise GeneratedStateError("generated-state registry is absent")
        state_root = state_root_value
        registry = _load_registry(state_root, policy, policy_sha)
        _recover_transaction(root, state_root, policy, registry)
        lease = next((item for item in registry["leases"] if item["id"] == token), None)
        if lease is None:
            raise GeneratedStateError("generated-state lease is unknown or already released")
        current_identity = identity_fn(lease["pid"])
        if current_identity != lease["processIdentity"]:
            raise GeneratedStateError("generated-state lease process identity changed")
        entry = _entry_by_id(registry, lease["entryId"])
        if entry is None:  # pragma: no cover - registry validation closes this
            raise GeneratedStateError("generated-state lease entry is absent")
        entry["observedAllocatedBytes"] = allocated_bytes(
            _safe_absolute(root, entry["path"])
        )
        registry["sequence"] += 1
        entry["lastUseSequence"] = registry["sequence"]
        registry["leases"] = [item for item in registry["leases"] if item["id"] != token]
        _sort_registry(registry)
        _write_registry(state_root, policy, registry)
        return {
            "entryId": entry["id"],
            "observedAllocatedBytes": entry["observedAllocatedBytes"],
            "released": True,
        }


def register_protected(
    root: Path,
    owner: str,
    content_key: str,
    path: str,
    size_class: str = "protected",
    policy_path: Path | None = None,
) -> dict[str, Any]:
    root = root.resolve()
    policy, _, policy_sha = load_policy(root, policy_path)
    producer = _producer(policy, owner)
    if producer["leaseMode"] != "protected":
        raise GeneratedStateError("rebuildable generated state requires admission")
    absolute = _safe_absolute(root, path)
    with state_lock(root, policy) as state_root_value:
        assert state_root_value is not None
        state_root = state_root_value
        registry = _load_registry(state_root, policy, policy_sha)
        _recover_transaction(root, state_root, policy, registry)
        registry["sequence"] += 1
        candidate = _entry(
            policy, owner, content_key, path, size_class, registry["sequence"],
            allocated_bytes(absolute),
        )
        current = _register_entry(registry, candidate)
        current["lastUseSequence"] = registry["sequence"]
        current["observedAllocatedBytes"] = candidate["observedAllocatedBytes"]
        _sort_registry(registry)
        _write_registry(state_root, policy, registry)
        return {"entryId": current["id"], "protected": True}


def status(
    root: Path,
    policy_path: Path | None = None,
    identity_fn: Callable[[int], str | None] = process_identity,
) -> dict[str, Any]:
    root = root.resolve()
    policy, _, policy_sha = load_policy(root, policy_path)
    limits = _effective_limits(policy, os.environ)
    with state_lock(root, policy, create=False) as state_root_value:
        if state_root_value is None:
            registry = _new_registry(policy_sha)
            recovered = 0
        else:
            state_root = state_root_value
            registry = _load_registry(state_root, policy, policy_sha)
            _recover_transaction(root, state_root, policy, registry)
            recovered = _recover_leases(root, registry, identity_fn)
            if recovered:
                _sort_registry(registry)
                _write_registry(state_root, policy, registry)
    rebuildable = [
        entry for entry in registry["entries"] if entry["retentionClass"] == "rebuildable-output"
    ]
    return {
        "accountingBytes": sum(_accounting_bytes(entry) for entry in rebuildable),
        "activeLeases": len(registry["leases"]),
        "entryCount": len(registry["entries"]),
        "hardBytes": limits["hardBytes"],
        "kind": "genesis/generated-state-status-v0.1",
        "protectedEntries": len(registry["entries"]) - len(rebuildable),
        "rebuildableEntries": len(rebuildable),
        "recoveredStaleLeases": recovered,
        "softBytes": limits["softBytes"],
        "version": "0.1",
    }


def active_leases_below(
    root: Path,
    path: str,
    policy_path: Path | None = None,
    identity_fn: Callable[[int], str | None] = process_identity,
) -> bool:
    root = root.resolve()
    policy, _, policy_sha = load_policy(root, policy_path)
    with state_lock(root, policy, create=False) as state_root_value:
        if state_root_value is None:
            return False
        registry = _load_registry(state_root_value, policy, policy_sha)
        base = PurePosixPath(repo_path(path, "lease query path"))
        entries = {entry["id"]: PurePosixPath(entry["path"]) for entry in registry["entries"]}
        for lease in registry["leases"]:
            if identity_fn(lease["pid"]) != lease["processIdentity"]:
                continue
            candidate = entries[lease["entryId"]]
            if candidate == base or base in candidate.parents or candidate in base.parents:
                return True
        return False


@contextmanager
def cleanup_guard(
    root: Path,
    paths: Sequence[str],
    policy_path: Path | None = None,
    identity_fn: Callable[[int], str | None] = process_identity,
) -> Iterator[None]:
    """Serialize whole-root cleanup against admission and active leases."""
    root = root.resolve()
    default_policy = root / POLICY_REL
    if policy_path is None and not default_policy.is_file():
        yield
        return
    policy, _, policy_sha = load_policy(root, policy_path)
    with state_lock(root, policy, create=False) as state_root_value:
        if state_root_value is None:
            yield
            return
        registry = _load_registry(state_root_value, policy, policy_sha)
        entries = {entry["id"]: PurePosixPath(entry["path"]) for entry in registry["entries"]}
        guarded = [PurePosixPath(repo_path(path, "cleanup guard path")) for path in paths]
        for lease in registry["leases"]:
            if identity_fn(lease["pid"]) != lease["processIdentity"]:
                continue
            candidate = entries[lease["entryId"]]
            if any(
                candidate == base or base in candidate.parents or candidate in base.parents
                for base in guarded
            ):
                raise GeneratedStateError("active generated-state lease blocks whole-root cleanup")
        yield


def _emit_acquire(result: Mapping[str, Any], output_format: str, root: Path) -> None:
    if output_format == "json":
        print(pretty_bytes(result).decode("ascii"), end="")
    elif output_format == "shell":
        print(f"export GENESIS_GENERATED_STATE_ROOT={json.dumps(str(root.resolve()))}")
        print(f"export GENESIS_GENERATED_STATE_LEASE_TOKEN={result['leaseToken']}")
    else:
        print(result["leaseToken"])


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--policy", type=Path)
    commands = parser.add_subparsers(dest="command", required=True)
    acquire = commands.add_parser("acquire")
    acquire.add_argument("--owner", required=True)
    acquire.add_argument("--content-key", required=True)
    acquire.add_argument("--path", required=True)
    acquire.add_argument("--size-class", required=True)
    acquire.add_argument("--pid", type=int)
    acquire.add_argument("--format", choices=("token", "shell", "json"), default="token")
    release_cmd = commands.add_parser("release")
    release_cmd.add_argument("--token", required=True)
    status_cmd = commands.add_parser("status")
    status_cmd.add_argument("--format", choices=("human", "json"), default="human")
    protected = commands.add_parser("register-protected")
    protected.add_argument("--owner", required=True)
    protected.add_argument("--content-key", required=True)
    protected.add_argument("--path", required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "acquire":
            result = admit(
                args.root,
                args.owner,
                args.content_key,
                args.path,
                args.size_class,
                pid=args.pid,
                policy_path=args.policy,
            )
            _emit_acquire(result, args.format, args.root)
        elif args.command == "release":
            result = release(args.root, args.token, args.policy)
            print(pretty_bytes(result).decode("ascii"), end="")
        elif args.command == "register-protected":
            result = register_protected(
                args.root, args.owner, args.content_key, args.path, policy_path=args.policy
            )
            print(pretty_bytes(result).decode("ascii"), end="")
        else:
            result = status(args.root, args.policy)
            if args.format == "json":
                print(pretty_bytes(result).decode("ascii"), end="")
            else:
                print(
                    "generated-state: ok "
                    f"entries={result['entryCount']} active_leases={result['activeLeases']} "
                    f"accounting_bytes={result['accountingBytes']} hard_bytes={result['hardBytes']}"
                )
    except (GeneratedStateError, OSError) as exc:
        print(f"generated-state: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
