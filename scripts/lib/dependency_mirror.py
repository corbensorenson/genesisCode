#!/usr/bin/env python3
"""Create, verify, and exercise GenesisCode dependency mirrors."""

from __future__ import annotations

import argparse
import base64
import binascii
import copy
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import socket
import stat
import subprocess
import sys
import tarfile
import tempfile
import threading
from typing import Any, Dict, Iterable, List, Mapping, Optional, Sequence, Set, Tuple
import urllib.error
import urllib.parse
import urllib.request

import deterministic_cleanup


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_POLICY = ROOT / "genesis.dependency-mirror.json"
POLICY_KIND = "genesis/dependency-mirror-policy-v0.1"
MANIFEST_KIND = "genesis/dependency-mirror-manifest-v0.1"
CARGO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
VERSION_RE = re.compile(r"^[0-9A-Za-z.+_-]+$")
SAFE_ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
EXCLUDED_SOURCE_TOP_LEVEL = {".genesis", ".git", ".tmp", "node_modules", "target"}
EXPECTED_OFFLINE_CHECKS = [
    "cargo-cli-build",
    "cargo-evidence-verifier-check",
    "cargo-workspace-check",
    "npm-clean-install",
    "npm-playwright-import",
]
class MirrorError(ValueError):
    pass


def reject_float(value: str) -> None:
    raise MirrorError("floating-point JSON values are not allowed")


def reject_constant(value: str) -> None:
    raise MirrorError("non-finite JSON values are not allowed: " + value)


def object_pairs(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    for key, value in pairs:
        if key in out:
            raise MirrorError("duplicate JSON key: " + key)
        out[key] = value
    return out


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=object_pairs,
            parse_float=reject_float,
            parse_constant=reject_constant,
        )
    except (OSError, UnicodeError, json.JSONDecodeError, MirrorError) as exc:
        if isinstance(exc, MirrorError):
            raise
        raise MirrorError("invalid JSON at {0}: {1}".format(path, exc)) from exc


def canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, separators=(",", ":"), sort_keys=True) + "\n"
    ).encode("utf-8")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def hash_file(path: Path, max_bytes: Optional[int] = None) -> Tuple[str, int]:
    digest = hashlib.sha256()
    total = 0
    try:
        with path.open("rb") as handle:
            while True:
                chunk = handle.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if max_bytes is not None and total > max_bytes:
                    raise MirrorError("file exceeds byte bound: " + str(path))
                digest.update(chunk)
    except OSError as exc:
        raise MirrorError("cannot hash {0}: {1}".format(path, exc)) from exc
    return digest.hexdigest(), total


def hash_file_with(path: Path, algorithm: str, max_bytes: Optional[int] = None) -> Tuple[str, int]:
    try:
        digest = hashlib.new(algorithm)
    except ValueError as exc:
        raise MirrorError("unsupported digest algorithm: " + algorithm) from exc
    total = 0
    try:
        with path.open("rb") as handle:
            while True:
                chunk = handle.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if max_bytes is not None and total > max_bytes:
                    raise MirrorError("file exceeds byte bound: " + str(path))
                digest.update(chunk)
    except OSError as exc:
        raise MirrorError("cannot hash {0}: {1}".format(path, exc)) from exc
    return digest.hexdigest(), total


def require_dict(value: Any, label: str) -> Dict[str, Any]:
    if not isinstance(value, dict):
        raise MirrorError(label + " must be an object")
    return value


def require_list(value: Any, label: str) -> List[Any]:
    if not isinstance(value, list):
        raise MirrorError(label + " must be an array")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise MirrorError(label + " must be a non-empty string")
    return value


def require_int(value: Any, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 1:
        raise MirrorError(label + " must be a positive integer")
    return value


def exact_keys(value: Mapping[str, Any], expected: Iterable[str], label: str) -> None:
    expected_set = set(expected)
    actual = set(value)
    if actual != expected_set:
        raise MirrorError(
            "{0} keys differ: missing={1} unknown={2}".format(
                label,
                sorted(expected_set - actual),
                sorted(actual - expected_set),
            )
        )


def safe_relative_path(raw: str, label: str) -> str:
    value = require_string(raw, label)
    if "\\" in value or value.startswith("/") or "//" in value:
        raise MirrorError(label + " is not a canonical relative path: " + value)
    parts = PurePosixPath(value).parts
    if not parts or any(part in ("", ".", "..") for part in parts):
        raise MirrorError(label + " is not a canonical relative path: " + value)
    if PurePosixPath(*parts).as_posix() != value:
        raise MirrorError(label + " is not normalized: " + value)
    return value


def sorted_unique_strings(value: Any, label: str) -> List[str]:
    items = [require_string(item, label + "[]") for item in require_list(value, label)]
    if items != sorted(items) or len(items) != len(set(items)):
        raise MirrorError(label + " must be sorted and unique")
    return items


def authority_path(root: Path, relative: str) -> Path:
    path = root / safe_relative_path(relative, "authority path")
    if not path.is_file() or path.is_symlink():
        raise MirrorError("authority must be a regular non-symlink file: " + relative)
    try:
        path.resolve().relative_to(root.resolve())
    except ValueError as exc:
        raise MirrorError("authority escapes repository root: " + relative) from exc
    return path


def validate_policy(path: Path, root: Path = ROOT) -> Dict[str, Any]:
    policy = require_dict(load_json(path), "policy")
    exact_keys(
        policy,
        [
            "authorityFiles",
            "cargo",
            "fetchPolicy",
            "kind",
            "mirror",
            "networkIsolation",
            "npm",
            "offlineChecks",
            "version",
        ],
        "policy",
    )
    if policy["kind"] != POLICY_KIND or policy["version"] != "0.1":
        raise MirrorError("unsupported dependency mirror policy kind/version")

    authorities = sorted_unique_strings(policy["authorityFiles"], "policy.authorityFiles")
    for relative in authorities:
        authority_path(root, relative)

    npm = require_dict(policy["npm"], "policy.npm")
    exact_keys(
        npm,
        ["lockfile", "manifest", "maxPackageBytes", "maxTotalBytes", "requiredIntegrity"],
        "policy.npm",
    )
    npm_manifest = safe_relative_path(npm["manifest"], "npm manifest")
    npm_lockfile = safe_relative_path(npm["lockfile"], "npm lockfile")
    authority_path(root, npm_manifest)
    authority_path(root, npm_lockfile)
    if npm["requiredIntegrity"] != "sha512":
        raise MirrorError("npm integrity algorithm must be sha512")
    require_int(npm["maxPackageBytes"], "policy.npm.maxPackageBytes")
    require_int(npm["maxTotalBytes"], "policy.npm.maxTotalBytes")

    cargo = require_dict(policy["cargo"], "policy.cargo")
    exact_keys(
        cargo,
        [
            "maxArchiveBytes",
            "maxExpandedBytes",
            "maxExpandedFiles",
            "registrySource",
            "vendorArchive",
            "workspaces",
        ],
        "policy.cargo",
    )
    if cargo["registrySource"] != CARGO_SOURCE:
        raise MirrorError("policy.cargo.registrySource must be the pinned crates.io source")
    require_int(cargo["maxArchiveBytes"], "policy.cargo.maxArchiveBytes")
    require_int(cargo["maxExpandedBytes"], "policy.cargo.maxExpandedBytes")
    require_int(cargo["maxExpandedFiles"], "policy.cargo.maxExpandedFiles")
    archive = require_dict(cargo["vendorArchive"], "policy.cargo.vendorArchive")
    exact_keys(archive, ["compression", "format", "metadataProfile"], "vendorArchive")
    if archive != {
        "compression": "gzip",
        "format": "ustar",
        "metadataProfile": "genesis/canonical-archive-v0.1",
    }:
        raise MirrorError("unsupported Cargo vendor archive profile")

    workspaces = require_list(cargo["workspaces"], "policy.cargo.workspaces")
    observed_workspaces: List[Tuple[str, str, str]] = []
    for index, raw in enumerate(workspaces):
        item = require_dict(raw, "cargo workspace")
        exact_keys(item, ["id", "lockfile", "manifest"], "cargo workspace")
        workspace_id = require_string(item["id"], "cargo workspace id")
        if not SAFE_ID_RE.fullmatch(workspace_id):
            raise MirrorError("invalid Cargo workspace id: " + workspace_id)
        manifest = safe_relative_path(item["manifest"], "cargo manifest")
        lockfile = safe_relative_path(item["lockfile"], "cargo lockfile")
        authority_path(root, manifest)
        authority_path(root, lockfile)
        observed_workspaces.append((workspace_id, lockfile, manifest))
    if observed_workspaces != sorted(observed_workspaces):
        raise MirrorError("policy.cargo.workspaces must be sorted by identity")
    if len({item[0] for item in observed_workspaces}) != len(observed_workspaces):
        raise MirrorError("duplicate Cargo workspace id")
    expected_authorities = sorted(
        [item for row in observed_workspaces for item in (row[1], row[2])]
        + [npm_lockfile, npm_manifest]
    )
    if authorities != expected_authorities:
        raise MirrorError("policy.authorityFiles must exactly close Cargo and npm authorities")

    fetch = require_dict(policy["fetchPolicy"], "policy.fetchPolicy")
    exact_keys(
        fetch,
        ["cargoRegistryIndex", "mode", "npmRegistryOrigin", "redirects", "requireTls"],
        "policy.fetchPolicy",
    )
    if fetch != {
        "cargoRegistryIndex": "https://github.com/rust-lang/crates.io-index",
        "mode": "declared-once",
        "npmRegistryOrigin": "https://registry.npmjs.org/",
        "redirects": "deny",
        "requireTls": True,
    }:
        raise MirrorError("unsupported fetch policy")

    mirror = require_dict(policy["mirror"], "policy.mirror")
    exact_keys(mirror, ["manifestKind", "root", "storeMode"], "policy.mirror")
    if mirror != {
        "manifestKind": MANIFEST_KIND,
        "root": ".genesis/dependency-mirrors",
        "storeMode": "create-new-sha256",
    }:
        raise MirrorError("unsupported mirror storage policy")

    isolation = require_dict(policy["networkIsolation"], "policy.networkIsolation")
    exact_keys(isolation, ["darwin", "linux", "windows"], "networkIsolation")
    expected_isolation = {
        "darwin": {
            "backend": "sandbox-exec",
            "minimumEvidence": "live-loopback-connect-denied",
        },
        "linux": {
            "backend": "network-namespace",
            "minimumEvidence": "live-loopback-connect-denied",
        },
        "windows": {"backend": "unsupported-fail-closed", "minimumEvidence": "none"},
    }
    if isolation != expected_isolation:
        raise MirrorError("network isolation matrix differs from the v0.1 closed profile")

    checks = sorted_unique_strings(policy["offlineChecks"], "policy.offlineChecks")
    if checks != EXPECTED_OFFLINE_CHECKS:
        raise MirrorError("offline check set is incomplete or unknown")
    return policy


def parse_cargo_lock(path: Path) -> List[Dict[str, str]]:
    packages: List[Dict[str, str]] = []
    current: Optional[Dict[str, str]] = None
    field_re = re.compile(r'^(name|version|source|checksum) = "([^"]+)"$')
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if line == "[[package]]":
            if current is not None:
                packages.append(current)
            current = {}
            continue
        if current is None:
            continue
        match = field_re.fullmatch(line)
        if match:
            key, value = match.groups()
            if key in current:
                raise MirrorError("duplicate Cargo lock field at {0}:{1}".format(path, line_number))
            current[key] = value
    if current is not None:
        packages.append(current)

    external: List[Dict[str, str]] = []
    for package in packages:
        source = package.get("source")
        if source is None:
            continue
        if source != CARGO_SOURCE:
            raise MirrorError("undeclared Cargo source in {0}: {1}".format(path, source))
        if set(package) != {"checksum", "name", "source", "version"}:
            raise MirrorError("external Cargo package lacks closed identity in " + str(path))
        if not SHA256_RE.fullmatch(package["checksum"]):
            raise MirrorError("invalid Cargo checksum in " + str(path))
        if not package["name"] or not VERSION_RE.fullmatch(package["version"]):
            raise MirrorError("invalid Cargo package identity in " + str(path))
        external.append(dict(package))
    return sorted(external, key=lambda item: (item["name"], item["version"], item["checksum"]))


def parse_npm_lock(path: Path, policy: Mapping[str, Any]) -> List[Dict[str, Any]]:
    document = require_dict(load_json(path), "npm lockfile")
    if document.get("lockfileVersion") != 3:
        raise MirrorError("npm lockfileVersion must be 3")
    packages = require_dict(document.get("packages"), "npm lockfile packages")
    origin = policy["fetchPolicy"]["npmRegistryOrigin"]
    out: List[Dict[str, Any]] = []
    seen_digests: Set[str] = set()
    for lock_path, raw in sorted(packages.items()):
        if not isinstance(lock_path, str):
            raise MirrorError("npm package path must be a string")
        item = require_dict(raw, "npm package " + lock_path)
        resolved = item.get("resolved")
        if resolved is None:
            continue
        resolved = require_string(resolved, "npm resolved URL")
        parsed = urllib.parse.urlsplit(resolved)
        if (
            parsed.scheme != "https"
            or parsed.netloc != "registry.npmjs.org"
            or parsed.username is not None
            or parsed.password is not None
            or parsed.port is not None
            or parsed.query
            or parsed.fragment
            or not parsed.path.startswith("/")
            or "//" in parsed.path
            or any(part in ("", ".", "..") for part in parsed.path.split("/")[1:])
            or not resolved.startswith(origin)
        ):
            raise MirrorError("npm URL is outside the exact declared origin: " + resolved)
        integrity = require_string(item.get("integrity"), "npm package integrity")
        if not integrity.startswith("sha512-"):
            raise MirrorError("npm package integrity must use sha512: " + lock_path)
        try:
            digest = base64.b64decode(integrity[7:], validate=True)
        except (binascii.Error, ValueError) as exc:
            raise MirrorError("invalid npm SHA-512 SRI: " + lock_path) from exc
        if len(digest) != 64:
            raise MirrorError("invalid npm SHA-512 digest length: " + lock_path)
        digest_hex = digest.hex()
        out.append(
            {
                "blob": "npm/sha512/{0}.tgz".format(digest_hex),
                "integrity": integrity,
                "lockPath": lock_path,
                "resolved": resolved,
                "sha512": digest_hex,
            }
        )
        seen_digests.add(digest_hex)
    if not out:
        raise MirrorError("npm lockfile has no mirrored packages")
    return out


def authority_records(policy: Mapping[str, Any], root: Path = ROOT) -> List[Dict[str, Any]]:
    records = []
    for relative in policy["authorityFiles"]:
        digest, size = hash_file(authority_path(root, relative))
        records.append({"bytes": size, "path": relative, "sha256": digest})
    return records


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req: Any, fp: Any, code: int, msg: str, headers: Any, url: str) -> Any:
        raise MirrorError("dependency download redirect denied: " + url)


def fetch_npm_package(url: str, expected_sha512: str, output: Path, max_bytes: int) -> int:
    opener = urllib.request.build_opener(NoRedirect())
    request = urllib.request.Request(url, headers={"User-Agent": "GenesisCode-Mirror/0.1"})
    digest = hashlib.sha512()
    total = 0
    try:
        with opener.open(request, timeout=60) as response, output.open("xb") as handle:
            if response.geturl() != url:
                raise MirrorError("dependency download URL changed")
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > max_bytes:
                    raise MirrorError("npm package exceeds per-package byte bound")
                digest.update(chunk)
                handle.write(chunk)
    except MirrorError:
        output.unlink(missing_ok=True)
        raise
    except (OSError, urllib.error.URLError) as exc:
        output.unlink(missing_ok=True)
        raise MirrorError("npm dependency fetch failed: " + str(exc)) from exc
    if digest.hexdigest() != expected_sha512:
        output.unlink(missing_ok=True)
        raise MirrorError("npm dependency SHA-512 mismatch: " + url)
    return total


def normalized_tree(root: Path) -> Tuple[str, int, int]:
    digest = hashlib.sha256()
    file_count = 0
    expanded_bytes = 0
    paths = sorted(root.rglob("*"), key=lambda path: path.relative_to(root).as_posix())
    for path in paths:
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            raise MirrorError("Cargo vendor tree contains a link: " + relative)
        raw_mode = path.stat().st_mode
        if path.is_dir():
            kind = "directory"
            executable = False
            size = 0
            content_hash = "0" * 64
        elif path.is_file():
            kind = "file"
            executable = bool(raw_mode & stat.S_IXUSR)
            content_hash, size = hash_file(path)
            expanded_bytes += size
            file_count += 1
        else:
            raise MirrorError("Cargo vendor tree contains an unsupported entry: " + relative)
        fields = [relative, kind, "1" if executable else "0", str(size), content_hash]
        for field in fields:
            encoded = field.encode("utf-8")
            digest.update(len(encoded).to_bytes(8, "big"))
            digest.update(encoded)
    return digest.hexdigest(), file_count, expanded_bytes


def write_canonical_vendor_archive(source: Path, output: Path) -> Tuple[str, int, int, int, str]:
    tree_hash, file_count, expanded_bytes = normalized_tree(source)
    with output.open("xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                for path in sorted(
                    source.rglob("*"), key=lambda item: item.relative_to(source).as_posix()
                ):
                    relative = path.relative_to(source).as_posix()
                    info = tarfile.TarInfo(relative)
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.mtime = 0
                    if path.is_dir():
                        info.type = tarfile.DIRTYPE
                        info.mode = 0o755
                        info.size = 0
                        archive.addfile(info)
                    elif path.is_file() and not path.is_symlink():
                        info.type = tarfile.REGTYPE
                        info.mode = 0o755 if path.stat().st_mode & stat.S_IXUSR else 0o644
                        info.size = path.stat().st_size
                        with path.open("rb") as handle:
                            archive.addfile(info, handle)
                    else:
                        raise MirrorError("unsupported Cargo vendor archive entry: " + relative)
    archive_hash, archive_bytes = hash_file(output)
    return archive_hash, archive_bytes, file_count, expanded_bytes, tree_hash


def run_cargo_vendor(
    policy: Mapping[str, Any], destination: Path, fetch_offline: bool, root: Path = ROOT
) -> None:
    workspaces = policy["cargo"]["workspaces"]
    root_workspace = next((item for item in workspaces if item["id"] == "root"), None)
    if root_workspace is None:
        raise MirrorError("root Cargo workspace is not declared")
    argv = [
        "cargo",
        "vendor",
        "--locked",
        "--quiet",
        "--versioned-dirs",
        "--manifest-path",
        root_workspace["manifest"],
    ]
    if fetch_offline:
        argv.append("--offline")
    for item in workspaces:
        if item["id"] != "root":
            argv.extend(["--sync", item["manifest"]])
    argv.append(str(destination))
    try:
        result = subprocess.run(
            argv,
            cwd=str(root),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=900,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise MirrorError("cargo vendor failed: " + str(exc)) from exc
    if result.returncode != 0:
        diagnostic = result.stderr[-65536:].decode("utf-8", errors="replace").strip()
        raise MirrorError("cargo vendor failed: " + diagnostic)


def prepare_mirror(
    policy_path: Path, store: Path, fetch_offline: bool, root: Path = ROOT
) -> Path:
    policy = validate_policy(policy_path, root)
    policy_hash = sha256_bytes(policy_path.read_bytes())
    cargo_packages: Dict[Tuple[str, str, str], Dict[str, str]] = {}
    for workspace in policy["cargo"]["workspaces"]:
        for package in parse_cargo_lock(root / workspace["lockfile"]):
            key = (package["name"], package["version"], package["checksum"])
            cargo_packages[key] = package
    npm_packages = parse_npm_lock(root / policy["npm"]["lockfile"], policy)

    store = store.resolve()
    store.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".staging-", dir=str(store)))
    try:
        vendor = staging / "expanded-vendor"
        run_cargo_vendor(policy, vendor, fetch_offline, root)
        cargo_dir = staging / "cargo"
        cargo_dir.mkdir()
        archive_path = cargo_dir / "vendor.tar.gz"
        archive_hash, archive_bytes, file_count, expanded_bytes, tree_hash = (
            write_canonical_vendor_archive(vendor, archive_path)
        )
        shutil.rmtree(vendor)
        if archive_bytes > policy["cargo"]["maxArchiveBytes"]:
            raise MirrorError("Cargo archive exceeds policy byte bound")
        if expanded_bytes > policy["cargo"]["maxExpandedBytes"]:
            raise MirrorError("Cargo vendor tree exceeds expanded byte bound")
        if file_count > policy["cargo"]["maxExpandedFiles"]:
            raise MirrorError("Cargo vendor tree exceeds file-count bound")

        npm_records: List[Dict[str, Any]] = []
        npm_total = 0
        fetched: Dict[str, Tuple[int, str]] = {}
        for package in npm_packages:
            relative = package["blob"]
            output = staging / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            digest = package["sha512"]
            if digest not in fetched:
                size = fetch_npm_package(
                    package["resolved"],
                    digest,
                    output,
                    policy["npm"]["maxPackageBytes"],
                )
                npm_total += size
                if npm_total > policy["npm"]["maxTotalBytes"]:
                    raise MirrorError("npm closure exceeds aggregate byte bound")
                payload_hash, _ = hash_file(output)
                fetched[digest] = (size, payload_hash)
            size, payload_hash = fetched[digest]
            npm_records.append(
                dict(package, bytes=size, sha256=payload_hash)
            )

        manifest = {
            "authorities": authority_records(policy, root),
            "cargo": {
                "archive": {
                    "bytes": archive_bytes,
                    "expandedBytes": expanded_bytes,
                    "expandedFiles": file_count,
                    "path": "cargo/vendor.tar.gz",
                    "sha256": archive_hash,
                    "treeSha256": tree_hash,
                },
                "packages": [cargo_packages[key] for key in sorted(cargo_packages)],
                "registrySource": CARGO_SOURCE,
            },
            "kind": MANIFEST_KIND,
            "npm": {"packages": npm_records, "totalBlobBytes": npm_total},
            "offlineChecks": list(policy["offlineChecks"]),
            "policySha256": policy_hash,
            "version": "0.1",
        }
        manifest_bytes = canonical_json(manifest)
        mirror_id = sha256_bytes(manifest_bytes)
        (staging / "manifest.json").write_bytes(manifest_bytes)
        destination = store / ("sha256-" + mirror_id)
        if destination.exists():
            existing = destination / "manifest.json"
            if not existing.is_file() or existing.read_bytes() != manifest_bytes:
                raise MirrorError("content-addressed mirror path collision: " + str(destination))
            verify_mirror(policy_path, destination, root=root)
            shutil.rmtree(staging)
            return destination
        staging.rename(destination)
        return destination
    except Exception:
        if staging.exists():
            shutil.rmtree(staging)
        raise


def validate_generated_manifest(value: Any) -> Dict[str, Any]:
    manifest = require_dict(value, "mirror manifest")
    exact_keys(
        manifest,
        ["authorities", "cargo", "kind", "npm", "offlineChecks", "policySha256", "version"],
        "mirror manifest",
    )
    if manifest["kind"] != MANIFEST_KIND or manifest["version"] != "0.1":
        raise MirrorError("unsupported mirror manifest kind/version")
    if not SHA256_RE.fullmatch(require_string(manifest["policySha256"], "policySha256")):
        raise MirrorError("invalid mirror policy hash")
    if sorted_unique_strings(manifest["offlineChecks"], "offlineChecks") != EXPECTED_OFFLINE_CHECKS:
        raise MirrorError("mirror offline checks differ from the closed profile")
    authorities = require_list(manifest["authorities"], "authorities")
    previous = ""
    for raw in authorities:
        item = require_dict(raw, "authority")
        exact_keys(item, ["bytes", "path", "sha256"], "authority")
        path = safe_relative_path(item["path"], "authority path")
        require_int(item["bytes"], "authority bytes")
        if not SHA256_RE.fullmatch(require_string(item["sha256"], "authority sha256")):
            raise MirrorError("invalid authority SHA-256")
        if path <= previous:
            raise MirrorError("mirror authorities must be sorted and unique")
        previous = path
    cargo = require_dict(manifest["cargo"], "cargo mirror")
    exact_keys(cargo, ["archive", "packages", "registrySource"], "cargo mirror")
    if cargo["registrySource"] != CARGO_SOURCE:
        raise MirrorError("mirror Cargo source differs from policy")
    archive = require_dict(cargo["archive"], "cargo archive")
    exact_keys(
        archive,
        ["bytes", "expandedBytes", "expandedFiles", "path", "sha256", "treeSha256"],
        "cargo archive",
    )
    if archive["path"] != "cargo/vendor.tar.gz":
        raise MirrorError("unexpected Cargo archive path")
    for key in ("bytes", "expandedBytes", "expandedFiles"):
        require_int(archive[key], "cargo archive " + key)
    for key in ("sha256", "treeSha256"):
        if not SHA256_RE.fullmatch(require_string(archive[key], "cargo archive " + key)):
            raise MirrorError("invalid Cargo archive hash")
    packages = require_list(cargo["packages"], "cargo packages")
    package_keys: List[Tuple[str, str, str]] = []
    for raw in packages:
        item = require_dict(raw, "Cargo package")
        exact_keys(item, ["checksum", "name", "source", "version"], "Cargo package")
        if item["source"] != CARGO_SOURCE or not SHA256_RE.fullmatch(item["checksum"]):
            raise MirrorError("invalid Cargo package source/checksum")
        package_keys.append((item["name"], item["version"], item["checksum"]))
    if package_keys != sorted(package_keys) or len(package_keys) != len(set(package_keys)):
        raise MirrorError("Cargo package identities must be sorted and unique")

    npm = require_dict(manifest["npm"], "npm mirror")
    exact_keys(npm, ["packages", "totalBlobBytes"], "npm mirror")
    require_int(npm["totalBlobBytes"], "npm totalBlobBytes")
    npm_packages = require_list(npm["packages"], "npm packages")
    npm_keys: List[str] = []
    total_by_digest: Dict[str, int] = {}
    for raw in npm_packages:
        item = require_dict(raw, "npm package")
        exact_keys(
            item,
            ["blob", "bytes", "integrity", "lockPath", "resolved", "sha256", "sha512"],
            "npm package",
        )
        safe_relative_path(item["blob"], "npm blob")
        require_int(item["bytes"], "npm package bytes")
        for key in ("sha256",):
            if not SHA256_RE.fullmatch(require_string(item[key], "npm " + key)):
                raise MirrorError("invalid npm SHA-256")
        digest = require_string(item["sha512"], "npm sha512")
        if not re.fullmatch(r"[0-9a-f]{128}", digest):
            raise MirrorError("invalid npm SHA-512")
        if item["blob"] != "npm/sha512/{0}.tgz".format(digest):
            raise MirrorError("npm blob path does not match SHA-512")
        npm_keys.append(require_string(item["lockPath"], "npm lockPath"))
        total_by_digest[digest] = item["bytes"]
    if npm_keys != sorted(npm_keys) or len(npm_keys) != len(set(npm_keys)):
        raise MirrorError("npm package lock paths must be sorted and unique")
    if sum(total_by_digest.values()) != npm["totalBlobBytes"]:
        raise MirrorError("npm aggregate byte count mismatch")
    return manifest


def safe_extract_vendor(archive_path: Path, destination: Path, policy: Mapping[str, Any]) -> None:
    max_archive = policy["cargo"]["maxArchiveBytes"]
    digest, archive_bytes = hash_file(archive_path, max_archive)
    del digest
    if archive_bytes < 1:
        raise MirrorError("Cargo archive is empty")
    destination.mkdir(parents=True, exist_ok=False)
    seen: Set[str] = set()
    expanded_bytes = 0
    file_count = 0
    try:
        with tarfile.open(archive_path, mode="r:gz") as archive:
            for member in archive:
                name = safe_relative_path(member.name, "Cargo archive member")
                if name in seen:
                    raise MirrorError("duplicate Cargo archive member: " + name)
                seen.add(name)
                if member.uid != 0 or member.gid != 0 or member.mtime != 0:
                    raise MirrorError("Cargo archive metadata is not canonical: " + name)
                if member.uname or member.gname:
                    raise MirrorError("Cargo archive owner names are not canonical: " + name)
                target = destination / name
                try:
                    target.parent.resolve().relative_to(destination.resolve())
                except ValueError as exc:
                    raise MirrorError("Cargo archive member escapes destination: " + name) from exc
                if member.isdir():
                    if member.mode != 0o755:
                        raise MirrorError("Cargo archive directory mode is not canonical: " + name)
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                if not member.isfile() or member.mode not in (0o644, 0o755):
                    raise MirrorError("unsupported Cargo archive entry: " + name)
                file_count += 1
                expanded_bytes += member.size
                if file_count > policy["cargo"]["maxExpandedFiles"]:
                    raise MirrorError("Cargo archive exceeds file-count bound")
                if expanded_bytes > policy["cargo"]["maxExpandedBytes"]:
                    raise MirrorError("Cargo archive exceeds expanded byte bound")
                source = archive.extractfile(member)
                if source is None:
                    raise MirrorError("Cargo archive file has no body: " + name)
                target.parent.mkdir(parents=True, exist_ok=True)
                with target.open("xb") as output:
                    remaining = member.size
                    while remaining:
                        chunk = source.read(min(1024 * 1024, remaining))
                        if not chunk:
                            raise MirrorError("truncated Cargo archive member: " + name)
                        output.write(chunk)
                        remaining -= len(chunk)
                    if source.read(1):
                        raise MirrorError("Cargo archive member exceeds declared size: " + name)
                target.chmod(member.mode)
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise


def verify_mirror(
    policy_path: Path,
    mirror: Path,
    extraction: Optional[Path] = None,
    require_authorities: bool = True,
    root: Path = ROOT,
) -> Dict[str, Any]:
    policy = validate_policy(policy_path, root)
    mirror = mirror.resolve()
    manifest_path = mirror / "manifest.json"
    manifest_raw = manifest_path.read_bytes()
    manifest = validate_generated_manifest(load_json(manifest_path))
    if manifest_raw != canonical_json(manifest):
        raise MirrorError("mirror manifest is not canonical JSON")
    mirror_id = sha256_bytes(manifest_raw)
    if mirror.name != "sha256-" + mirror_id:
        raise MirrorError("mirror directory name does not match manifest identity")
    if manifest["policySha256"] != sha256_bytes(policy_path.read_bytes()):
        raise MirrorError("mirror policy hash differs from current policy")
    if require_authorities and manifest["authorities"] != authority_records(policy, root):
        raise MirrorError("mirror authority hashes differ from current source")

    archive_record = manifest["cargo"]["archive"]
    archive_path = mirror / archive_record["path"]
    digest, size = hash_file(archive_path, policy["cargo"]["maxArchiveBytes"])
    if digest != archive_record["sha256"] or size != archive_record["bytes"]:
        raise MirrorError("Cargo archive payload identity mismatch")

    seen_npm: Set[str] = set()
    for item in manifest["npm"]["packages"]:
        if item["sha512"] in seen_npm:
            continue
        seen_npm.add(item["sha512"])
        blob = mirror / item["blob"]
        blob_sha256, blob_size = hash_file(blob, policy["npm"]["maxPackageBytes"])
        if blob_sha256 != item["sha256"] or blob_size != item["bytes"]:
            raise MirrorError("npm blob SHA-256/size mismatch: " + item["lockPath"])
        digest512, _ = hash_file_with(blob, "sha512", policy["npm"]["maxPackageBytes"])
        if digest512 != item["sha512"]:
            raise MirrorError("npm blob SHA-512 mismatch: " + item["lockPath"])

    owns_extraction = extraction is None
    if extraction is None:
        extraction = Path(tempfile.mkdtemp(prefix="genesis-mirror-verify.")) / "vendor"
    try:
        safe_extract_vendor(archive_path, extraction, policy)
        tree_hash, file_count, expanded_bytes = normalized_tree(extraction)
        if tree_hash != archive_record["treeSha256"]:
            raise MirrorError("Cargo vendor logical tree identity mismatch")
        if file_count != archive_record["expandedFiles"]:
            raise MirrorError("Cargo vendor expanded file count mismatch")
        if expanded_bytes != archive_record["expandedBytes"]:
            raise MirrorError("Cargo vendor expanded byte count mismatch")
    finally:
        if owns_extraction:
            shutil.rmtree(extraction.parent, ignore_errors=True)
    return manifest


def network_guard_prefix() -> Tuple[str, List[str]]:
    if sys.platform == "darwin":
        executable = shutil.which("sandbox-exec")
        if executable is None:
            raise MirrorError("Darwin network isolation backend is unavailable")
        return "sandbox-exec", [
            executable,
            "-p",
            "(version 1)(allow default)(deny network*)",
        ]
    if sys.platform.startswith("linux"):
        unshare = shutil.which("unshare")
        if unshare is None:
            raise MirrorError("Linux network namespace backend is unavailable")
        unprivileged = subprocess.run(
            [unshare, "--user", "--map-root-user", "--net", "--", "true"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if unprivileged.returncode == 0:
            return "unshare-user-net", [unshare, "--user", "--map-root-user", "--net", "--"]
        sudo = shutil.which("sudo")
        if sudo is not None:
            allowed = subprocess.run(
                [sudo, "-n", "true"],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            if allowed.returncode == 0:
                return "unshare-sudo-net", [
                    sudo,
                    "-n",
                    unshare,
                    "--net",
                    "--setuid",
                    str(os.getuid()),
                    "--setgid",
                    str(os.getgid()),
                    "--",
                ]
        raise MirrorError("Linux network namespace creation is not permitted")
    raise MirrorError("hard network isolation is unsupported on this host")


def prove_network_denial(prefix: Sequence[str]) -> None:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(4)
    listener.settimeout(0.2)
    stop = threading.Event()

    def serve() -> None:
        while not stop.is_set():
            try:
                peer, _ = listener.accept()
                peer.close()
            except socket.timeout:
                continue
            except OSError:
                return

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    port = listener.getsockname()[1]
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=1):
            pass
        code = (
            "import socket,sys\n"
            "try:\n socket.create_connection(('127.0.0.1',int(sys.argv[1])),1)\n"
            "except OSError as e:\n print('DENIED:'+str(getattr(e,'errno',None)))\n sys.exit(17)\n"
            "sys.exit(0)\n"
        )
        result = subprocess.run(
            list(prefix) + [sys.executable, "-c", code, str(port)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
            check=False,
        )
        output = result.stdout.decode("utf-8", errors="replace").strip()
        if result.returncode != 17 or not output.startswith("DENIED:"):
            raise MirrorError("network isolation canary did not fail at connect")
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise MirrorError("network isolation canary was inconclusive: " + str(exc)) from exc
    finally:
        stop.set()
        listener.close()
        thread.join(timeout=1)


def source_inventory(root: Path) -> List[str]:
    try:
        result = subprocess.run(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
            cwd=str(root),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise MirrorError("cannot enumerate clean source materialization: " + str(exc)) from exc
    if result.returncode != 0:
        raise MirrorError("git source inventory failed")
    paths = []
    for raw in result.stdout.split(b"\0"):
        if not raw:
            continue
        try:
            relative = raw.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise MirrorError("source inventory contains a non-UTF-8 path") from exc
        relative = safe_relative_path(relative, "source inventory path")
        if relative.split("/", 1)[0] in EXCLUDED_SOURCE_TOP_LEVEL:
            raise MirrorError("excluded generated path entered source inventory: " + relative)
        path = root / relative
        if path.exists():
            if path.is_symlink() or not path.is_file():
                raise MirrorError("source materialization accepts regular files only: " + relative)
            paths.append(relative)
    return sorted(set(paths))


def materialize_source(root: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=False)
    for relative in source_inventory(root):
        source = root / relative
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        target.chmod(0o755 if source.stat().st_mode & stat.S_IXUSR else 0o644)


def write_cargo_source_config(source: Path, vendor: Path) -> None:
    config = source / ".cargo" / "config.toml"
    config.parent.mkdir(parents=True, exist_ok=True)
    existing = config.read_text(encoding="utf-8") if config.exists() else ""
    if "[source.crates-io]" in existing or "[source.genesis-vendored-sources]" in existing:
        raise MirrorError("source Cargo config already defines dependency source replacement")
    escaped = str(vendor.resolve()).replace("\\", "\\\\").replace('"', '\\"')
    addition = (
        "\n[source.crates-io]\n"
        'replace-with = "genesis-vendored-sources"\n\n'
        "[source.genesis-vendored-sources]\n"
        'directory = "{0}"\n'.format(escaped)
    )
    config.write_text(existing.rstrip() + "\n" + addition, encoding="utf-8")


def rewrite_npm_lock(source: Path, mirror: Path, manifest: Mapping[str, Any]) -> None:
    lock_path = source / "package-lock.json"
    lock = require_dict(load_json(lock_path), "materialized npm lock")
    packages = require_dict(lock.get("packages"), "materialized npm packages")
    by_path = {item["lockPath"]: item for item in manifest["npm"]["packages"]}
    observed: Set[str] = set()
    for lock_key, raw in packages.items():
        item = require_dict(raw, "materialized npm package")
        if "resolved" not in item:
            continue
        if lock_key not in by_path:
            raise MirrorError("npm lock contains an unmirrored package: " + lock_key)
        record = by_path[lock_key]
        if item.get("resolved") != record["resolved"] or item.get("integrity") != record["integrity"]:
            raise MirrorError("npm authority drift during offline materialization: " + lock_key)
        item["resolved"] = (mirror / record["blob"]).resolve().as_uri()
        observed.add(lock_key)
    if observed != set(by_path):
        raise MirrorError("mirror contains npm packages absent from the source lock")
    lock_path.write_bytes(json.dumps(lock, ensure_ascii=True, indent=2).encode("utf-8") + b"\n")


def offline_environment(work: Path) -> Dict[str, str]:
    env: Dict[str, str] = {}
    for key in ("DEVELOPER_DIR", "MACOSX_DEPLOYMENT_TARGET", "SDKROOT"):
        if key in os.environ:
            env[key] = os.environ[key]
    original_home = Path.home()
    home = work / "home"
    cargo_home = work / "cargo-home"
    npm_cache = work / "npm-cache"
    for path in (home, cargo_home, npm_cache):
        path.mkdir(parents=True, exist_ok=True)
    rustup_home = Path(os.environ.get("RUSTUP_HOME", str(original_home / ".rustup"))).resolve()
    env.update(
        {
            "CARGO_HOME": str(cargo_home),
            "CARGO_NET_OFFLINE": "true",
            "CARGO_TARGET_DIR": str(work / "target"),
            "GIT_CONFIG_NOSYSTEM": "1",
            "HOME": str(home),
            "HTTPS_PROXY": "http://127.0.0.1:9",
            "HTTP_PROXY": "http://127.0.0.1:9",
            "LANG": "C",
            "LC_ALL": "C",
            "NPM_CONFIG_AUDIT": "false",
            "NPM_CONFIG_CACHE": str(npm_cache),
            "NPM_CONFIG_FUND": "false",
            "NPM_CONFIG_OFFLINE": "true",
            "NPM_CONFIG_UPDATE_NOTIFIER": "false",
            "NO_PROXY": "",
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1",
            "RUSTUP_HOME": str(rustup_home),
            "TMPDIR": str(work / "tmp"),
        }
    )
    (work / "tmp").mkdir(parents=True, exist_ok=True)
    return env


def run_guarded(
    prefix: Sequence[str], argv: Sequence[str], cwd: Path, env: Mapping[str, str], timeout: int
) -> None:
    resolved = shutil.which(argv[0], path=env.get("PATH"))
    if resolved is None:
        raise MirrorError("offline command is unavailable: " + argv[0])
    command = [resolved] + list(argv[1:])
    guarded_prefix = list(prefix)
    process_env = dict(env)
    if len(guarded_prefix) >= 3 and Path(guarded_prefix[0]).name == "sudo":
        env_tool = shutil.which("env") or "/usr/bin/env"
        guarded_prefix.extend(
            [env_tool, "-i"] + ["{0}={1}".format(key, env[key]) for key in sorted(env)]
        )
        process_env = {"LANG": "C", "PATH": "/usr/bin:/bin"}
    print("offline-dependency-mirror: run " + " ".join(argv), flush=True)
    try:
        result = subprocess.run(
            guarded_prefix + command,
            cwd=str(cwd),
            env=process_env,
            stdin=subprocess.DEVNULL,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise MirrorError("offline command failed to execute: " + str(exc)) from exc
    if result.returncode != 0:
        raise MirrorError(
            "offline command failed with exit {0}: {1}".format(result.returncode, " ".join(argv))
        )


def offline_test(policy_path: Path, mirror: Path, root: Path = ROOT) -> str:
    policy = validate_policy(policy_path, root)
    backend, prefix = network_guard_prefix()
    prove_network_denial(prefix)
    authority_before = canonical_json(authority_records(policy, root))
    mirror_before = normalized_tree(mirror.resolve())
    with tempfile.TemporaryDirectory(prefix="genesis-offline-dependencies.") as raw_work:
        work = Path(raw_work)
        vendor = work / "vendor"
        manifest = verify_mirror(policy_path, mirror, extraction=vendor, root=root)
        source = work / "source"
        materialize_source(root, source)
        write_cargo_source_config(source, vendor)
        rewrite_npm_lock(source, mirror.resolve(), manifest)
        env = offline_environment(work)
        commands = {
            "cargo-cli-build": ["cargo", "build", "-p", "gc_cli", "--locked", "--offline"],
            "cargo-evidence-verifier-check": [
                "cargo",
                "check",
                "--manifest-path",
                "tools/genesis-evidence-verifier/Cargo.toml",
                "--all-targets",
                "--locked",
                "--offline",
            ],
            "cargo-workspace-check": [
                "cargo",
                "check",
                "--workspace",
                "--all-targets",
                "--locked",
                "--offline",
            ],
            "npm-clean-install": [
                "npm",
                "ci",
                "--offline",
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
            ],
            "npm-playwright-import": [
                "node",
                "-e",
                "import('playwright').then(() => process.stdout.write('playwright-offline-ok\\n'))",
            ],
        }
        for check_id in policy["offlineChecks"]:
            timeout = 1800 if check_id.startswith("cargo-") else 300
            run_guarded(prefix, commands[check_id], source, env, timeout)
    if authority_before != canonical_json(authority_records(policy, root)):
        raise MirrorError("offline test mutated retained dependency authorities")
    if mirror_before != normalized_tree(mirror.resolve()):
        raise MirrorError("offline test mutated retained mirror payloads")
    return backend


def self_test(policy_path: Path, root: Path = ROOT) -> None:
    policy = validate_policy(policy_path, root)
    cargo_count = 0
    for workspace in policy["cargo"]["workspaces"]:
        cargo_count += len(parse_cargo_lock(root / workspace["lockfile"]))
    npm_count = len(parse_npm_lock(root / policy["npm"]["lockfile"], policy))
    if cargo_count < 1 or npm_count < 1:
        raise MirrorError("dependency closure self-test found no packages")
    _, prefix = network_guard_prefix()
    prove_network_denial(prefix)
    try:
        prove_network_denial([])
    except MirrorError:
        pass
    else:
        raise MirrorError("network canary accepted an unisolated command")
    first = canonical_json(policy)
    second = canonical_json(require_dict(json.loads(first), "round-trip policy"))
    if first != second:
        raise MirrorError("canonical policy rendering is not deterministic")
    for alias in ("../escape", "./alias", "a//b", "/absolute", "a\\b"):
        try:
            safe_relative_path(alias, "self-test path")
        except MirrorError:
            pass
        else:
            raise MirrorError("path alias self-test was accepted: " + alias)
    with tempfile.TemporaryDirectory(prefix="genesis-mirror-self-test.") as raw_temp:
        temp = Path(raw_temp)
        duplicate = temp / "duplicate.json"
        duplicate.write_text('{"a":1,"a":2}\n', encoding="utf-8")
        try:
            load_json(duplicate)
        except MirrorError:
            pass
        else:
            raise MirrorError("duplicate-key JSON self-test was accepted")
        tree = temp / "tree"
        (tree / "crate-1.0.0").mkdir(parents=True)
        sample = tree / "crate-1.0.0" / "sample.txt"
        sample.write_bytes(b"canonical dependency bytes\n")
        (tree / "crate-1.0.0" / "second.txt").write_bytes(b"second dependency file\n")
        archive_a = temp / "a.tar.gz"
        archive_b = temp / "b.tar.gz"
        identity_a = write_canonical_vendor_archive(tree, archive_a)
        identity_b = write_canonical_vendor_archive(tree, archive_b)
        if identity_a != identity_b or archive_a.read_bytes() != archive_b.read_bytes():
            raise MirrorError("canonical Cargo archive is not byte-deterministic")
        malicious = temp / "malicious.tar.gz"
        with tarfile.open(malicious, mode="w:gz") as archive:
            info = tarfile.TarInfo("../escape")
            info.size = 1
            archive.addfile(info, io.BytesIO(b"x"))
        try:
            safe_extract_vendor(malicious, temp / "malicious-out", policy)
        except MirrorError:
            pass
        else:
            raise MirrorError("archive traversal self-test was accepted")
        linked = temp / "linked.tar.gz"
        with tarfile.open(linked, mode="w:gz") as archive:
            info = tarfile.TarInfo("crate/link")
            info.type = tarfile.SYMTYPE
            info.linkname = "../../escape"
            archive.addfile(info)
        try:
            safe_extract_vendor(linked, temp / "linked-out", policy)
        except MirrorError:
            pass
        else:
            raise MirrorError("archive link self-test was accepted")
        duplicated = temp / "duplicated.tar.gz"
        with tarfile.open(duplicated, mode="w:gz", format=tarfile.USTAR_FORMAT) as archive:
            for _ in range(2):
                info = tarfile.TarInfo("crate/file")
                info.size = 1
                info.mode = 0o644
                archive.addfile(info, io.BytesIO(b"x"))
        try:
            safe_extract_vendor(duplicated, temp / "duplicated-out", policy)
        except MirrorError:
            pass
        else:
            raise MirrorError("duplicate archive member self-test was accepted")
        bounded = copy.deepcopy(policy)
        bounded["cargo"]["maxExpandedFiles"] = 1
        try:
            safe_extract_vendor(archive_a, temp / "bounded-out", bounded)
        except MirrorError:
            pass
        else:
            raise MirrorError("archive file-count bound self-test was accepted")

        zero256 = "0" * 64
        zero512 = "0" * 128
        synthetic_manifest = {
            "authorities": [{"bytes": 1, "path": "Cargo.lock", "sha256": zero256}],
            "cargo": {
                "archive": {
                    "bytes": 1,
                    "expandedBytes": 1,
                    "expandedFiles": 1,
                    "path": "cargo/vendor.tar.gz",
                    "sha256": zero256,
                    "treeSha256": zero256,
                },
                "packages": [
                    {
                        "checksum": zero256,
                        "name": "crate",
                        "source": CARGO_SOURCE,
                        "version": "1.0.0",
                    }
                ],
                "registrySource": CARGO_SOURCE,
            },
            "kind": MANIFEST_KIND,
            "npm": {
                "packages": [
                    {
                        "blob": "npm/sha512/{0}.tgz".format(zero512),
                        "bytes": 1,
                        "integrity": "sha512-" + base64.b64encode(bytes(64)).decode("ascii"),
                        "lockPath": "node_modules/package",
                        "resolved": "https://registry.npmjs.org/package/-/package-1.0.0.tgz",
                        "sha256": zero256,
                        "sha512": zero512,
                    }
                ],
                "totalBlobBytes": 1,
            },
            "offlineChecks": list(EXPECTED_OFFLINE_CHECKS),
            "policySha256": zero256,
            "version": "0.1",
        }
        validate_generated_manifest(synthetic_manifest)
        manifest_mutations = []
        unknown = copy.deepcopy(synthetic_manifest)
        unknown["trustMe"] = True
        manifest_mutations.append(unknown)
        wrong_blob = copy.deepcopy(synthetic_manifest)
        wrong_blob["npm"]["packages"][0]["blob"] = "npm/sha512/" + ("1" * 128) + ".tgz"
        manifest_mutations.append(wrong_blob)
        incomplete = copy.deepcopy(synthetic_manifest)
        incomplete["offlineChecks"].pop()
        manifest_mutations.append(incomplete)
        for mutation in manifest_mutations:
            try:
                validate_generated_manifest(mutation)
            except MirrorError:
                pass
            else:
                raise MirrorError("generated-manifest adversarial self-test was accepted")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--policy", type=Path)
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("validate")
    subcommands.add_parser("self-test")
    prepare = subcommands.add_parser("prepare")
    prepare.add_argument("--store", type=Path)
    prepare.add_argument("--fetch-offline", action="store_true")
    prepare.add_argument("--format", choices=["human", "path", "json"], default="human")
    verify = subcommands.add_parser("verify")
    verify.add_argument("--mirror", type=Path, required=True)
    offline = subcommands.add_parser("offline-test")
    offline.add_argument("--mirror", type=Path, required=True)
    args = parser.parse_args(argv)
    source_root = args.source_root.resolve()
    policy_path = (args.policy or (source_root / "genesis.dependency-mirror.json")).resolve()
    try:
        if args.command == "validate":
            policy = validate_policy(policy_path, source_root)
            cargo_packages = sum(
                len(parse_cargo_lock(source_root / item["lockfile"]))
                for item in policy["cargo"]["workspaces"]
            )
            npm_packages = len(parse_npm_lock(source_root / policy["npm"]["lockfile"], policy))
            print(
                "dependency-mirror-policy: ok (authorities={0} cargo_packages={1} npm_packages={2})".format(
                    len(policy["authorityFiles"]), cargo_packages, npm_packages
                )
            )
        elif args.command == "self-test":
            self_test(policy_path, source_root)
            print("dependency-mirror-self-test: ok")
        elif args.command == "prepare":
            policy = validate_policy(policy_path, source_root)
            store = args.store or (source_root / policy["mirror"]["root"])
            mirror = prepare_mirror(policy_path, store, args.fetch_offline, source_root)
            default_store = (source_root / policy["mirror"]["root"]).resolve()
            if Path(store).resolve() == default_store:
                deterministic_cleanup.initialize_root_marker(
                    source_root,
                    policy["mirror"]["root"],
                    "dependency-mirror",
                )
            mirror_id = mirror.name[7:]
            if args.format == "path":
                print(mirror)
            elif args.format == "json":
                print(
                    json.dumps(
                        {"kind": "genesis/dependency-mirror-prepare-result-v0.1", "id": mirror_id, "path": str(mirror)},
                        separators=(",", ":"),
                        sort_keys=True,
                    )
                )
            else:
                print("dependency-mirror-prepare: ok id={0} path={1}".format(mirror_id, mirror))
        elif args.command == "verify":
            manifest = verify_mirror(policy_path, args.mirror, root=source_root)
            print(
                "dependency-mirror-verify: ok (cargo_packages={0} npm_packages={1})".format(
                    len(manifest["cargo"]["packages"]), len(manifest["npm"]["packages"])
                )
            )
        elif args.command == "offline-test":
            backend = offline_test(policy_path, args.mirror, source_root)
            print("offline-dependency-mirror: ok (network_backend={0} checks=5)".format(backend))
        else:
            raise MirrorError("unsupported command")
    except (MirrorError, OSError) as exc:
        print("dependency-mirror: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
