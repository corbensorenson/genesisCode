#!/usr/bin/env python3
"""Validate evidence storage classes and render immutable release assets."""

from __future__ import annotations

import argparse
from hashlib import sha256
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import sys
import tarfile
from typing import Any, Dict, Iterable, List, Mapping, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_POLICY = ROOT / "policies/evidence_storage_classes_v0.1.json"
DEFAULT_FIXTURE_CATALOG = (
    ROOT / "docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json"
)
EVIDENCE_ROOT = ROOT / "docs/program/evidence"
RELEASE_ID_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
HEX64_RE = re.compile(r"^[0-9a-f]{64}$")
CLASS_IDS = ("E0", "E1", "E2", "E3", "E4")

BASE_FIXTURE_ROLES = {
    "docs/program/evidence/GENESIS_EVIDENCE_ARTIFACT_TREE_v0.1.json": (
        "artifact-hash-tree",
        "E3",
    ),
    "docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json": (
        "authenticated-bundle",
        "E3",
    ),
    "docs/program/evidence/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.json": (
        "adversarial-catalog",
        "none",
    ),
    "docs/program/evidence/artifact/genesis-example.bin": (
        "subject-artifact",
        "E3",
    ),
}
ROADMAP_BASELINE_RE = re.compile(
    r"^docs/program/evidence/roadmap-baselines/roadmap-baseline-e0-[0-9]{4}-[0-9]{2}-[0-9]{2}-sha256-[0-9a-f]{64}\.json$"
)
ROADMAP_BASELINE_KEY_RE = re.compile(
    r"^docs/program/evidence/roadmap-baselines/roadmap-baseline-fixture-key-sha256-[0-9a-f]{64}\.pub$"
)


def fixture_roles() -> Mapping[str, Tuple[str, str]]:
    roles = dict(BASE_FIXTURE_ROLES)
    baseline_root = EVIDENCE_ROOT / "roadmap-baselines"
    if baseline_root.is_dir():
        for path in baseline_root.iterdir():
            if not path.is_file():
                continue
            relative = path.relative_to(ROOT).as_posix()
            if ROADMAP_BASELINE_RE.fullmatch(relative):
                roles[relative] = ("signed-e0-baseline", "E0")
            elif ROADMAP_BASELINE_KEY_RE.fullmatch(relative):
                roles[relative] = ("fixture-public-key", "none")
    return roles


class StorageError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise StorageError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path, label: str) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise StorageError(f"missing {label}: {display_path(path)}") from exc
    except json.JSONDecodeError as exc:
        raise StorageError(
            f"invalid JSON in {label}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def parse_json_bytes(data: bytes, label: str) -> Any:
    try:
        return json.loads(data.decode("utf-8"), object_pairs_hook=reject_duplicate_keys)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise StorageError(f"invalid JSON in {label}: {exc}") from exc


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def reject_floats(value: Any, label: str) -> None:
    if isinstance(value, float):
        raise StorageError(f"{label} must not contain floating-point values")
    if isinstance(value, list):
        for index, item in enumerate(value):
            reject_floats(item, f"{label}[{index}]")
    elif isinstance(value, dict):
        for key, item in value.items():
            reject_floats(item, f"{label}.{key}")


def canonical_bytes(value: Any) -> bytes:
    reject_floats(value, "document")
    return json.dumps(
        value,
        ensure_ascii=False,
        allow_nan=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def retained_bytes(value: Any) -> bytes:
    reject_floats(value, "document")
    return (
        json.dumps(
            value, ensure_ascii=False, allow_nan=False, sort_keys=True, indent=2
        ).encode("utf-8")
        + b"\n"
    )


def sha256_hex(data: bytes) -> str:
    return sha256(data).hexdigest()


def require_object(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise StorageError(f"{label} must be an object")
    return value


def require_array(value: Any, label: str, *, non_empty: bool = False) -> List[Any]:
    if not isinstance(value, list):
        raise StorageError(f"{label} must be an array")
    if non_empty and not value:
        raise StorageError(f"{label} must not be empty")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise StorageError(f"{label} must be a non-empty string")
    return value


def require_int(value: Any, label: str, *, minimum: int = 0) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise StorageError(f"{label} must be an integer >= {minimum}")
    return value


def exact_keys(value: Mapping[str, Any], keys: Iterable[str], label: str) -> None:
    expected = set(keys)
    observed = set(value)
    missing = sorted(expected - observed)
    unknown = sorted(observed - expected)
    if missing:
        raise StorageError(f"{label} missing fields: {', '.join(missing)}")
    if unknown:
        raise StorageError(f"{label} contains unknown fields: {', '.join(unknown)}")


def require_sorted_unique(values: Sequence[str], label: str) -> None:
    if list(values) != sorted(set(values)):
        raise StorageError(f"{label} must be sorted and unique")


def require_sha256(value: Any, label: str) -> str:
    text = require_string(value, label)
    if not HEX64_RE.fullmatch(text):
        raise StorageError(f"{label} must be 64 lowercase hexadecimal characters")
    return text


def require_relative_path(value: Any, label: str) -> str:
    text = require_string(value, label)
    path = PurePosixPath(text)
    if (
        text.startswith("/")
        or "\\" in text
        or "//" in text
        or ".." in path.parts
        or (len(text) >= 2 and text[1] == ":")
    ):
        raise StorageError(f"{label} must be a normalized relative path")
    return text


def validate_policy(policy: Any) -> Mapping[str, Any]:
    policy = require_object(policy, "storage policy")
    exact_keys(
        policy,
        (
            "kind",
            "version",
            "authorityRules",
            "classes",
            "releaseAssetProfile",
            "mirrorProfile",
        ),
        "storage policy",
    )
    if policy["kind"] != "genesis/evidence-storage-policy-v0.1":
        raise StorageError("storage policy kind is unsupported")
    if policy["version"] != "0.1":
        raise StorageError("storage policy version is unsupported")
    authority = require_object(policy["authorityRules"], "authority rules")
    exact_keys(
        authority,
        (
            "claimedClassDoesNotGrantAuthority",
            "inTreeFixtureMaximumClass",
            "releaseAuthorityRequiresExternalPolicy",
            "selfPublicationForbidden",
        ),
        "authority rules",
    )
    if authority != {
        "claimedClassDoesNotGrantAuthority": True,
        "inTreeFixtureMaximumClass": "E2",
        "releaseAuthorityRequiresExternalPolicy": True,
        "selfPublicationForbidden": True,
    }:
        raise StorageError("storage authority rules must remain fail-closed")
    classes = require_array(policy["classes"], "storage classes", non_empty=True)
    if [item.get("id") for item in classes if isinstance(item, dict)] != list(
        CLASS_IDS
    ):
        raise StorageError("storage classes must contain E0-E4 exactly in order")
    for index, raw in enumerate(classes):
        item = require_object(raw, f"storage class[{index}]")
        exact_keys(
            item,
            (
                "id",
                "authority",
                "root",
                "tracked",
                "mutable",
                "requirements",
                "publication",
            ),
            f"storage class[{index}]",
        )
        class_id = item["id"]
        require_string(item["authority"], f"{class_id}.authority")
        require_relative_path(item["root"], f"{class_id}.root")
        if not isinstance(item["tracked"], bool) or not isinstance(
            item["mutable"], bool
        ):
            raise StorageError(f"{class_id} tracked/mutable fields must be boolean")
        requirements = require_object(item["requirements"], f"{class_id}.requirements")
        exact_keys(
            requirements,
            (
                "authentication",
                "independentVerification",
                "minimumSignatures",
                "minimumMirrors",
            ),
            f"{class_id}.requirements",
        )
        require_string(requirements["authentication"], f"{class_id}.authentication")
        require_int(requirements["minimumSignatures"], f"{class_id}.minimumSignatures")
        require_int(requirements["minimumMirrors"], f"{class_id}.minimumMirrors")
        if not isinstance(requirements["independentVerification"], bool):
            raise StorageError(f"{class_id}.independentVerification must be boolean")
        publication = require_object(item["publication"], f"{class_id}.publication")
        exact_keys(
            publication,
            ("channel", "immutable", "offlineVerifiable"),
            f"{class_id}.publication",
        )
        require_string(publication["channel"], f"{class_id}.channel")
        if not isinstance(publication["immutable"], bool) or not isinstance(
            publication["offlineVerifiable"], bool
        ):
            raise StorageError(f"{class_id} publication booleans are invalid")
        if class_id == "E0" and (item["tracked"] or not item["mutable"]):
            raise StorageError("E0 must remain ignored and mutable")
        if class_id in ("E1", "E2") and not item["tracked"]:
            raise StorageError(f"{class_id} must remain source-controlled")
        if class_id in ("E3", "E4"):
            if item["tracked"] or item["mutable"] or not publication["immutable"]:
                raise StorageError(f"{class_id} must be untracked and immutable")
            if not requirements["independentVerification"]:
                raise StorageError(f"{class_id} requires independent verification")
            if not item["root"].startswith(".genesis/release-assets/evidence/"):
                raise StorageError(
                    f"{class_id} root must remain under ignored release assets"
                )
    e4 = classes[4]["requirements"]
    if e4["minimumSignatures"] < 2 or e4["minimumMirrors"] < 2:
        raise StorageError("E4 requires at least two signatures and two mirrors")
    release = require_object(policy["releaseAssetProfile"], "release asset profile")
    exact_keys(
        release,
        (
            "format",
            "mediaType",
            "sourceDateEpoch",
            "fileMode",
            "directoryMode",
            "ownerId",
            "groupId",
            "maximumAssetBytes",
            "contentAddressedFilename",
            "requiredArchiveEntries",
            "forbiddenArchivePrefixes",
        ),
        "release asset profile",
    )
    if (
        release["format"] != "ustar"
        or release["sourceDateEpoch"] != 0
        or release["fileMode"] != 0o644
        or release["directoryMode"] != 0o755
        or release["ownerId"] != 0
        or release["groupId"] != 0
    ):
        raise StorageError("USTAR normalization profile drift")
    require_int(release["maximumAssetBytes"], "maximum asset bytes", minimum=1)
    for field in ("requiredArchiveEntries", "forbiddenArchivePrefixes"):
        values = [
            require_string(item, field)
            for item in require_array(release[field], field, non_empty=True)
        ]
        require_sorted_unique(values, field)
    mirror = require_object(policy["mirrorProfile"], "mirror profile")
    exact_keys(
        mirror,
        (
            "copyMode",
            "overwriteForbidden",
            "verifyBeforeAndAfterCopy",
            "trustPolicyDistribution",
            "requiredInstructions",
        ),
        "mirror profile",
    )
    if (
        mirror["copyMode"] != "create-new"
        or mirror["overwriteForbidden"] is not True
        or mirror["verifyBeforeAndAfterCopy"] is not True
        or mirror["trustPolicyDistribution"] != "out-of-band"
    ):
        raise StorageError(
            "mirror profile must remain create-new and out-of-band trusted"
        )
    instructions = [
        require_string(item, "mirror instruction")
        for item in require_array(
            mirror["requiredInstructions"], "mirror instructions", non_empty=True
        )
    ]
    require_sorted_unique(instructions, "mirror instructions")
    return policy


def class_policy(policy: Mapping[str, Any], storage_class: str) -> Mapping[str, Any]:
    if storage_class not in ("E3", "E4"):
        raise StorageError("release assets may only use E3 or E4")
    for item in policy["classes"]:
        if item["id"] == storage_class:
            return item
    raise StorageError(f"storage class is absent from policy: {storage_class}")


def render_fixture_catalog() -> Mapping[str, Any]:
    roles = fixture_roles()
    observed = {
        path.relative_to(ROOT).as_posix(): path
        for path in EVIDENCE_ROOT.rglob("*")
        if path.is_file()
    }
    if set(observed) != set(roles):
        missing = sorted(set(roles) - set(observed))
        unknown = sorted(set(observed) - set(roles))
        raise StorageError(
            f"fixture classification coverage drift: missing={missing} unknown={unknown}"
        )
    files = []
    for relative in sorted(observed):
        role, claimed_class = roles[relative]
        files.append(
            {
                "path": relative,
                "sha256": sha256_hex(observed[relative].read_bytes()),
                "role": role,
                "claimedEvidenceClass": claimed_class,
                "testOnly": True,
            }
        )
    return {
        "kind": "genesis/evidence-fixture-classification-v0.1",
        "version": "0.1",
        "root": "docs/program/evidence/",
        "distributionClass": "E2",
        "authority": False,
        "coverage": "exact-recursive-files",
        "files": files,
    }


def check_fixture_catalog(path: Path) -> None:
    observed = load_json(path, "fixture classification")
    expected = render_fixture_catalog()
    if observed != expected or path.read_bytes() != retained_bytes(expected):
        raise StorageError(
            "fixture classification drift: run bash scripts/update_evidence_fixture_classification.sh"
        )


def load_release_inputs(
    storage_class: str,
    bundle_path: Path,
    tree_path: Path,
    artifact_root: Path,
    verification_path: Path,
    policy: Mapping[str, Any],
) -> Tuple[
    Mapping[str, Any], Mapping[str, Any], Mapping[str, Any], List[Tuple[str, bytes]]
]:
    class_info = class_policy(policy, storage_class)
    bundle_bytes = bundle_path.read_bytes()
    tree_bytes = tree_path.read_bytes()
    verification_bytes = verification_path.read_bytes()
    bundle = require_object(
        parse_json_bytes(bundle_bytes, "release bundle"), "release bundle"
    )
    tree = require_object(
        parse_json_bytes(tree_bytes, "artifact tree"), "artifact tree"
    )
    verification = require_object(
        parse_json_bytes(verification_bytes, "verification result"),
        "verification result",
    )
    if bundle_bytes != retained_bytes(bundle) or tree_bytes != retained_bytes(tree):
        raise StorageError(
            "release bundle and artifact tree must use retained canonical encoding"
        )
    if bundle.get("profile") != storage_class:
        raise StorageError(
            f"class escalation rejected: bundle profile={bundle.get('profile')} requested={storage_class}"
        )
    if verification.get("ok") is not True:
        raise StorageError(
            "release asset requires a passing independent verification result"
        )
    if verification.get("bundleSha256") != sha256_hex(bundle_bytes):
        raise StorageError("verification result does not bind exact bundle bytes")
    if verification.get("artifactTreeSha256") != sha256_hex(canonical_bytes(tree)):
        raise StorageError("verification result does not bind canonical artifact tree")
    verified_signatures = require_int(
        verification.get("verifiedSignatures"), "verified signatures"
    )
    if verified_signatures < class_info["requirements"]["minimumSignatures"]:
        raise StorageError(f"{storage_class} signature threshold is not satisfied")
    entries = require_array(
        tree.get("entries"), "artifact tree entries", non_empty=True
    )
    artifacts: List[Tuple[str, bytes]] = []
    if artifact_root.is_symlink() or not artifact_root.is_dir():
        raise StorageError("artifact root must be a non-symlink directory")
    canonical_root = artifact_root.resolve(strict=True)
    for index, raw in enumerate(entries):
        entry = require_object(raw, f"artifact tree entry[{index}]")
        path = require_relative_path(
            entry.get("path"), f"artifact tree entry[{index}].path"
        )
        expected_digest = require_sha256(
            require_object(entry.get("digest"), "artifact digest").get("sha256"),
            "artifact digest",
        )
        expected_size = require_int(entry.get("sizeBytes"), "artifact size")
        source = artifact_root
        for part in PurePosixPath(path).parts:
            source = source / part
            try:
                metadata = source.lstat()
            except FileNotFoundError as exc:
                raise StorageError(f"release artifact path is missing: {path}") from exc
            if stat.S_ISLNK(metadata.st_mode):
                raise StorageError(f"release artifact path contains a symlink: {path}")
        if not source.resolve(strict=True).is_relative_to(canonical_root):
            raise StorageError(f"release artifact escapes artifact root: {path}")
        with source.open("rb") as handle:
            metadata = os.fstat(handle.fileno())
            if not stat.S_ISREG(metadata.st_mode):
                raise StorageError(f"release artifact is not a regular file: {path}")
            data = handle.read(expected_size + 1)
        if len(data) != expected_size or sha256_hex(data) != expected_digest:
            raise StorageError(f"release artifact differs from hash tree: {path}")
        artifacts.append((f"evidence/{path}", data))
    return bundle, tree, verification, artifacts


def tar_bytes(files: Sequence[Tuple[str, bytes]], policy: Mapping[str, Any]) -> bytes:
    release = policy["releaseAssetProfile"]
    names = [name for name, _ in files]
    require_sorted_unique(names, "archive entries")
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        for name, data in files:
            require_relative_path(name, "archive entry")
            info = tarfile.TarInfo(name=name)
            info.size = len(data)
            info.mtime = release["sourceDateEpoch"]
            info.mode = release["fileMode"]
            info.uid = release["ownerId"]
            info.gid = release["groupId"]
            info.uname = ""
            info.gname = ""
            archive.addfile(info, io.BytesIO(data))
    data = stream.getvalue()
    if len(data) > release["maximumAssetBytes"]:
        raise StorageError("release asset exceeds policy size limit")
    return data


def release_manifest(
    storage_class: str,
    release_id: str,
    policy_digest: str,
    payloads: Sequence[Tuple[str, bytes]],
    verification: Mapping[str, Any],
) -> Mapping[str, Any]:
    return {
        "kind": "genesis/evidence-release-manifest-v0.1",
        "version": "0.1",
        "releaseId": release_id,
        "storageClass": storage_class,
        "authorityState": "candidate-until-immutable-publication",
        "storagePolicySha256": policy_digest,
        "verificationPolicySha256": verification["policySha256"],
        "trustPolicyBundled": False,
        "entries": [
            {"path": name, "sha256": sha256_hex(data), "sizeBytes": len(data)}
            for name, data in payloads
        ],
    }


def internal_mirror_instructions(
    storage_class: str,
    release_id: str,
    class_info: Mapping[str, Any],
    policy: Mapping[str, Any],
) -> Mapping[str, Any]:
    return {
        "kind": "genesis/evidence-mirror-instructions-v0.1",
        "version": "0.1",
        "releaseId": release_id,
        "storageClass": storage_class,
        "minimumMirrors": class_info["requirements"]["minimumMirrors"],
        "trustPolicyDistribution": policy["mirrorProfile"]["trustPolicyDistribution"],
        "instructionIds": policy["mirrorProfile"]["requiredInstructions"],
        "rules": [
            "obtain trust policy hash through an authenticated out-of-band channel",
            "verify primary asset SHA-256 before opening archive",
            "fetch required independent mirror and compare exact bytes",
            "run the standalone verifier offline against extracted logical payloads",
            "never overwrite an existing content-addressed asset",
        ],
    }


def external_mirror_descriptor(
    storage_class: str,
    release_id: str,
    asset_name: str,
    asset_digest: str,
    asset_size: int,
    class_info: Mapping[str, Any],
    policy: Mapping[str, Any],
) -> Mapping[str, Any]:
    return {
        "kind": "genesis/evidence-release-mirror-v0.1",
        "version": "0.1",
        "releaseId": release_id,
        "storageClass": storage_class,
        "authorityState": "candidate-until-immutable-publication",
        "asset": {
            "name": asset_name,
            "sha256": asset_digest,
            "sizeBytes": asset_size,
            "mediaType": policy["releaseAssetProfile"]["mediaType"],
        },
        "minimumMirrors": class_info["requirements"]["minimumMirrors"],
        "trustPolicyDistribution": "out-of-band",
        "instructions": [
            {
                "id": "compare-bytes",
                "argv": ["cmp", "--silent", "<PRIMARY_ASSET>", "<MIRROR_ASSET>"],
            },
            {
                "id": "fetch-independent-mirror",
                "argv": [
                    "curl",
                    "--fail",
                    "<INDEPENDENT_MIRROR_URL>",
                    "--output",
                    asset_name,
                ],
            },
            {
                "id": "fetch-primary",
                "argv": [
                    "curl",
                    "--fail",
                    "<PRIMARY_RELEASE_URL>",
                    "--output",
                    asset_name,
                ],
            },
            {
                "id": "verify-offline",
                "argv": [
                    "genesis-evidence-verifier",
                    "--bundle",
                    "evidence/bundle.json",
                    "--policy",
                    "<OUT_OF_BAND_POLICY>",
                    "--policy-sha256",
                    "<OUT_OF_BAND_POLICY_SHA256>",
                    "--artifact-tree",
                    "evidence/artifact-tree.json",
                    "--artifact-root",
                    "evidence",
                ],
            },
            {
                "id": "verify-sha256",
                "argv": [
                    "shasum",
                    "-a",
                    "256",
                    "--check",
                    f"{asset_digest}  {asset_name}",
                ],
            },
        ],
    }


def render_release(
    policy_path: Path,
    storage_class: str,
    release_id: str,
    bundle_path: Path,
    tree_path: Path,
    artifact_root: Path,
    verification_path: Path,
    output_dir: Path,
) -> str:
    if not RELEASE_ID_RE.fullmatch(release_id):
        raise StorageError("release id must be a lowercase portable identifier")
    policy_bytes = policy_path.read_bytes()
    policy = validate_policy(parse_json_bytes(policy_bytes, "storage policy"))
    class_info = class_policy(policy, storage_class)
    bundle, tree, verification, artifacts = load_release_inputs(
        storage_class,
        bundle_path,
        tree_path,
        artifact_root,
        verification_path,
        policy,
    )
    payloads = [
        ("evidence/artifact-tree.json", retained_bytes(tree)),
        ("evidence/bundle.json", retained_bytes(bundle)),
        *artifacts,
        ("verification/result.json", retained_bytes(verification)),
    ]
    payloads.sort(key=lambda item: item[0])
    manifest = release_manifest(
        storage_class,
        release_id,
        sha256_hex(policy_bytes),
        payloads,
        verification,
    )
    mirror_inside = internal_mirror_instructions(
        storage_class, release_id, class_info, policy
    )
    archive_files = [
        ("MIRROR.json", retained_bytes(mirror_inside)),
        *payloads,
        ("release-manifest.json", retained_bytes(manifest)),
    ]
    archive_files.sort(key=lambda item: item[0])
    archive = tar_bytes(archive_files, policy)
    digest = sha256_hex(archive)
    template = policy["releaseAssetProfile"]["contentAddressedFilename"]
    asset_name = template.format(
        **{"class": storage_class, "release": release_id, "digest": digest}
    )
    descriptor = external_mirror_descriptor(
        storage_class,
        release_id,
        asset_name,
        digest,
        len(archive),
        class_info,
        policy,
    )
    output_dir.mkdir(parents=True, exist_ok=True)
    outputs = {
        asset_name: archive,
        f"{asset_name}.mirror.json": retained_bytes(descriptor),
        f"{asset_name}.sha256": f"{digest}  {asset_name}\n".encode("ascii"),
    }
    for name, data in outputs.items():
        path = output_dir / name
        try:
            with path.open("xb") as handle:
                handle.write(data)
        except FileExistsError as exc:
            raise StorageError(
                f"immutable release output already exists: {name}"
            ) from exc
    return asset_name


def safe_tar_members(data: bytes, policy: Mapping[str, Any]) -> Mapping[str, bytes]:
    release = policy["releaseAssetProfile"]
    if len(data) > release["maximumAssetBytes"]:
        raise StorageError("release asset exceeds policy size limit")
    result: Dict[str, bytes] = {}
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as archive:
            for member in archive.getmembers():
                name = require_relative_path(member.name, "archive member")
                if not member.isfile() or member.issym() or member.islnk():
                    raise StorageError(f"archive member is not a regular file: {name}")
                if (
                    member.mtime != release["sourceDateEpoch"]
                    or member.mode != release["fileMode"]
                    or member.uid != release["ownerId"]
                    or member.gid != release["groupId"]
                    or member.uname
                    or member.gname
                ):
                    raise StorageError(f"archive metadata is not normalized: {name}")
                if name in result:
                    raise StorageError(f"duplicate archive member: {name}")
                stream = archive.extractfile(member)
                if stream is None:
                    raise StorageError(f"archive member cannot be read: {name}")
                result[name] = stream.read()
    except tarfile.TarError as exc:
        raise StorageError(f"invalid USTAR release asset: {exc}") from exc
    return result


def verify_release(policy_path: Path, release_dir: Path) -> str:
    policy_bytes = policy_path.read_bytes()
    policy = validate_policy(parse_json_bytes(policy_bytes, "storage policy"))
    if not release_dir.is_dir():
        raise StorageError("release directory is missing")
    names = sorted(path.name for path in release_dir.iterdir() if path.is_file())
    tar_names = [name for name in names if name.endswith(".tar")]
    if len(tar_names) != 1 or len(names) != 3:
        raise StorageError(
            "release directory must contain exactly one asset, sidecar, and mirror descriptor"
        )
    asset_name = tar_names[0]
    expected_names = sorted(
        [asset_name, f"{asset_name}.mirror.json", f"{asset_name}.sha256"]
    )
    if names != expected_names:
        raise StorageError("release sidecar or mirror descriptor name mismatch")
    archive = (release_dir / asset_name).read_bytes()
    digest = sha256_hex(archive)
    if f"sha256-{digest}.tar" not in asset_name:
        raise StorageError("release asset filename is not content-addressed")
    expected_sidecar = f"{digest}  {asset_name}\n".encode("ascii")
    if (release_dir / f"{asset_name}.sha256").read_bytes() != expected_sidecar:
        raise StorageError("release SHA-256 sidecar mismatch")
    descriptor = require_object(
        load_json(release_dir / f"{asset_name}.mirror.json", "mirror descriptor"),
        "mirror descriptor",
    )
    exact_keys(
        descriptor,
        (
            "kind",
            "version",
            "releaseId",
            "storageClass",
            "authorityState",
            "asset",
            "minimumMirrors",
            "trustPolicyDistribution",
            "instructions",
        ),
        "mirror descriptor",
    )
    if descriptor.get("kind") != "genesis/evidence-release-mirror-v0.1":
        raise StorageError("mirror descriptor kind is unsupported")
    if (
        descriptor.get("version") != "0.1"
        or descriptor.get("authorityState") != "candidate-until-immutable-publication"
        or descriptor.get("trustPolicyDistribution") != "out-of-band"
    ):
        raise StorageError("mirror descriptor version or authority boundary is invalid")
    asset = require_object(descriptor.get("asset"), "mirror descriptor asset")
    exact_keys(
        asset,
        ("name", "sha256", "sizeBytes", "mediaType"),
        "mirror descriptor asset",
    )
    if (
        asset.get("name") != asset_name
        or asset.get("sha256") != digest
        or asset.get("sizeBytes") != len(archive)
        or asset.get("mediaType") != policy["releaseAssetProfile"]["mediaType"]
    ):
        raise StorageError("mirror descriptor does not bind exact release asset")
    storage_class = require_string(descriptor.get("storageClass"), "storage class")
    class_info = class_policy(policy, storage_class)
    if descriptor.get("minimumMirrors") != class_info["requirements"]["minimumMirrors"]:
        raise StorageError("mirror descriptor minimum does not match storage class")
    instructions = require_array(
        descriptor.get("instructions"), "mirror instructions", non_empty=True
    )
    ids = []
    for item in instructions:
        item = require_object(item, "instruction")
        exact_keys(item, ("id", "argv"), "instruction")
        ids.append(require_string(item.get("id"), "instruction id"))
        argv = require_array(item.get("argv"), "instruction argv", non_empty=True)
        for argument in argv:
            require_string(argument, "instruction argument")
    require_sorted_unique(ids, "mirror instruction ids")
    if ids != policy["mirrorProfile"]["requiredInstructions"]:
        raise StorageError("mirror descriptor instructions do not match policy")
    members = safe_tar_members(archive, policy)
    required = set(policy["releaseAssetProfile"]["requiredArchiveEntries"])
    if not required.issubset(members):
        raise StorageError(
            f"release archive missing required entries: {sorted(required - set(members))}"
        )
    if any(
        name.startswith("trust/") or name.startswith(".genesis/") for name in members
    ):
        raise StorageError(
            "release archive contains forbidden trust or local-state material"
        )
    mirror_inside = require_object(
        parse_json_bytes(members["MIRROR.json"], "internal mirror instructions"),
        "internal mirror instructions",
    )
    exact_keys(
        mirror_inside,
        (
            "kind",
            "version",
            "releaseId",
            "storageClass",
            "minimumMirrors",
            "trustPolicyDistribution",
            "instructionIds",
            "rules",
        ),
        "internal mirror instructions",
    )
    if (
        mirror_inside.get("kind") != "genesis/evidence-mirror-instructions-v0.1"
        or mirror_inside.get("version") != "0.1"
        or mirror_inside.get("releaseId") != descriptor.get("releaseId")
        or mirror_inside.get("storageClass") != storage_class
        or mirror_inside.get("minimumMirrors") != descriptor.get("minimumMirrors")
        or mirror_inside.get("trustPolicyDistribution") != "out-of-band"
        or mirror_inside.get("instructionIds") != ids
    ):
        raise StorageError("internal and external mirror instructions disagree")
    rules = require_array(
        mirror_inside.get("rules"), "internal mirror rules", non_empty=True
    )
    for rule in rules:
        require_string(rule, "internal mirror rule")
    manifest = require_object(
        parse_json_bytes(members["release-manifest.json"], "release manifest"),
        "release manifest",
    )
    exact_keys(
        manifest,
        (
            "kind",
            "version",
            "releaseId",
            "storageClass",
            "authorityState",
            "storagePolicySha256",
            "verificationPolicySha256",
            "trustPolicyBundled",
            "entries",
        ),
        "release manifest",
    )
    if (
        manifest.get("kind") != "genesis/evidence-release-manifest-v0.1"
        or manifest.get("version") != "0.1"
        or manifest.get("storageClass") != storage_class
        or manifest.get("releaseId") != descriptor.get("releaseId")
        or manifest.get("trustPolicyBundled") is not False
        or manifest.get("authorityState") != "candidate-until-immutable-publication"
        or manifest.get("storagePolicySha256") != sha256_hex(policy_bytes)
    ):
        raise StorageError("release manifest authority or class binding mismatch")
    manifest_entries = require_array(
        manifest.get("entries"), "release manifest entries", non_empty=True
    )
    expected_payloads = set(members) - {"MIRROR.json", "release-manifest.json"}
    observed_payloads = set()
    for raw in manifest_entries:
        entry = require_object(raw, "release manifest entry")
        exact_keys(entry, ("path", "sha256", "sizeBytes"), "release manifest entry")
        path = require_relative_path(entry["path"], "release manifest path")
        if path in observed_payloads:
            raise StorageError(f"duplicate release manifest payload: {path}")
        observed_payloads.add(path)
        data = members.get(path)
        if data is None:
            raise StorageError(f"release manifest references missing payload: {path}")
        if entry["sha256"] != sha256_hex(data) or entry["sizeBytes"] != len(data):
            raise StorageError(f"release manifest payload mismatch: {path}")
    if observed_payloads != expected_payloads:
        raise StorageError("release manifest does not exactly cover payloads")
    bundle = require_object(
        parse_json_bytes(members["evidence/bundle.json"], "bundle"), "bundle"
    )
    if bundle.get("profile") != storage_class:
        raise StorageError("release bundle profile does not match storage class")
    verification = require_object(
        parse_json_bytes(members["verification/result.json"], "verification result"),
        "verification result",
    )
    exact_keys(
        verification,
        (
            "kind",
            "ok",
            "verifierVersion",
            "compatibilityProfile",
            "bundleSha256",
            "policySha256",
            "artifactTreeSha256",
            "artifactTreeRoot",
            "verifiedAttestations",
            "verifiedSignatures",
            "verifiedArtifacts",
            "verifiedNegativeControls",
        ),
        "verification result",
    )
    if verification.get("ok") is not True:
        raise StorageError("release contains a failing verification result")
    if manifest.get("verificationPolicySha256") != verification.get("policySha256"):
        raise StorageError("release manifest does not bind verification trust policy")
    if verification.get("bundleSha256") != sha256_hex(members["evidence/bundle.json"]):
        raise StorageError(
            "release verification result does not bind archived bundle bytes"
        )
    tree = parse_json_bytes(members["evidence/artifact-tree.json"], "artifact tree")
    if verification.get("artifactTreeSha256") != sha256_hex(canonical_bytes(tree)):
        raise StorageError(
            "release verification result does not bind archived artifact tree"
        )
    return asset_name


def mirror_release(policy_path: Path, source_dir: Path, destination_dir: Path) -> str:
    asset_name = verify_release(policy_path, source_dir)
    try:
        destination_dir.mkdir(parents=True, exist_ok=False)
    except FileExistsError as exc:
        raise StorageError(
            "mirror destination already exists; overwrite is forbidden"
        ) from exc
    try:
        for source in sorted(source_dir.iterdir(), key=lambda path: path.name):
            if not source.is_file():
                raise StorageError(
                    "source release directory contains non-file material"
                )
            destination = destination_dir / source.name
            with source.open("rb") as reader, destination.open("xb") as writer:
                shutil.copyfileobj(reader, writer, length=64 * 1024)
        mirrored_name = verify_release(policy_path, destination_dir)
        if mirrored_name != asset_name:
            raise StorageError("mirror asset identity changed during copy")
        for source in source_dir.iterdir():
            if (
                source.is_file()
                and source.read_bytes() != (destination_dir / source.name).read_bytes()
            ):
                raise StorageError("mirror bytes differ from source")
    except Exception:
        shutil.rmtree(destination_dir, ignore_errors=True)
        raise
    return asset_name


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("--fixture-catalog", type=Path, default=DEFAULT_FIXTURE_CATALOG)
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("check-policy")

    catalog = subparsers.add_parser("render-fixture-catalog")
    catalog.add_argument("--output", type=Path, required=True)

    release = subparsers.add_parser("render-release")
    release.add_argument("--storage-class", required=True)
    release.add_argument("--release-id", required=True)
    release.add_argument("--bundle", type=Path, required=True)
    release.add_argument("--artifact-tree", type=Path, required=True)
    release.add_argument("--artifact-root", type=Path, required=True)
    release.add_argument("--verification-result", type=Path, required=True)
    release.add_argument("--output-dir", type=Path, required=True)

    verify = subparsers.add_parser("verify-release")
    verify.add_argument("--release-dir", type=Path, required=True)

    mirror = subparsers.add_parser("mirror-release")
    mirror.add_argument("--source-dir", type=Path, required=True)
    mirror.add_argument("--destination-dir", type=Path, required=True)

    args = parser.parse_args(argv)
    try:
        policy_path = args.policy.resolve()
        if args.command == "check-policy":
            validate_policy(load_json(policy_path, "storage policy"))
            check_fixture_catalog(args.fixture_catalog.resolve())
            print(f"evidence-storage: policy ok (classes=5 fixtures={len(fixture_roles())})")
        elif args.command == "render-fixture-catalog":
            validate_policy(load_json(policy_path, "storage policy"))
            output = args.output.resolve()
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes(retained_bytes(render_fixture_catalog()))
        elif args.command == "render-release":
            asset = render_release(
                policy_path,
                args.storage_class,
                args.release_id,
                args.bundle.resolve(),
                args.artifact_tree.resolve(),
                args.artifact_root.resolve(),
                args.verification_result.resolve(),
                args.output_dir.resolve(),
            )
            print(asset)
        elif args.command == "verify-release":
            asset = verify_release(policy_path, args.release_dir.resolve())
            print(f"evidence-storage: verified {asset}")
        else:
            asset = mirror_release(
                policy_path, args.source_dir.resolve(), args.destination_dir.resolve()
            )
            print(f"evidence-storage: mirrored {asset}")
    except (StorageError, OSError) as exc:
        print(f"evidence-storage: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
