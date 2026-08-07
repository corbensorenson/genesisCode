#!/usr/bin/env python3
"""Fetch and verify the same-run release-evidence fanout artifact."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import sys
import tempfile
import time
from typing import Any, Mapping, Optional, Sequence
import urllib.error
import urllib.parse
import urllib.request
import zipfile

import health_profile_evidence as health_evidence
import release_evidence_dag as evidence_dag


KIND = "genesis/release-evidence-fanout-auth-v0.2"
VERSION = "0.2.0"
AUTH_NAME = "fanout-auth.json"
MAX_ARTIFACT_BYTES = 20 * 1024 * 1024 * 1024
MAX_API_RESPONSE_BYTES = 4 * 1024 * 1024
EXECUTION_ENVIRONMENT_FIELDS = {
    "architecture", "identitySha256", "operatingSystem",
    "operatingSystemRelease", "profile", "toolchains",
}
STABLE_EXECUTION_FIELDS = ("architecture", "operatingSystem", "profile")
TOOLCHAIN_FIELDS = {"executableSha256", "name", "versionSha256"}


class FanoutError(ValueError):
    pass


class CredentialStrippingRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        if redirected is None:
            return None
        source = urllib.parse.urlsplit(req.full_url)
        target = urllib.parse.urlsplit(newurl)
        if target.scheme != "https" or target.username is not None or target.password is not None:
            fail("GitHub artifact redirect is not a credential-safe HTTPS URL")
        if (source.scheme, source.hostname, source.port) != (
            target.scheme,
            target.hostname,
            target.port,
        ):
            redirected.remove_header("Authorization")
        return redirected


SAFE_OPENER = urllib.request.build_opener(CredentialStrippingRedirect())


def fail(message: str) -> None:
    raise FanoutError(message)


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def identity(value: Mapping[str, Any]) -> str:
    clone = dict(value)
    clone.pop("contentIdentitySha256", None)
    return sha256_bytes(canonical(clone))


def exact_keys(value: Any, expected: set[str], label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        fail(f"{label} fields mismatch: expected={sorted(expected)!r} observed={observed!r}")
    return value


def is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(char in "0123456789abcdef" for char in value)
    )


def github_context(environ: Optional[Mapping[str, str]] = None) -> dict[str, str]:
    env = os.environ if environ is None else environ
    values = {
        "repository": env.get("GITHUB_REPOSITORY", ""),
        "runAttempt": env.get("GITHUB_RUN_ATTEMPT", ""),
        "runId": env.get("GITHUB_RUN_ID", ""),
        "sha": env.get("GITHUB_SHA", ""),
    }
    if "/" not in values["repository"] or values["repository"].startswith("/"):
        fail("fanout GitHub repository identity is invalid")
    for field in ("runAttempt", "runId"):
        if not values[field].isdigit() or values[field].startswith("0"):
            fail(f"fanout GitHub {field} is invalid")
    if not is_sha256(values["sha"]) and not (
        len(values["sha"]) == 40
        and all(char in "0123456789abcdef" for char in values["sha"])
    ):
        fail("fanout GitHub SHA is invalid")
    return values


def artifact_name(context: Mapping[str, str]) -> str:
    return (
        f"release-evidence-fanout-{context['runId']}-"
        f"{context['runAttempt']}-{context['sha']}"
    )


def compatible_execution_environment(
    producer: Mapping[str, Any], consumer: Mapping[str, Any]
) -> bool:
    return not execution_environment_mismatches(producer, consumer)


def validated_execution_environment(
    label: str, environment: Mapping[str, Any]
) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    exact_keys(
        environment,
        EXECUTION_ENVIRONMENT_FIELDS,
        f"fanout {label} execution environment",
    )
    core = {
        name: environment[name]
        for name in EXECUTION_ENVIRONMENT_FIELDS - {"identitySha256"}
    }
    if (
        not is_sha256(environment["identitySha256"])
        or environment["identitySha256"] != sha256_bytes(canonical(core))
    ):
        fail(f"fanout {label} execution environment identity mismatch")
    if any(
        not isinstance(environment[name], str) or not environment[name]
        for name in (*STABLE_EXECUTION_FIELDS, "operatingSystemRelease")
    ) or not isinstance(environment["toolchains"], list):
        fail(f"fanout {label} execution environment is malformed")

    tools: dict[str, dict[str, str]] = {}
    observed_names = []
    for index, raw in enumerate(environment["toolchains"]):
        row = exact_keys(raw, TOOLCHAIN_FIELDS, f"fanout {label} toolchain {index}")
        name = row["name"]
        if (
            not isinstance(name, str)
            or not name
            or not is_sha256(row["executableSha256"])
            or not is_sha256(row["versionSha256"])
            or name in tools
        ):
            fail(f"fanout {label} toolchain inventory is malformed")
        observed_names.append(name)
        tools[name] = dict(row)
    if not tools or observed_names != sorted(observed_names):
        fail(f"fanout {label} toolchain inventory is malformed")
    stable = {name: environment[name] for name in STABLE_EXECUTION_FIELDS}
    return stable, tools


def execution_environment_mismatches(
    producer: Mapping[str, Any], consumer: Mapping[str, Any]
) -> list[str]:
    producer_fields, producer_tools = validated_execution_environment("producer", producer)
    consumer_fields, consumer_tools = validated_execution_environment("consumer", consumer)
    mismatches = []
    for name in STABLE_EXECUTION_FIELDS:
        if producer_fields[name] != consumer_fields[name]:
            mismatches.append(
                f"{name} producer={producer_fields[name]!r} consumer={consumer_fields[name]!r}"
            )
    producer_names = set(producer_tools)
    consumer_names = set(consumer_tools)
    if producer_names != consumer_names:
        mismatches.append(
            "toolchain names "
            f"producer={sorted(producer_names)!r} consumer={sorted(consumer_names)!r}"
        )
    for name in sorted(producer_names & consumer_names):
        for field in ("executableSha256", "versionSha256"):
            if producer_tools[name][field] != consumer_tools[name][field]:
                mismatches.append(
                    f"toolchain {name!r} {field} "
                    f"producer={producer_tools[name][field]} "
                    f"consumer={consumer_tools[name][field]}"
                )
    return mismatches


def execution_environment_fixture() -> dict[str, Any]:
    core = {
        "architecture": "x86_64",
        "operatingSystem": "Linux",
        "operatingSystemRelease": "fixture-kernel-a",
        "profile": "release-full",
        "toolchains": [
            {
                "executableSha256": "a" * 64,
                "name": "python3",
                "versionSha256": "b" * 64,
            },
            {
                "executableSha256": "c" * 64,
                "name": "rustc",
                "versionSha256": "d" * 64,
            },
        ],
    }
    return {**core, "identitySha256": sha256_bytes(canonical(core))}


def reidentify_execution_environment(environment: Mapping[str, Any]) -> dict[str, Any]:
    result = json.loads(json.dumps(environment))
    core = {
        name: result[name]
        for name in EXECUTION_ENVIRONMENT_FIELDS - {"identitySha256"}
    }
    result["identitySha256"] = sha256_bytes(canonical(core))
    return result


def self_test() -> int:
    producer = execution_environment_fixture()
    if not compatible_execution_environment(producer, producer):
        fail("fanout self-test rejected an identical execution environment")

    kernel_variant = dict(producer)
    kernel_variant["operatingSystemRelease"] = "fixture-kernel-b"
    kernel_variant = reidentify_execution_environment(kernel_variant)
    if not compatible_execution_environment(producer, kernel_variant):
        fail("fanout self-test rejected the declared kernel-release variance")

    controls = 0
    mutations = [
        lambda row: row.__setitem__("architecture", "aarch64"),
        lambda row: row.__setitem__("operatingSystem", "Darwin"),
        lambda row: row.__setitem__("profile", "prepush-standard"),
        lambda row: row["toolchains"][0].__setitem__("executableSha256", "e" * 64),
        lambda row: row["toolchains"][0].__setitem__("versionSha256", "f" * 64),
        lambda row: row["toolchains"].pop(),
    ]
    for mutate in mutations:
        candidate = json.loads(json.dumps(producer))
        mutate(candidate)
        candidate = reidentify_execution_environment(candidate)
        if not execution_environment_mismatches(producer, candidate):
            fail(f"fanout self-test accepted compatibility mutation {controls + 1}")
        controls += 1

    malformed = []
    duplicate = json.loads(json.dumps(producer))
    duplicate["toolchains"].append(dict(duplicate["toolchains"][0]))
    malformed.append(reidentify_execution_environment(duplicate))
    unsorted = json.loads(json.dumps(producer))
    unsorted["toolchains"].reverse()
    malformed.append(reidentify_execution_environment(unsorted))
    bad_hash = json.loads(json.dumps(producer))
    bad_hash["toolchains"][0]["versionSha256"] = "not-a-hash"
    malformed.append(reidentify_execution_environment(bad_hash))
    stale_identity = dict(producer)
    stale_identity["identitySha256"] = "0" * 64
    malformed.append(stale_identity)
    for candidate in malformed:
        try:
            execution_environment_mismatches(producer, candidate)
        except FanoutError:
            controls += 1
        else:
            fail(f"fanout self-test accepted malformed environment {controls + 1}")
    return controls


def load_manifest(root: Path, bundle: Path) -> dict[str, Any]:
    manifest_path = bundle / "manifest.json"
    try:
        payload = manifest_path.read_bytes()
        manifest = health_evidence.validate_manifest_shape(json.loads(payload))
    except (OSError, UnicodeError, json.JSONDecodeError, health_evidence.EvidenceError) as exc:
        fail(f"fanout evidence manifest is invalid: {exc}")
    if manifest["profile"] != "release-full":
        fail("fanout evidence profile is not release-full")
    if manifest["source"] != health_evidence.source_inventory(root):
        fail("fanout evidence source does not match the checkout")
    mismatches = execution_environment_mismatches(
        manifest["executionEnvironment"],
        health_evidence.execution_environment("release-full"),
    )
    if mismatches:
        fail(
            "fanout evidence execution environment does not match the consumer: "
            + "; ".join(mismatches)
        )
    generated = health_evidence.parse_time(manifest["generatedAtUtc"], "generatedAtUtc")
    expires = health_evidence.parse_time(manifest["expiresAtUtc"], "expiresAtUtc")
    now = dt.datetime.now(dt.timezone.utc)
    if generated > now + dt.timedelta(minutes=5) or now > expires:
        fail("fanout evidence is stale")
    for name, record in manifest["artifacts"].items():
        path = bundle / name
        try:
            size = path.stat().st_size
        except OSError as exc:
            fail(f"fanout artifact is missing: {name}: {exc}")
        if size != record["bytes"] or sha256_file(path) != record["sha256"]:
            fail(f"fanout artifact identity mismatch: {name}")
    return manifest


def dag_identity(root: Path) -> str:
    policy = evidence_dag.load_policy(root)
    source = (root / "scripts/render_upgrade_plan_health_report.sh").read_text(encoding="utf-8")
    evidence_dag.validate(policy, source)
    return evidence_dag.sha256(policy)


def validate_auth(
    root: Path,
    bundle: Path,
    auth_path: Path,
    token: str,
    environ: Optional[Mapping[str, str]] = None,
) -> dict[str, Any]:
    try:
        auth = json.loads(auth_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        fail(f"cannot read fanout authentication: {exc}")
    exact_keys(
        auth,
        {
            "artifact", "contentIdentitySha256", "github", "kind", "producer",
            "version",
        },
        "fanout authentication",
    )
    if auth["kind"] != KIND or auth["version"] != VERSION:
        fail("fanout authentication identity mismatch")
    if auth["contentIdentitySha256"] != identity(auth):
        fail("fanout authentication content identity mismatch")
    context = github_context(environ)
    if auth["github"] != context:
        fail("fanout authentication is from another workflow run, attempt, or revision")
    artifact = exact_keys(
        auth["artifact"],
        {"createdAt", "digestSha256", "id", "name"},
        "fanout artifact",
    )
    if (
        artifact["name"] != artifact_name(context)
        or not isinstance(artifact["id"], int)
        or artifact["id"] <= 0
        or not is_sha256(artifact["digestSha256"])
        or token != artifact["digestSha256"]
    ):
        fail("fanout artifact token or identity mismatch")
    try:
        created_at = dt.datetime.fromisoformat(str(artifact["createdAt"]).replace("Z", "+00:00"))
    except ValueError:
        fail("fanout artifact creation time is invalid")
    now = dt.datetime.now(dt.timezone.utc)
    if (
        created_at.tzinfo is None
        or created_at > now + dt.timedelta(minutes=5)
        or now - created_at > dt.timedelta(hours=6)
    ):
        fail("fanout artifact creation time is stale or implausible")
    producer = exact_keys(
        auth["producer"],
        {
            "bundleIdentitySha256", "dagIdentitySha256", "evidenceClass",
            "index", "manifestSha256",
        },
        "fanout producer",
    )
    manifest = load_manifest(root, bundle)
    manifest_path = bundle / "manifest.json"
    if (
        producer["evidenceClass"] != "cache-sensitive"
        or producer["index"] != 1
        or producer["dagIdentitySha256"] != dag_identity(root)
        or producer["manifestSha256"] != sha256_file(manifest_path)
        or producer["bundleIdentitySha256"] != manifest["contentIdentitySha256"]
        or manifest["source"]["gitCommit"] != context["sha"]
    ):
        fail("fanout producer binding mismatch")
    return auth


def api_json(url: str, token: str) -> dict[str, Any]:
    require_https_url(url, "GitHub artifact API")
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        with SAFE_OPENER.open(request, timeout=30) as response:
            payload = response.read(MAX_API_RESPONSE_BYTES + 1)
            if len(payload) > MAX_API_RESPONSE_BYTES:
                fail("GitHub artifact API response exceeds the metadata bound")
            value = json.loads(payload)
    except (OSError, UnicodeError, json.JSONDecodeError, urllib.error.HTTPError) as exc:
        fail(f"GitHub artifact API request failed: {exc}")
    if not isinstance(value, dict):
        fail("GitHub artifact API response is not an object")
    return value


def api_download(url: str, token: str, destination: Path) -> str:
    require_https_url(url, "GitHub artifact download API")
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        digest = hashlib.sha256()
        observed = 0
        with SAFE_OPENER.open(request, timeout=120) as response:
            with destination.open("xb") as handle:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    observed += len(chunk)
                    if observed > MAX_ARTIFACT_BYTES:
                        fail("fanout archive exceeds the worker artifact budget")
                    digest.update(chunk)
                    handle.write(chunk)
        return digest.hexdigest()
    except (OSError, urllib.error.HTTPError) as exc:
        fail(f"GitHub artifact download failed: {exc}")


def require_https_url(url: str, label: str) -> None:
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
    ):
        fail(f"{label} must use credential-safe HTTPS")


def safe_extract(archive_path: Path, output: Path) -> None:
    try:
        archive = zipfile.ZipFile(archive_path)
    except zipfile.BadZipFile as exc:
        fail(f"fanout archive is invalid: {exc}")
    names: set[str] = set()
    expanded = 0
    for info in archive.infolist():
        path = PurePosixPath(info.filename)
        mode = info.external_attr >> 16
        if (
            not path.parts
            or path.is_absolute()
            or ".." in path.parts
            or path.as_posix() in names
            or stat.S_ISLNK(mode)
        ):
            fail(f"fanout archive path is unsafe or duplicated: {info.filename!r}")
        names.add(path.as_posix())
        expanded += info.file_size
        if expanded > MAX_ARTIFACT_BYTES:
            fail("fanout archive expansion exceeds the worker artifact budget")
    if output.exists() and any(output.iterdir()):
        fail("fanout output must start absent or empty")
    output.mkdir(parents=True, exist_ok=True)
    for info in archive.infolist():
        relative = PurePosixPath(info.filename)
        destination = output.joinpath(*relative.parts)
        if info.is_dir():
            destination.mkdir(parents=True, exist_ok=True)
            continue
        destination.parent.mkdir(parents=True, exist_ok=True)
        with archive.open(info) as source, destination.open("xb") as target:
            shutil.copyfileobj(source, target, length=1024 * 1024)


def fetch(root: Path, output: Path, timeout_seconds: int, poll_seconds: int) -> dict[str, Any]:
    context = github_context()
    token = os.environ.get("GITHUB_TOKEN", "")
    if not token:
        fail("GITHUB_TOKEN is required to authenticate same-run fanout")
    name = artifact_name(context)
    base = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")
    encoded_name = urllib.parse.quote(name, safe="")
    url = (
        f"{base}/repos/{context['repository']}/actions/runs/{context['runId']}/artifacts"
        f"?name={encoded_name}&per_page=100"
    )
    deadline = time.monotonic() + timeout_seconds
    artifact: Optional[dict[str, Any]] = None
    while time.monotonic() < deadline:
        response = api_json(url, token)
        rows = response.get("artifacts")
        if not isinstance(rows, list):
            fail("GitHub artifact API lacks an artifacts array")
        candidates = [
            row for row in rows
            if isinstance(row, dict) and row.get("name") == name and row.get("expired") is False
        ]
        if len(candidates) > 1:
            fail("same-run fanout artifact name is duplicated")
        if candidates:
            artifact = candidates[0]
            break
        time.sleep(poll_seconds)
    if artifact is None:
        fail("same-run cold-1 fanout artifact did not become available before deadline")
    digest = artifact.get("digest")
    if not isinstance(digest, str) or not digest.startswith("sha256:") or not is_sha256(digest[7:]):
        fail("GitHub fanout artifact lacks an authenticated SHA-256 digest")
    artifact_id = artifact.get("id")
    if not isinstance(artifact_id, int) or artifact_id <= 0:
        fail("GitHub fanout artifact id is invalid")
    with tempfile.TemporaryDirectory(prefix="genesis-release-fanout-") as temp:
        archive_path = Path(temp) / "fanout.zip"
        observed_digest = api_download(
            f"{base}/repos/{context['repository']}/actions/artifacts/{artifact_id}/zip",
            token,
            archive_path,
        )
        if observed_digest != digest[7:]:
            fail("downloaded fanout archive digest does not match GitHub")
        safe_extract(archive_path, output)
    manifest = load_manifest(root, output)
    auth = {
        "artifact": {
            "createdAt": artifact.get("created_at"),
            "digestSha256": digest[7:],
            "id": artifact_id,
            "name": name,
        },
        "contentIdentitySha256": "",
        "github": context,
        "kind": KIND,
        "producer": {
            "bundleIdentitySha256": manifest["contentIdentitySha256"],
            "dagIdentitySha256": dag_identity(root),
            "evidenceClass": "cache-sensitive",
            "index": 1,
            "manifestSha256": sha256_file(output / "manifest.json"),
        },
        "version": VERSION,
    }
    auth["contentIdentitySha256"] = identity(auth)
    auth_path = output.parent / AUTH_NAME
    auth_path.write_text(json.dumps(auth, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    validate_auth(root, output, auth_path, digest[7:])
    return auth


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("self-test")
    fetch_parser = sub.add_parser("fetch")
    fetch_parser.add_argument("--output", type=Path, required=True)
    fetch_parser.add_argument("--timeout-seconds", type=int, default=2400)
    fetch_parser.add_argument("--poll-seconds", type=int, default=15)
    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("--bundle", type=Path, required=True)
    verify_parser.add_argument("--auth", type=Path, required=True)
    verify_parser.add_argument("--token", required=True)
    args = parser.parse_args(argv)
    try:
        root = args.root.resolve(strict=True)
        if args.action == "self-test":
            controls = self_test()
            print(f"release-evidence-fanout: self-test ok (negative_controls={controls})")
        elif args.action == "fetch":
            if not 1 <= args.timeout_seconds <= 2700 or not 1 <= args.poll_seconds <= 60:
                fail("fanout polling limits are invalid")
            auth = fetch(root, args.output.resolve(), args.timeout_seconds, args.poll_seconds)
            print(auth["artifact"]["digestSha256"])
        else:
            validate_auth(
                root,
                args.bundle.resolve(strict=True),
                args.auth.resolve(strict=True),
                args.token,
            )
            print("release-evidence-fanout: verified")
    except (
        FanoutError,
        evidence_dag.DagError,
        health_evidence.EvidenceError,
        OSError,
        UnicodeError,
        json.JSONDecodeError,
    ) as exc:
        print(f"release-evidence-fanout: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
