#!/usr/bin/env python3
"""Independent custody verifier for partial self-hosted package lock reading."""

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
    "binding": "core/pkg::lock-read-authority",
    "decisionInventory": [
        "supported-lock-version-admission", "workspace-and-policy-normalization",
        "requirement-policy-and-strategy-normalization", "tag-policy-selector-compatibility",
        "locked-entry-normalization", "closed-public-lock-term",
        "request-bound-result-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "capability-policy-and-sandbox-path-enforcement",
        "bounded-file-read-and-utf8-validation",
        "generic-toml-decoding-and-term-transport",
        "strict-result-contradiction-checking",
        "effect-log-and-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": False, "productionRequired": True, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_lock_read_authority.py",
    "kind": "genesis/selfhost-pkg-lock-read-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/pkg-lock-read-authority-request-v0.1",
    "resultKind": "genesis/pkg-lock-read-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_LOCK_READ_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_lock_read_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_LOCK_READ_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "all-lock-consumer-authority", "bootstrap-fixpoint", "graph-resolution-authority",
    "h2-package-resolution", "r4-2-e-closure", "registry-authority",
    "release-qualification", "selfhost-toml-codec", "sh-c-closure",
    "workspace-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


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


def source_text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    try:
        return (root / relative).read_text()
    except OSError as error:
        fail(f"cannot read {relative}: {error}")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = source_text(root, profile["sourceModule"], overrides)
    manifest = source_text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = source_text(root, profile["artifact"], overrides)
    adapter = source_text(root, "crates/gc_effects/src/pkg_lock_read_authority.rs", overrides)
    dispatch = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", overrides)
    runner = source_text(root, "crates/gc_effects/src/runner.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::lock-read-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-lock-read::normalize-document", "selfhost/pkg-lock-read::normalize-strategy",
        "selfhost/pkg-lock-read::tag-selector?", "selfhost/hash::hash-term request",
    ):
        if marker not in module:
            fail(f"GenesisCode lock read authority missing marker: {marker}")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody lock read module and binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain lock read module and binding")
    for marker in (
        "pub(crate) struct PkgLockReadAuthority", "SelfhostBootstrapMode::ArtifactOnly",
        "const STEP_LIMIT: u64 = 20_000_000", "const ALLOC_LIMIT: u64 = 80_000_000",
        "toml::from_str", "toml_to_term", "decode_result", "validate_lock",
        "result field set mismatch", "request-h",
    ):
        if marker not in adapter:
            fail(f"Rust lock read adapter missing marker: {marker}")
    parity_marker = '\n#[cfg(any(test, feature = "parity-oracle"))]\nfn dispatch_load_lock_parity'
    if parity_marker not in dispatch:
        fail("test-only typed lock parser boundary missing")
    production = dispatch.split(parity_marker, 1)[0]
    route_marker = '"core/pkg-low::load-lock" => {'
    next_route_marker = '"core/pkg-low::load-package"'
    if route_marker not in production or next_route_marker not in production:
        fail("production load-lock route boundary missing")
    load_route = production.split(route_marker, 1)[1].split(next_route_marker, 1)[0]
    if "const MAX_LOCK_BYTES: u64 = 4 * 1024 * 1024" not in production:
        fail("production load-lock byte ceiling missing")
    for marker in (
        "read_bounded_lock(&lock_path)", "authority.read_toml(&bytes)?",
        "requires the artifact-loaded GenesisCode lock read authority",
    ):
        if marker not in load_route:
            fail(f"production load-lock route missing marker: {marker}")
    if "gc_pkg::GenesisLock::load" in load_route:
        fail("production load-lock route retains typed Rust parser")
    if ".map(PkgLockReadAuthority::load)" not in runner:
        fail("runner does not lazily load lock read authority")
    if 'req.op == "core/pkg-low::load-lock"' not in runner:
        fail("lock read authority is not restricted to exact operation")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (profile["sourceModule"], "crates/gc_effects/src/pkg_lock_read_authority.rs"):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing production authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing lock read specification")
    limitations = "\n".join(row.get("limitations", []))
    if "toml" not in limitations.lower() or "remain" not in limitations.lower():
        fail("semantic ledger does not disclose the production TOML oracle")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("lock read authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_lock_read_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs",
        "crates/gc_effects/src/runner.rs",
    ]
    sources = {path: source_text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    profile_mutation("binding", "core/pkg::legacy-lock-reader")
    profile_mutation("decisionInventory", profile["decisionInventory"][:-1])
    profile_mutation("hostMechanisms", profile["hostMechanisms"][:-1])
    profile_mutation("hostOracle", {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"})
    profile_mutation("productionEntrypoints", ["genesis"])
    profile_mutation("nonclaims", profile["nonclaims"][:-1])
    profile_mutation("sourceSha256", "f" * 64)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    source_mutation(profile["sourceModule"], "normalize-document", "legacy-document", "source")
    source_mutation("selfhost/toolchain_manifest.gc", profile["sourceModule"], "selfhost/missing.gc", "manifest")
    source_mutation(
        profile["artifact"],
        f':path "{profile["sourceModule"]}"',
        ':path "selfhost/missing.gc"',
        "artifact",
    )
    source_mutation("crates/gc_effects/src/pkg_lock_read_authority.rs", "toml::from_str", "legacy_parse", "codec")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", "read_bounded_lock(&lock_path)", "std::fs::read(&lock_path)", "bound")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", "authority.read_toml(&bytes)?", "legacy_read(&bytes)?", "route")
    source_mutation("crates/gc_effects/src/runner.rs", 'req.op == "core/pkg-low::load-lock"', 'req.op.starts_with("core/pkg-low::")', "lazy-route")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides, check_identity=True)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    print(f"selfhost-pkg-lock-read-authority: self-test ok (negative_controls={controls})")
    return controls


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        profile = load_json(args.profile)
        schema = load_json(args.schema)
        validate_all(root, profile, schema)
        if args.artifact and args.artifact.resolve() != (root / profile["artifact"]).resolve():
            fail("artifact argument does not match profile")
        controls = self_test(root, profile, schema) if args.self_test else 0
        print(
            "selfhost-pkg-lock-read-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-lock-read-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
