#!/usr/bin/env python3
"""Independent custody verifier for partial self-hosted package lock writing."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import sys
from pathlib import Path


class CheckError(RuntimeError):
    pass


def fail(message: str) -> None:
    raise CheckError(message)


def unique_object(pairs):
    out = {}
    for key, value in pairs:
        if key in out:
            fail(f"duplicate JSON key: {key}")
        out[key] = value
    return out


def load_json(path: Path):
    try:
        value = json.loads(path.read_text(), object_pairs_hook=unique_object)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"JSON root is not an object: {path}")
    return value


FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModule",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::lock-write-authority",
    "decisionInventory": [
        "lock-payload-normalization", "requirement-policy-and-strategy-normalization",
        "resolution-strategy-inference", "legacy-lock-version-upgrade",
        "canonical-lock-toml-serialization", "canonical-lock-content-identity",
        "lock-and-update-final-serialization",
        "request-bound-result-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "capability-policy-and-sandbox-path-enforcement", "atomic-file-persistence",
        "strict-result-contradiction-checking", "effect-log-and-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_lock_write_authority.py",
    "kind": "genesis/selfhost-pkg-lock-write-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-lock-write-authority-request-v0.1",
    "resultKind": "genesis/pkg-lock-write-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_LOCK_WRITE_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_lock_write_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_LOCK_WRITE_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "graph-resolution-authority", "h2-package-resolution",
    "lock-read-parse-authority", "r4-2-e-closure", "registry-authority",
    "release-qualification", "sh-c-closure", "workspace-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def source_identity(relative: str, data: bytes) -> str:
    digest = hashlib.sha256()
    digest.update(relative.encode())
    digest.update(b"\0")
    digest.update(data)
    digest.update(b"\0")
    return digest.hexdigest()


def validate_profile(profile, schema, check_identity=True) -> None:
    if set(profile) != FIELDS:
        fail("profile field closure drift")
    if (schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
            or schema.get("type") != "object" or schema.get("additionalProperties") is not False
            or set(schema.get("required", [])) != FIELDS
            or set(schema.get("properties", {})) != FIELDS):
        fail("schema closure drift")
    for name, expected in CONSTANTS.items():
        if profile.get(name) != expected:
            fail(f"profile {name} drift")
    if set(profile.get("nonclaims", [])) != NONCLAIMS:
        fail("profile nonclaim inventory drift")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(profile.get("auditDate", ""))):
        fail("profile auditDate invalid")
    for name in ("contentIdentitySha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(name, ""))):
            fail(f"profile {name} invalid")
    if check_identity and canonical_identity(profile) != profile["contentIdentitySha256"]:
        fail("profile content identity mismatch")


def text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    try:
        return (root / relative).read_text()
    except OSError as error:
        fail(f"cannot read {relative}: {error}")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = text(root, profile["sourceModule"], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_effects/src/pkg_lock_write_authority.rs", overrides)
    save_lock = text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io/save_lock.rs", overrides)
    resolution = text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", overrides)
    pkg_dispatch = text(root, "crates/gc_effects/src/runner_cap_pkg_low.rs", overrides)
    runner = text(root, "crates/gc_effects/src/runner.rs", overrides)
    dispatch = text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::lock-write-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-lock-write::render-lock", "selfhost/pkg-lock-write::effective-version",
        "core/crypto::blake3 bytes", "selfhost/hash::hash-term request",
    ):
        if marker not in module:
            fail(f"GenesisCode lock authority missing marker: {marker}")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody lock authority module and binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain lock authority module and binding")
    for marker in (
        "pub(crate) struct PkgLockWriteAuthority", "SelfhostBootstrapMode::ArtifactOnly",
        "const STEP_LIMIT: u64 = 20_000_000", "const ALLOC_LIMIT: u64 = 80_000_000",
        "decode_result", "result bytes and :lock-h contradict", "result field set mismatch",
    ):
        if marker not in adapter:
            fail(f"Rust lock authority adapter missing marker: {marker}")
    parity_boundary = '\n#[cfg(any(test, feature = "parity-oracle"))]\nfn dispatch_save_lock_parity'
    if parity_boundary not in save_lock:
        fail("test-only parity oracle boundary missing")
    production = save_lock.split(parity_boundary, 1)[0]
    for marker in (
        "authority.write(payload)?", "sandbox_path_write(", "atomic_write_text(&lock_path, &bytes)",
        "requires the artifact-loaded GenesisCode lock write authority",
    ):
        if marker not in production:
            fail(f"production save-lock route missing marker: {marker}")
    if "gc_pkg::GenesisLock" in production or "to_toml_canonical" in production:
        fail("production save-lock route retains Rust serialization oracle")
    if "fn dispatch_save_lock_parity" not in save_lock:
        fail("test-only parity oracle declaration missing")
    if ".map(PkgLockWriteAuthority::load)" not in runner or "pkg_lock_write_authority.as_mut()" not in runner:
        fail("runner does not lazily load and forward package lock authority")
    if "if pkg_lock_write_authority.is_none()" not in runner or "if pkg_lock_read_authority.is_none()" not in runner:
        fail("runner lock authority lazy-load boundaries are missing")
    lock_writer_load = runner.split("if pkg_lock_write_authority.is_none()", 1)[1].split(
        "if pkg_lock_read_authority.is_none()", 1
    )[0]
    for operation in ("core/pkg-low::save-lock", "core/pkg-low::lock", "core/pkg-low::update"):
        if f'"{operation}"' not in lock_writer_load:
            fail(f"package lock authority is not loaded for {operation}")
    if "pkg_lock_write_authority" not in dispatch or "dispatch_save_lock(" not in dispatch:
        fail("lock dispatch does not forward package lock authority")
    if resolution.count("render_resolved_lock(") < 3:
        fail("lock and update do not both route final models through one writer boundary")
    finalizer = resolution.split("fn render_resolved_lock(", 1)[1]
    resolution_parity = '\n    #[cfg(any(test, feature = "parity-oracle"))]\n'
    if resolution_parity not in finalizer:
        fail("lock/update native writer parity boundary missing")
    production_finalizer = finalizer.split(resolution_parity, 1)[0]
    for marker in (
        "authority.write_model(lock_path, lock)",
        "lock and update require the artifact-loaded GenesisCode lock write authority",
    ):
        if marker not in finalizer:
            fail(f"lock/update finalizer missing marker: {marker}")
    for forbidden in ("to_toml_canonical", "blake3::hash"):
        if forbidden in production_finalizer:
            fail(f"production lock/update finalizer retains native writer: {forbidden}")
    if resolution.count("atomic_write_text(&lock_write_path, &bytes)") < 2:
        fail("lock and update do not persist the exact authority bytes")
    if "dispatch_resolution::dispatch_resolution(" not in pkg_dispatch:
        fail("package dispatch does not forward lock-write authority to resolution operations")
    resolution_dispatch = pkg_dispatch.split("dispatch_resolution::dispatch_resolution(", 1)[1].split(
        ");", 1
    )[0]
    if "pkg_lock_write_authority" not in resolution_dispatch:
        fail("resolution dispatch call omits lock-write authority")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (profile["sourceModule"], "crates/gc_effects/src/pkg_lock_write_authority.rs"):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing production authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing lock authority specification")
    limitations = "\n".join(row.get("limitations", []))
    if "lock" not in limitations.lower() or "remain" not in limitations.lower():
        fail("semantic ledger lacks partial lock authority and residual limitation")

    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("lock authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_lock_write_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io/save_lock.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs",
        "crates/gc_effects/src/runner_cap_pkg_low.rs",
        "crates/gc_effects/src/runner.rs",
    ]
    sources = {path: text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    profile_mutation("binding", "core/pkg::legacy-lock-writer")
    profile_mutation("decisionInventory", profile["decisionInventory"][:-1])
    profile_mutation("hostMechanisms", profile["hostMechanisms"][:-1])
    profile_mutation("productionEntrypoints", ["genesis"])
    profile_mutation("nonclaims", profile["nonclaims"][:-1])
    profile_mutation("sourceSha256", "f" * 64)
    opened = copy.deepcopy(profile)
    opened["extra"] = True
    mutations.append((opened, {}, "profile closure"))
    mutations.extend([
        (profile, {profile["sourceModule"]: sources[profile["sourceModule"]].replace(
            "(def core/pkg::lock-write-authority", "(def core/pkg::legacy", 1)}, "source binding"),
        (profile, {"selfhost/toolchain_manifest.gc": sources["selfhost/toolchain_manifest.gc"].replace(
            f'    "{profile["sourceModule"]}"\n', "", 1)}, "module custody"),
        (profile, {profile["artifact"]: sources[profile["artifact"]].replace(
            f':path "{profile["sourceModule"]}"', ':path "selfhost/legacy.gc"', 1)}, "artifact custody"),
        (profile, {"crates/gc_effects/src/pkg_lock_write_authority.rs": sources[
            "crates/gc_effects/src/pkg_lock_write_authority.rs"].replace(
                "result bytes and :lock-h contradict", "result accepted", 1)}, "hash contradiction"),
        (profile, {"crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io/save_lock.rs": sources[
            "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io/save_lock.rs"].replace(
                "authority.write(payload)?", "legacy_writer(payload)?", 1)}, "production route"),
        (profile, {"crates/gc_effects/src/runner.rs": sources["crates/gc_effects/src/runner.rs"].replace(
            ".map(PkgLockWriteAuthority::load)", ".map(StoreAuthority::load)", 1)}, "runner load"),
        (profile, {"crates/gc_effects/src/runner.rs": sources["crates/gc_effects/src/runner.rs"].replace(
            '| "core/pkg-low::lock"', '| "core/pkg-low::legacy-lock"', 1)}, "lock route load"),
        (profile, {"crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs": sources[
            "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs"].replace(
                "authority.write_model(lock_path, lock)", "legacy_writer(lock)", 1)}, "resolution writer route"),
        (profile, {"crates/gc_effects/src/runner_cap_pkg_low.rs": sources[
            "crates/gc_effects/src/runner_cap_pkg_low.rs"].replace(
                "            pkg_lock_write_authority,\n            pkg_resolution_identity_authority,",
                "            None,\n            pkg_resolution_identity_authority,", 1)}, "resolution authority forwarding"),
    ])
    controls = 0
    for candidate, overrides, label in mutations:
        try:
            validate_all(root, candidate, schema, overrides, True)
        except CheckError:
            controls += 1
        else:
            fail(f"mutation survived: {label}")
    if controls != 16:
        fail(f"negative control inventory drift: {controls}")
    return controls


def write_identities(path: Path, profile, root: Path) -> None:
    source_path = root / profile["sourceModule"]
    profile["sourceSha256"] = source_identity(profile["sourceModule"], source_path.read_bytes())
    profile["contentIdentitySha256"] = canonical_identity(profile)
    path.write_text(json.dumps(profile, indent=2) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write-identities", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    profile_path = args.profile if args.profile.is_absolute() else root / args.profile
    schema_path = args.schema if args.schema.is_absolute() else root / args.schema
    try:
        profile, schema = load_json(profile_path), load_json(schema_path)
        if args.write_identities:
            validate_profile(profile, schema, check_identity=False)
            write_identities(profile_path, profile, root)
            profile = load_json(profile_path)
        validate_all(root, profile, schema)
        controls = self_test(root, profile, schema) if args.self_test else 0
    except CheckError as error:
        print(f"selfhost-pkg-lock-write-authority: {error}", file=sys.stderr)
        return 1
    suffix = f" negative_controls={controls}" if args.self_test else ""
    print(f"selfhost-pkg-lock-write-authority: ok content_identity={profile['contentIdentitySha256']}{suffix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
