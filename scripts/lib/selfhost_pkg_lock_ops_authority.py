#!/usr/bin/env python3
"""Independent custody verifier for self-hosted package lock operations."""

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
    "productionOperations", "requestKind", "resultKind", "schema", "sourceModule",
    "sourceSha256", "spec", "version",
}
OPERATIONS = [
    "core/pkg-low::init", "core/pkg-low::add", "core/pkg-low::list",
    "core/pkg-low::bridge",
]
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::lock-ops-authority",
    "decisionInventory": [
        "direct-lock-initialization-and-default-normalization",
        "requirement-mutation-and-metadata-normalization",
        "bridge-lock-mutation-and-artifact-identity",
        "complete-lock-normalization-before-operation",
        "canonical-lock-toml-and-content-identity",
        "closed-list-projection", "request-bound-operation-result",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "canonical-coreform-runtime-map-freeze",
        "capability-policy-and-sandbox-path-enforcement",
        "bounded-file-read-utf8-and-generic-toml-transport",
        "strict-result-contradiction-checking", "atomic-authorized-byte-persistence",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_lock_ops_authority.py",
    "kind": "genesis/selfhost-pkg-lock-ops-authority-v0.1",
    "productionOperations": OPERATIONS,
    "requestKind": "genesis/pkg-lock-ops-authority-request-v0.1",
    "resultKind": "genesis/pkg-lock-ops-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_LOCK_OPS_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_lock_ops_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_LOCK_OPS_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "bridge-object-and-conversion-authority",
    "graph-and-semver-mechanism-authority",
    "h2-package-resolution", "publish-and-registry-authority", "r4-2-e-closure",
    "release-qualification", "selfhost-toml-codec", "sh-c-closure", "workspace-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    data = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(data).hexdigest()


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
    shared = source_text(root, "selfhost/pkg_lock_read_authority_v1.gc", overrides)
    model = source_text(root, "selfhost/pkg_lock_model_authority_v1.gc", overrides)
    writer = source_text(root, "selfhost/pkg_lock_write_authority_v1.gc", overrides)
    manifest = source_text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = source_text(root, profile["artifact"], overrides)
    adapter = source_text(root, "crates/gc_effects/src/pkg_lock_ops_authority.rs", overrides)
    reader = source_text(root, "crates/gc_effects/src/pkg_lock_read_authority.rs", overrides)
    dispatch = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", overrides)
    publish = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs", overrides)
    bridge_dispatch = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_lock.rs", overrides)
    parity = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io/parity.rs", overrides)
    parent = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low.rs", overrides)
    runner = source_text(root, "crates/gc_effects/src/runner.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::lock-ops-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/pkg-lock-ops::init", "selfhost/pkg-lock-ops::add-to-model",
        "selfhost/pkg-lock-ops::bridge-to-model", "selfhost/pkg-lock-ops::dep-key",
        "selfhost/pkg-lock-ops::list-requirements-loop", "selfhost/pkg-lock-ops::list-locked-loop",
        "selfhost/pkg-lock-read::normalize-model-document",
        "selfhost/pkg-lock-write::render-lock", "selfhost/pkg-lock-read::exact-map? request",
        "core/coreform::parse-term", "core/coreform::print-term",
        "selfhost/hash::hash-term request", "core/crypto::blake3 bytes",
    ):
        if marker not in module:
            fail(f"GenesisCode lock ops authority missing marker: {marker}")
    for marker in ("(def selfhost/pkg-lock-read::exact-map?", "(def selfhost/pkg-lock-read::map-has-key-loop?"):
        if marker not in shared:
            fail(f"shared lock normalization module missing marker: {marker}")
    if ("(def selfhost/pkg-lock-read::normalize-model-document\n" not in model
            or "(def selfhost/pkg-lock-write::render-lock\n" not in writer):
        fail("lock ops authority dependencies are not explicitly custodied")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody lock ops module and binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain lock ops module and binding")

    for marker in (
        "pub(crate) enum PkgLockOpsDecision", "pub(crate) fn init_lock",
        "pub(crate) fn add_lock_toml",
        "pub(crate) fn list_lock_toml",
        "pub(crate) fn bridge_lock_toml", "pub(crate) struct PkgBridgeLockFacts",
        "fn decode_ops_result", "fn validate_list_entries", "OPS_REQUEST_KIND",
        "OPS_RESULT_KIND", "bytes and :lock-h are malformed or contradictory",
    ):
        if marker not in adapter:
            fail(f"Rust lock ops adapter missing marker: {marker}")
    for marker in (
        "const MAX_LOCK_BYTES: u64 = 4 * 1024 * 1024", "read_bounded_lock",
        "bounded_lock_reader_rejects_oversized_input",
    ):
        if marker not in parent:
            fail(f"bounded lock transport missing marker: {marker}")
    for operation in OPERATIONS:
        if operation not in reader:
            fail(f"runner lazy authority set missing {operation}")
        route = publish if operation == "core/pkg-low::bridge" else dispatch
        if f'"{operation}" =>' not in route:
            fail(f"lock dispatcher missing {operation}")
    for marker in (
        "authority.init_lock(payload)?",
        "authority.add_lock_toml(&bytes, payload)?",
        "authority.list_lock_toml(&bytes, payload)?", "atomic_write_text(&lock_path, &bytes)",
        "atomic_write_text(&lock_write_path, &bytes)", "lock_ops_authority_unavailable(op_eff)",
    ):
        if marker not in dispatch:
            fail(f"production lock ops route missing marker: {marker}")
    for marker in (
        '#[path = "dispatch_publish/bridge_objects.rs"]',
        '"core/pkg-low::bridge" => bridge_objects::dispatch_bridge(',
    ):
        if marker not in publish:
            fail(f"bridge dispatcher missing delegated authority route: {marker}")
    bridge_objects = source_text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs", overrides
    )
    for marker in ("BridgeLockUpdate", "bridge_lock::update_lock(", "authority.finalize_bridge"):
        if marker not in bridge_objects:
            fail(f"bridge object adapter missing lock authority route: {marker}")
    for marker in (
        "PkgBridgeLockFacts", "authority.bridge_lock_toml(&bytes, facts)?",
        "read_bounded_lock", "atomic_write_text(&write_path, &bytes)",
    ):
        if marker not in bridge_dispatch:
            fail(f"bridge lock mechanism adapter missing marker: {marker}")
    for marker in (
        "gc_pkg::GenesisLock", "set_requirement_with_metadata", "to_toml_canonical",
    ):
        if marker in publish or marker in bridge_dispatch or marker in bridge_objects:
            fail(f"production bridge lock route retains Rust semantic oracle: {marker}")
    if bridge_objects.index("requires the artifact-loaded GenesisCode bridge authority") > bridge_objects.index("let provenance_root = put!"):
        fail("bridge authority presence is not checked before bridge object side effects")
    if bridge_objects.index("authority.finalize_bridge") > bridge_objects.index("bridge_lock::update_lock("):
        fail("bridge lock mutation is attempted before bridge authority finalization")
    fallback = '#[cfg(any(test, feature = "parity-oracle"))]'
    if fallback not in dispatch:
        fail("typed lock operation fallback is not compile-time parity-only")
    direct_routes = dispatch.split('"core/pkg-low::init" =>', 1)[1].split('"core/pkg-low::load-lock" =>', 1)[0]
    for marker in (
        "GenesisLock::empty", "GenesisLock::load", "to_toml_canonical",
        "set_requirement_with_metadata",
    ):
        if marker in direct_routes:
            fail(f"production init/add/list route retains Rust semantic oracle: {marker}")
    for marker in (
        "let mut lock = gc_pkg::GenesisLock::empty(workspace)",
        "let mut lock = match gc_pkg::GenesisLock::load(&path)",
        "let lock = match gc_pkg::GenesisLock::load(&path)",
        "lock.set_requirement_with_metadata(", "let bytes = lock.to_toml_canonical().into_bytes()",
    ):
        if marker not in parity:
            fail(f"parity oracle lost retained legacy marker: {marker}")
    if "PkgLockReadAuthority::required_for_request(&req.op, &req.payload)" not in runner:
        fail("runner does not use the closed lock authority operation set")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (
        profile["sourceModule"], "crates/gc_effects/src/pkg_lock_ops_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_lock.rs",
    ):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing production authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing lock ops specification")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("semantic ledger missing lock ops verifier")
    limitations = "\n".join(row.get("limitations", [])).lower()
    if "direct init" in limitations or "toml" not in limitations or "h0" not in limitations:
        fail("semantic ledger does not disclose partial lock ops authority and TOML host mechanism")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("lock ops authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/pkg_lock_read_authority_v1.gc",
        "selfhost/pkg_lock_model_authority_v1.gc", "selfhost/pkg_lock_write_authority_v1.gc",
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_lock_ops_authority.rs",
        "crates/gc_effects/src/pkg_lock_read_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io/parity.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_lock.rs",
        "crates/gc_effects/src/runner_cap_pkg_low.rs", "crates/gc_effects/src/runner.rs",
    ]
    sources = {path: source_text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-lock-ops"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("productionOperations", profile["productionOperations"][:-1]),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        profile_mutation(name, value)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    opened = copy.deepcopy(profile)
    opened["extra"] = True
    mutations.append((opened, {}, "profile-closure"))
    source_mutation(profile["sourceModule"], "              :payload\n", "              :wrong\n", "request-closure")
    source_mutation(profile["sourceModule"], "selfhost/pkg-lock-ops::add-to-model", "selfhost/pkg-lock-ops::legacy-add", "source")
    source_mutation(profile["sourceModule"], "core/coreform::parse-term", "core/coreform::legacy-parse", "runtime-map-freeze")
    source_mutation("selfhost/pkg_lock_read_authority_v1.gc", "(def selfhost/pkg-lock-read::exact-map?", "(def selfhost/pkg-lock-read::count-only?", "shared-closure")
    source_mutation("selfhost/pkg_lock_model_authority_v1.gc", "(def selfhost/pkg-lock-read::normalize-model-document\n", "(def selfhost/pkg-lock-read::legacy-model-document\n", "model-dependency")
    source_mutation("selfhost/pkg_lock_write_authority_v1.gc", "(def selfhost/pkg-lock-write::render-lock\n", "(def selfhost/pkg-lock-write::legacy-render-lock\n", "writer-dependency")
    source_mutation("selfhost/toolchain_manifest.gc", profile["sourceModule"], "selfhost/missing.gc", "manifest")
    source_mutation(profile["artifact"], f':path "{profile["sourceModule"]}"', ':path "selfhost/missing.gc"', "artifact")
    source_mutation("crates/gc_effects/src/pkg_lock_ops_authority.rs", "fn decode_ops_result", "fn legacy_decode", "decoder")
    source_mutation("crates/gc_effects/src/pkg_lock_ops_authority.rs", "bytes and :lock-h are malformed or contradictory", "bytes accepted", "hash-contradiction")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low.rs", "MAX_LOCK_BYTES", "UNBOUNDED_LOCK_BYTES", "bound")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", "authority.init_lock(payload)?", "legacy_init(payload)?", "init-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", "authority.add_lock_toml(&bytes, payload)?", "legacy_add(&bytes, payload)?", "add-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io.rs", "authority.list_lock_toml(&bytes, payload)?", "legacy_list(&bytes, payload)?", "list-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_lock.rs", "authority.bridge_lock_toml(&bytes, facts)?", "legacy_bridge(&bytes, facts)?", "bridge-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs", '"core/pkg-low::bridge" => bridge_objects::dispatch_bridge(', '"core/pkg-low::bridge" => legacy_bridge(', "bridge-dispatch")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs", "bridge_lock::update_lock(", "legacy_bridge_lock(", "bridge-lock-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_lock_io/parity.rs", "let mut lock = match gc_pkg::GenesisLock::load(&path)", "let mut lock = match gc_pkg::LegacyLock::load(&path)", "parity-oracle")
    source_mutation("crates/gc_effects/src/pkg_lock_read_authority.rs", '"core/pkg-low::list"', '"core/pkg-low::legacy-list"', "lazy-route-set")
    source_mutation("crates/gc_effects/src/runner.rs", "PkgLockReadAuthority::required_for_request(&req.op, &req.payload)", "req.op.starts_with(\"core/pkg-low::\")", "lazy-route-use")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides, check_identity=True)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 27:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-pkg-lock-ops-authority: self-test ok (negative_controls={controls})")
    return controls


def write_identities(path: Path, profile, root: Path) -> None:
    source_path = root / profile["sourceModule"]
    profile["sourceSha256"] = source_identity(profile["sourceModule"], source_path.read_bytes())
    profile["contentIdentitySha256"] = canonical_identity(profile)
    path.write_text(json.dumps(profile, indent=2) + "\n")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write-identities", action="store_true")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        profile_path = args.profile if args.profile.is_absolute() else root / args.profile
        schema_path = args.schema if args.schema.is_absolute() else root / args.schema
        profile = load_json(profile_path)
        schema = load_json(schema_path)
        if args.write_identities:
            validate_profile(profile, schema, check_identity=False)
            write_identities(profile_path, profile, root)
            profile = load_json(profile_path)
        validate_all(root, profile, schema)
        if args.artifact and args.artifact.resolve() != (root / profile["artifact"]).resolve():
            fail("artifact argument does not match profile")
        controls = self_test(root, profile, schema) if args.self_test else 0
        print(
            "selfhost-pkg-lock-ops-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-lock-ops-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
