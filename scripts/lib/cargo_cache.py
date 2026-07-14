#!/usr/bin/env python3
"""Resolve GenesisCode Cargo caches from semantic build configuration."""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
import os
from pathlib import Path, PurePosixPath
import re
import shlex
import subprocess
import sys
import tempfile
from typing import Any, Mapping, Sequence

import deterministic_cleanup
import generated_state


POLICY_REL = "policies/cargo_cache_v0.1.json"
SCHEMA_REL = "docs/spec/CARGO_CACHE_POLICY_v0.1.schema.json"
EXPECTED_POLICY_FIELDS = {
    "kind", "version", "strategyVersion", "keyAlgorithm", "cacheRoot",
    "metadataFile", "sourceInputs", "buildEnvironment", "scopes",
}
EXPECTED_SCOPE_FIELDS = {"id", "workspace", "target", "manifestGlobs"}
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
SAFE_ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
SAFE_TARGET_RE = re.compile(r"^[A-Za-z0-9_.-]+$")
ENV_RE = re.compile(r"^[A-Z][A-Z0-9_]+$")


class CachePolicyError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CachePolicyError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys)
    except FileNotFoundError as exc:
        raise CachePolicyError(f"missing file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise CachePolicyError(f"invalid JSON in {path}:{exc.lineno}:{exc.colno}: {exc.msg}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def pretty_bytes(value: Any) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


def digest_bytes(value: bytes) -> str:
    return sha256(value).hexdigest()


def digest_file(path: Path) -> str:
    return digest_bytes(path.read_bytes())


def repo_path(value: str, field: str) -> PurePosixPath:
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or "\\" in value or not value:
        raise CachePolicyError(f"{field} must be a canonical repository-relative path: {value!r}")
    return path


def load_policy(root: Path, policy_path: Path | None = None) -> dict[str, Any]:
    path = policy_path or root / POLICY_REL
    policy = load_json(path)
    if not isinstance(policy, dict) or set(policy) != EXPECTED_POLICY_FIELDS:
        raise CachePolicyError("policy top-level fields mismatch")
    if policy["kind"] != "genesis/cargo-cache-policy-v0.1" or policy["version"] != "0.1":
        raise CachePolicyError("policy identity mismatch")
    if policy["strategyVersion"] != 1 or policy["keyAlgorithm"] != "sha256-canonical-json":
        raise CachePolicyError("unsupported cache strategy")
    if policy["cacheRoot"] != ".genesis/build/cargo-cache/v1":
        raise CachePolicyError("cache root drift")
    if policy["metadataFile"] != ".genesis-cargo-cache-key.json":
        raise CachePolicyError("metadata filename drift")
    if policy["sourceInputs"] != "cargo-fingerprinted-not-cache-addressed":
        raise CachePolicyError("source input policy drift")
    env = policy["buildEnvironment"]
    if not isinstance(env, list) or not env or env != sorted(set(env)) or any(not isinstance(x, str) or not ENV_RE.fullmatch(x) for x in env):
        raise CachePolicyError("buildEnvironment must be a sorted unique environment-variable list")
    scopes = policy["scopes"]
    if not isinstance(scopes, list) or not scopes:
        raise CachePolicyError("scopes must be a non-empty list")
    ids: list[str] = []
    pairs: list[tuple[str, str]] = []
    for scope in scopes:
        if not isinstance(scope, dict) or set(scope) != EXPECTED_SCOPE_FIELDS:
            raise CachePolicyError("scope fields mismatch")
        sid, workspace, target = scope["id"], scope["workspace"], scope["target"]
        if not isinstance(sid, str) or not SAFE_ID_RE.fullmatch(sid):
            raise CachePolicyError(f"invalid scope id: {sid!r}")
        if workspace != ".":
            repo_path(workspace, f"scope {sid} workspace")
        if not isinstance(target, str) or (target != "host" and not SAFE_TARGET_RE.fullmatch(target)):
            raise CachePolicyError(f"invalid scope target: {target!r}")
        globs = scope["manifestGlobs"]
        if not isinstance(globs, list) or not globs or globs != sorted(set(globs)):
            raise CachePolicyError(f"scope {sid} manifestGlobs must be sorted and unique")
        for pattern in globs:
            if not isinstance(pattern, str):
                raise CachePolicyError(f"scope {sid} has a non-string manifest glob")
            repo_path(pattern, f"scope {sid} manifest glob")
            if not pattern.endswith("Cargo.toml"):
                raise CachePolicyError(f"scope {sid} manifest glob must select Cargo.toml")
        ids.append(sid)
        pairs.append((workspace, target))
    if ids != sorted(set(ids)):
        raise CachePolicyError("scope ids must be sorted and unique")
    if len(pairs) != len(set(pairs)):
        raise CachePolicyError("workspace/target pairs must be unique")
    return policy


def rustc_identity(root: Path, environ: Mapping[str, str]) -> dict[str, str]:
    override = environ.get("GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON")
    if override:
        try:
            raw = json.loads(override, object_pairs_hook=reject_duplicate_keys)
        except (json.JSONDecodeError, CachePolicyError) as exc:
            raise CachePolicyError(f"invalid GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON: {exc}") from exc
        required = {"release", "commit-hash", "host"}
        if not isinstance(raw, dict) or set(raw) != required or any(not isinstance(raw[k], str) or not raw[k] for k in required):
            raise CachePolicyError("mock rustc identity fields mismatch")
        return {k: raw[k] for k in sorted(required)}
    try:
        proc = subprocess.run(
            ["rustc", "-vV"], cwd=root, env=dict(environ), text=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise CachePolicyError(f"unable to identify rustc: {exc}") from exc
    if proc.returncode != 0:
        raise CachePolicyError(f"rustc -vV failed: {proc.stderr.strip()}")
    fields: dict[str, str] = {}
    for line in proc.stdout.splitlines():
        if ": " in line:
            key, value = line.split(": ", 1)
            fields[key] = value
    required = ("release", "commit-hash", "host")
    if any(not fields.get(k) for k in required):
        raise CachePolicyError("rustc -vV omitted release, commit-hash, or host")
    return {k: fields[k] for k in sorted(required)}


def workspace_manifests(root: Path, patterns: Sequence[str]) -> list[Path]:
    paths: set[Path] = set()
    for pattern in patterns:
        matches = [path for path in root.glob(pattern) if path.is_file()]
        if not matches:
            raise CachePolicyError(f"manifest glob matched no files: {pattern}")
        paths.update(matches)
    return sorted(paths)


def relative(root: Path, path: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as exc:
        raise CachePolicyError(f"cache input escaped repository: {path}") from exc


def definition_digest(root: Path, manifests: Sequence[Path], domain: str) -> str:
    # Cargo manifests are the authority for both feature and profile definitions.
    # Domain separation keeps these identities explicit without requiring a TOML
    # parser in every bootstrap environment.
    identity = {
        "domain": domain,
        "manifests": [
            {"path": relative(root, path), "sha256": digest_file(path)}
            for path in manifests
        ],
    }
    return digest_bytes(canonical_bytes(identity))


def resolve(
    root: Path,
    scope_id: str,
    environ: Mapping[str, str] | None = None,
    policy_path: Path | None = None,
) -> dict[str, Any]:
    root = root.resolve()
    env = dict(os.environ if environ is None else environ)
    policy = load_policy(root, policy_path)
    by_id = {scope["id"]: scope for scope in policy["scopes"]}
    if scope_id not in by_id:
        raise CachePolicyError(f"undeclared cache scope: {scope_id}")
    scope = by_id[scope_id]
    rustc = rustc_identity(root, env)
    target = rustc["host"] if scope["target"] == "host" else scope["target"]
    manifests = workspace_manifests(root, scope["manifestGlobs"])
    workspace = root if scope["workspace"] == "." else root / scope["workspace"]
    inputs = set(manifests)
    for path in (workspace / "Cargo.lock", root / "rust-toolchain.toml", root / ".cargo/config.toml"):
        if not path.is_file():
            raise CachePolicyError(f"missing cache configuration input: {path}")
        inputs.add(path)
    feature_sha = definition_digest(root, manifests, "cargo-feature-definitions-v1")
    profile_sha = definition_digest(root, manifests, "cargo-profile-definitions-v1")
    key = {
        "buildEnvironment": {name: env.get(name) for name in policy["buildEnvironment"]},
        "configurationInputs": [
            {"path": relative(root, path), "sha256": digest_file(path)} for path in sorted(inputs)
        ],
        "featureDefinitionsSha256": feature_sha,
        "kind": "genesis/cargo-cache-key-v0.1",
        "profileDefinitionsSha256": profile_sha,
        "rustc": rustc,
        "scope": scope_id,
        "strategyVersion": policy["strategyVersion"],
        "target": target,
        "workspace": scope["workspace"],
    }
    digest = digest_bytes(canonical_bytes(key))
    cache_root_raw = env.get("GENESIS_CARGO_CACHE_ROOT", policy["cacheRoot"])
    cache_root = Path(cache_root_raw)
    if not cache_root.is_absolute():
        cache_root = root / cache_root
    cache_root = cache_root.resolve()
    if cache_root == root or root in cache_root.parents and cache_root.relative_to(root).parts[:1] not in ((".genesis",),):
        raise CachePolicyError("cache root inside the repository must be below .genesis")
    workspace_slug = "root" if scope["workspace"] == "." else scope["workspace"].replace("/", "-")
    target_slug = target.replace("/", "-")
    target_dir = cache_root / workspace_slug / target_slug / digest
    metadata = {
        "cacheKey": key,
        "cacheKeySha256": digest,
        "kind": "genesis/cargo-cache-materialization-v0.1",
        "policySha256": digest_file(policy_path or root / POLICY_REL),
        "version": "0.1",
    }
    return {"target_dir": target_dir, "metadata": metadata, "metadata_file": policy["metadataFile"]}


def materialize(result: Mapping[str, Any]) -> bool:
    target = Path(result["target_dir"])
    target.mkdir(parents=True, exist_ok=True)
    metadata_path = target / str(result["metadata_file"])
    expected = pretty_bytes(result["metadata"])
    if metadata_path.exists():
        observed = metadata_path.read_bytes()
        if observed != expected:
            raise CachePolicyError(f"cache metadata mismatch: {metadata_path}")
        return True
    fd, tmp_name = tempfile.mkstemp(prefix=f".{metadata_path.name}.", dir=target)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(expected)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp_name, metadata_path)
    finally:
        try:
            os.unlink(tmp_name)
        except FileNotFoundError:
            pass
    return False


def emit(result: Mapping[str, Any], output_format: str) -> None:
    target = str(result["target_dir"])
    digest = result["metadata"]["cacheKeySha256"]
    scope = result["metadata"]["cacheKey"]["scope"]
    if output_format == "path":
        print(target)
    elif output_format == "json":
        print(pretty_bytes({"cacheKeySha256": digest, "scope": scope, "targetDir": target}).decode(), end="")
    elif output_format == "shell":
        print(f"export CARGO_TARGET_DIR={shlex.quote(target)}")
        print("export GENESIS_CARGO_CACHE_RESOLVED=1")
        print(f"export GENESIS_CARGO_CACHE_SCOPE={shlex.quote(scope)}")
        print(f"export GENESIS_CARGO_CACHE_KEY_SHA256={shlex.quote(digest)}")
        print(f"export GENESIS_CARGO_CACHE_HIT={1 if result.get('cache_hit') else 0}")
        if result.get("generated_state"):
            print(f"export GENESIS_GENERATED_STATE_ROOT={shlex.quote(str(result['root']))}")
            print(
                "export GENESIS_GENERATED_STATE_LEASE_TOKEN="
                + shlex.quote(str(result["generated_state"]["leaseToken"]))
            )
            print(
                "export GENESIS_GENERATED_STATE_LEASE_PID="
                + shlex.quote(str(result["generated_state"]["leasePid"]))
            )
    elif output_format == "github-env":
        print(f"CARGO_TARGET_DIR={target}")
        print("GENESIS_CARGO_CACHE_RESOLVED=1")
        print(f"GENESIS_CARGO_CACHE_SCOPE={scope}")
        print(f"GENESIS_CARGO_CACHE_KEY_SHA256={digest}")
    else:  # pragma: no cover - argparse closes this boundary
        raise CachePolicyError(f"unsupported output format: {output_format}")


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--format", choices=("path", "json", "shell", "github-env"), default="path")
    parser.add_argument("--no-materialize", action="store_true")
    parser.add_argument("--lease-pid", type=int)
    args = parser.parse_args(argv)
    try:
        result = resolve(args.root, args.scope)
        result["root"] = args.root.resolve()
        result["cache_hit"] = False
        if not args.no_materialize:
            target = Path(result["target_dir"]).resolve()
            build_root = args.root.resolve() / ".genesis/build"
            try:
                target.relative_to(build_root)
            except ValueError:
                relative_target = None
            else:
                relative_target = target.relative_to(args.root.resolve()).as_posix()
                deterministic_cleanup.initialize_root_marker(
                    args.root.resolve(), ".genesis/build", "cargo-cache"
                )
                scope = result["metadata"]["cacheKey"]["scope"]
                size_class = (
                    "cargo-verifier"
                    if scope == "evidence-verifier-host"
                    else "cargo-wasm"
                    if scope in ("root-wasi", "root-wasm")
                    else "cargo-host"
                )
                result["generated_state"] = generated_state.admit(
                    args.root.resolve(),
                    "cargo-cache",
                    result["metadata"]["cacheKeySha256"],
                    relative_target,
                    size_class,
                    pid=args.lease_pid,
                )
            try:
                result["cache_hit"] = materialize(result)
            except BaseException:
                if result.get("generated_state"):
                    generated_state.release(
                        args.root.resolve(), result["generated_state"]["leaseToken"]
                    )
                raise
        emit(result, args.format)
    except (CachePolicyError, generated_state.GeneratedStateError, OSError) as exc:
        print(f"cargo-cache: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
