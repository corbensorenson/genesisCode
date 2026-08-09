#!/usr/bin/env python3
"""Prepare and verify the exact RustSec snapshot used by cargo-deny."""

from __future__ import annotations

import argparse
import copy
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from typing import Any, Dict, Iterable, List, Mapping, Optional, Sequence, Tuple

import deterministic_cleanup


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "policies/rustsec_advisory_db_v0.1.json"
SCHEMA_PATH = ROOT / "docs/spec/RUSTSEC_ADVISORY_DB_v0.1.schema.json"
POLICY_KIND = "genesis/rustsec-advisory-database-policy-v0.1"
SOURCE_URL = "https://github.com/RustSec/advisory-db"
STORAGE_ROOT = ".genesis/dependency-mirrors/rustsec-advisory-db-v0.1"
DATABASE_DIRECTORY = "advisory-db-3157b0e258782691"
SHA40_RE = re.compile(r"^[0-9a-f]{40}$")
SHA64_RE = re.compile(r"^[0-9a-f]{64}$")


class RustSecError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RustSecError(message)


def reject_float(value: str) -> None:
    raise RustSecError("floating-point JSON values are not allowed")


def reject_constant(value: str) -> None:
    raise RustSecError("non-finite JSON value is not allowed: " + value)


def unique_pairs(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key: " + key)
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=unique_pairs,
            parse_float=reject_float,
            parse_constant=reject_constant,
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise RustSecError("invalid JSON at {0}: {1}".format(path, exc)) from exc


def exact_keys(value: Mapping[str, Any], expected: Iterable[str], label: str) -> None:
    expected_set = set(expected)
    actual = set(value)
    require(
        actual == expected_set,
        "{0} keys differ: missing={1} unknown={2}".format(
            label, sorted(expected_set - actual), sorted(actual - expected_set)
        ),
    )


def canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        + "\n"
    ).encode("ascii")


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_relative_path(raw: Any, label: str) -> str:
    require(isinstance(raw, str) and raw, label + " must be a non-empty string")
    require("\\" not in raw and not raw.startswith("/") and "//" not in raw, label + " is not canonical")
    parts = PurePosixPath(raw).parts
    require(parts and all(part not in ("", ".", "..") for part in parts), label + " is not canonical")
    require(PurePosixPath(*parts).as_posix() == raw, label + " is not normalized")
    return raw


def validate_schema() -> None:
    schema = load_json(SCHEMA_PATH)
    require(isinstance(schema, dict), "RustSec schema must be an object")
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "RustSec schema draft drift")
    require(schema.get("additionalProperties") is False, "RustSec schema must be closed")


def validate_policy(document: Any) -> Dict[str, Any]:
    require(isinstance(document, dict), "RustSec policy must be an object")
    exact_keys(document, ["kind", "version", "source", "storage", "tool", "nonclaims"], "policy")
    require(document["kind"] == POLICY_KIND and document["version"] == "0.1", "RustSec policy identity drift")

    source = document["source"]
    require(isinstance(source, dict), "policy.source must be an object")
    exact_keys(source, ["url", "commit", "treeSha256", "licensePaths"], "policy.source")
    require(source["url"] == SOURCE_URL, "RustSec source URL must be exact")
    require(isinstance(source["commit"], str) and SHA40_RE.fullmatch(source["commit"]) is not None, "RustSec source commit must be exact")
    require(isinstance(source["treeSha256"], str) and SHA64_RE.fullmatch(source["treeSha256"]) is not None and source["treeSha256"] != "0" * 64, "RustSec tree identity must be nonzero SHA-256")
    licenses = source["licensePaths"]
    require(isinstance(licenses, list) and len(licenses) == 3, "RustSec license paths must contain exactly three entries")
    normalized_licenses = [safe_relative_path(item, "RustSec license path") for item in licenses]
    require(normalized_licenses == sorted(set(normalized_licenses)), "RustSec license paths must be sorted and unique")

    storage = document["storage"]
    require(isinstance(storage, dict), "policy.storage must be an object")
    exact_keys(storage, ["root", "databaseDirectory", "maxFiles", "maxBytes"], "policy.storage")
    require(storage["root"] == STORAGE_ROOT, "RustSec storage root drift")
    require(storage["databaseDirectory"] == DATABASE_DIRECTORY, "RustSec cargo-deny database directory drift")
    for field, maximum in (("maxFiles", 10000), ("maxBytes", 134217728)):
        require(isinstance(storage[field], int) and not isinstance(storage[field], bool) and 0 < storage[field] <= maximum, "RustSec {0} bound invalid".format(field))

    tool = document["tool"]
    require(tool == {"name": "cargo-deny", "version": "0.19.0"}, "RustSec cargo-deny identity drift")
    nonclaims = document["nonclaims"]
    require(isinstance(nonclaims, list) and len(nonclaims) == 3 and all(isinstance(item, str) and item for item in nonclaims), "RustSec nonclaims incomplete")
    return dict(document)


def run(command: Sequence[str], *, cwd: Path, timeout: int = 120, env: Optional[Mapping[str, str]] = None) -> str:
    try:
        result = subprocess.run(
            list(command),
            cwd=str(cwd),
            env=None if env is None else dict(env),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise RustSecError("command failed to execute: {0}: {1}".format(command[0], exc)) from exc
    require(result.returncode == 0, "command failed: {0}: {1}".format(command[0], result.stdout.strip()))
    return result.stdout.strip()


def git(repo: Path, *arguments: str, timeout: int = 120) -> str:
    return run(["git", "-c", "core.quotepath=false", *arguments], cwd=repo, timeout=timeout)


def require_safe_directory_chain(root: Path, path: Path, *, allow_absent: bool) -> None:
    root = root.absolute()
    path = path.absolute()
    try:
        relative = path.relative_to(root)
    except ValueError as exc:
        raise RustSecError("RustSec storage path escapes the repository") from exc
    root_device = root.stat().st_dev
    current = root
    for part in relative.parts:
        current = current / part
        if not current.exists() and not current.is_symlink():
            require(allow_absent, "RustSec storage directory is absent: " + str(current))
            return
        require(not current.is_symlink(), "RustSec storage directory is a symlink: " + str(current))
        info = current.stat()
        require(stat.S_ISDIR(info.st_mode), "RustSec storage path is not a directory: " + str(current))
        require(info.st_dev == root_device, "RustSec storage crosses a filesystem boundary")


def tree_identity(repo: Path) -> Dict[str, Any]:
    try:
        raw = subprocess.run(
            ["git", "-c", "core.quotepath=false", "ls-tree", "-rz", "--full-tree", "HEAD"],
            cwd=str(repo),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise RustSecError("cannot enumerate RustSec Git tree: " + str(exc)) from exc
    require(raw.returncode == 0, "cannot enumerate RustSec Git tree")
    records: List[Dict[str, Any]] = []
    total_bytes = 0
    for entry in raw.stdout.split(b"\0"):
        if not entry:
            continue
        metadata, separator, encoded_path = entry.partition(b"\t")
        require(separator == b"\t" and encoded_path, "RustSec Git tree entry is malformed")
        fields = metadata.decode("ascii").split(" ")
        require(len(fields) == 3, "RustSec Git tree metadata is malformed")
        mode, kind, _object_id = fields
        require(kind == "blob" and mode in {"100644", "100755"}, "RustSec tree contains unsupported entry type")
        try:
            relative = encoded_path.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise RustSecError("RustSec tree path is not UTF-8") from exc
        relative = safe_relative_path(relative, "RustSec tree path")
        path = repo / relative
        require(path.is_file() and not path.is_symlink(), "RustSec tree entry is not a regular file: " + relative)
        size = path.stat().st_size
        total_bytes += size
        records.append({"mode": mode, "path": relative, "sha256": file_sha256(path), "size": size})
    require(records == sorted(records, key=lambda row: row["path"]), "RustSec Git tree order drift")
    return {
        "fileCount": len(records),
        "totalBytes": total_bytes,
        "treeSha256": hashlib.sha256(canonical_json(records)).hexdigest(),
    }


def verify_repository(repo: Path, policy: Mapping[str, Any]) -> Dict[str, Any]:
    require(repo.is_dir() and not repo.is_symlink(), "RustSec snapshot directory is absent or unsafe")
    head = git(repo, "rev-parse", "HEAD")
    require(head == policy["source"]["commit"], "RustSec snapshot commit mismatch")
    require(not git(repo, "status", "--porcelain", "--untracked-files=all"), "RustSec snapshot worktree is dirty")
    git(repo, "fsck", "--strict", "--no-progress")
    identity = tree_identity(repo)
    require(identity["treeSha256"] == policy["source"]["treeSha256"], "RustSec snapshot tree identity mismatch")
    require(identity["fileCount"] <= policy["storage"]["maxFiles"], "RustSec snapshot file bound exceeded")
    require(identity["totalBytes"] <= policy["storage"]["maxBytes"], "RustSec snapshot byte bound exceeded")
    for relative in policy["source"]["licensePaths"]:
        require((repo / relative).is_file(), "RustSec snapshot license is absent: " + relative)
    return identity


def policy_identity() -> str:
    return file_sha256(POLICY_PATH)


def install_paths(policy: Mapping[str, Any]) -> Tuple[Path, Path, Path]:
    base = ROOT / policy["storage"]["root"]
    install = base / policy_identity()
    db_parent = install / "db"
    repo = db_parent / policy["storage"]["databaseDirectory"]
    return install, db_parent, repo


def verify_install_layout(install: Path, db_parent: Path, repo: Path) -> None:
    base = install.parent
    require_safe_directory_chain(ROOT, base, allow_absent=False)
    require_safe_directory_chain(ROOT, install, allow_absent=False)
    require_safe_directory_chain(ROOT, db_parent, allow_absent=False)
    require_safe_directory_chain(ROOT, repo, allow_absent=False)


def verify_tool(policy: Mapping[str, Any]) -> None:
    output = run(["cargo-deny", "--version"], cwd=ROOT, timeout=30)
    require(output == "cargo-deny " + policy["tool"]["version"], "cargo-deny version mismatch")


def prepare(policy: Mapping[str, Any]) -> Dict[str, Any]:
    verify_tool(policy)
    install, _db_parent, repo = install_paths(policy)
    base = install.parent
    require_safe_directory_chain(ROOT, base, allow_absent=True)
    base.mkdir(parents=True, exist_ok=True)
    require_safe_directory_chain(ROOT, base, allow_absent=False)
    lock_path = base / ".prepare.lock"
    lock_flags = os.O_RDWR | os.O_CREAT
    if hasattr(os, "O_NOFOLLOW"):
        lock_flags |= os.O_NOFOLLOW
    lock_fd = os.open(lock_path, lock_flags, 0o600)
    with os.fdopen(lock_fd, "a+b") as lock:
        require(stat.S_ISREG(os.fstat(lock.fileno()).st_mode), "RustSec preparation lock is not a regular file")
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        if install.exists():
            verify_install_layout(install, install / "db", repo)
            identity = verify_repository(repo, policy)
            deterministic_cleanup.initialize_root_marker(
                ROOT, ".genesis/dependency-mirrors", "dependency-mirror"
            )
            print("rustsec-advisory-db: reused commit={0} tree={1}".format(policy["source"]["commit"], identity["treeSha256"]))
            return identity
        temporary = Path(tempfile.mkdtemp(prefix=install.name + ".tmp.", dir=str(base)))
        try:
            temporary_repo = temporary / "db" / policy["storage"]["databaseDirectory"]
            temporary_repo.mkdir(parents=True)
            git(temporary_repo, "init", "--quiet")
            git(temporary_repo, "remote", "add", "origin", policy["source"]["url"])
            git(temporary_repo, "fetch", "--quiet", "--depth=1", "--no-tags", "origin", policy["source"]["commit"], timeout=300)
            git(temporary_repo, "checkout", "--quiet", "--detach", "FETCH_HEAD")
            identity = verify_repository(temporary_repo, policy)
            os.replace(str(temporary), str(install))
            verify_install_layout(install, install / "db", repo)
        except Exception:
            shutil.rmtree(temporary, ignore_errors=True)
            raise
    deterministic_cleanup.initialize_root_marker(
        ROOT, ".genesis/dependency-mirrors", "dependency-mirror"
    )
    print("rustsec-advisory-db: prepared commit={0} tree={1}".format(policy["source"]["commit"], identity["treeSha256"]))
    return identity


def render_deny_config(policy: Mapping[str, Any], output: Path) -> None:
    install, db_parent, repo = install_paths(policy)
    verify_install_layout(install, db_parent, repo)
    verify_repository(repo, policy)
    source = (ROOT / "deny.toml").read_text(encoding="utf-8")
    require(source.count("[advisories]") == 1, "deny.toml advisories section drift")
    require("db-path" not in source and "db-urls" not in source, "deny.toml must not embed host-specific RustSec paths")
    insertion = (
        "[advisories]\n"
        + "db-path = " + json.dumps(str(db_parent), ensure_ascii=True) + "\n"
        + "db-urls = [" + json.dumps(policy["source"]["url"], ensure_ascii=True) + "]"
    )
    rendered = source.replace("[advisories]", insertion, 1)
    output.write_text(rendered, encoding="utf-8")


def expect_rejection(document: Any, message: str) -> None:
    try:
        validate_policy(document)
    except RustSecError as exc:
        require(message in str(exc), "wrong RustSec rejection: " + str(exc))
        return
    raise RustSecError("RustSec negative control was accepted")


def self_test(policy: Mapping[str, Any]) -> int:
    controls = 0
    candidate = copy.deepcopy(policy)
    candidate["source"]["commit"] = "main"
    expect_rejection(candidate, "commit must be exact")
    controls += 1
    candidate = copy.deepcopy(policy)
    candidate["source"]["url"] = "https://example.invalid/advisory-db"
    expect_rejection(candidate, "source URL must be exact")
    controls += 1
    candidate = copy.deepcopy(policy)
    candidate["source"]["treeSha256"] = "0" * 64
    expect_rejection(candidate, "tree identity")
    controls += 1
    candidate = copy.deepcopy(policy)
    candidate["storage"]["root"] = "../escape"
    expect_rejection(candidate, "storage root drift")
    controls += 1
    candidate = copy.deepcopy(policy)
    candidate["trustMe"] = True
    expect_rejection(candidate, "keys differ")
    controls += 1

    with tempfile.TemporaryDirectory(prefix="genesis-rustsec-self-test.") as raw:
        root = Path(raw)
        repo = root / "repo"
        repo.mkdir()
        git(repo, "init", "--quiet")
        (repo / "LICENSE.txt").write_text("fixture\n", encoding="ascii")
        (repo / "LICENSES").mkdir()
        (repo / "LICENSES/CC-BY-4.0.txt").write_text("fixture\n", encoding="ascii")
        (repo / "LICENSES/CC0-1.0.txt").write_text("fixture\n", encoding="ascii")
        (repo / "crates").mkdir()
        (repo / "crates/example.md").write_text("fixture\n", encoding="ascii")
        environment = dict(os.environ)
        environment.update({"GIT_AUTHOR_NAME": "Genesis fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid", "GIT_COMMITTER_NAME": "Genesis fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid"})
        run(["git", "add", "."], cwd=repo, env=environment)
        run(["git", "commit", "--quiet", "-m", "fixture"], cwd=repo, env=environment)
        fixture_policy = copy.deepcopy(policy)
        fixture_policy["source"]["commit"] = git(repo, "rev-parse", "HEAD")
        identity = tree_identity(repo)
        fixture_policy["source"]["treeSha256"] = identity["treeSha256"]
        verify_repository(repo, fixture_policy)
        controls += 1
        (repo / "crates/example.md").write_text("tampered\n", encoding="ascii")
        try:
            verify_repository(repo, fixture_policy)
        except RustSecError as exc:
            require("dirty" in str(exc), "tamper control returned wrong error")
        else:
            raise RustSecError("tampered RustSec snapshot was accepted")
        controls += 1
        outside = root / "outside"
        outside.mkdir()
        linked = root / "linked"
        linked.symlink_to(outside, target_is_directory=True)
        try:
            require_safe_directory_chain(root, linked / "snapshot", allow_absent=True)
        except RustSecError as exc:
            require("symlink" in str(exc), "symlink control returned wrong error")
        else:
            raise RustSecError("symlinked RustSec storage ancestor was accepted")
        controls += 1
    return controls


def inspect_source(path: Path) -> None:
    identity = tree_identity(path)
    print(json.dumps(identity, sort_keys=True, separators=(",", ":")))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["check", "inspect-source", "prepare", "resolve", "render-deny-config", "self-test"])
    parser.add_argument("--source", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.command == "inspect-source":
        require(args.source is not None, "inspect-source requires --source")
        inspect_source(args.source)
        return 0

    validate_schema()
    policy = validate_policy(load_json(POLICY_PATH))
    if args.command == "check":
        print("rustsec-advisory-db: policy ok commit={0}".format(policy["source"]["commit"]))
    elif args.command == "prepare":
        prepare(policy)
    elif args.command == "resolve":
        verify_tool(policy)
        install, db_parent, repo = install_paths(policy)
        verify_install_layout(install, db_parent, repo)
        verify_repository(repo, policy)
        print(str(db_parent))
    elif args.command == "render-deny-config":
        require(args.output is not None, "render-deny-config requires --output")
        render_deny_config(policy, args.output)
    elif args.command == "self-test":
        controls = self_test(policy)
        print("rustsec-advisory-db: self-test ok (controls={0})".format(controls))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RustSecError, OSError, UnicodeError) as exc:
        print("rustsec-advisory-db: " + str(exc), file=sys.stderr)
        raise SystemExit(1)
