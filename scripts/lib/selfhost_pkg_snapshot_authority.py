#!/usr/bin/env python3
"""Independent custody verifier for self-hosted package snapshot identities."""

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


def parse_json(text: str, name: str):
    try:
        value = json.loads(text, object_pairs_hook=unique_object)
    except json.JSONDecodeError as error:
        fail(f"cannot parse {name}: {error}")
    if not isinstance(value, dict):
        fail(f"JSON root is not an object: {name}")
    return value


FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionOperations", "requestKind", "resultKind", "schema", "sourceModule",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::snapshot-authority",
    "decisionInventory": [
        "canonical-module-object-bytes-and-content-identities",
        "module-identity-recomputation-and-substitution-rejection",
        "canonical-package-snapshot-object-and-content-identity",
        "request-bound-ordered-artifact-plan",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "sandboxed-manifest-and-module-byte-transport",
        "frontend-parse-and-canonical-form-transport",
        "capability-and-cumulative-store-budget-enforcement",
        "exact-authorized-byte-content-addressed-storage",
        "strict-result-and-store-identity-contradiction-checking",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_snapshot_authority.py",
    "kind": "genesis/selfhost-pkg-snapshot-authority-v0.1",
    "productionOperations": ["core/pkg-low::snapshot"],
    "requestKind": "genesis/pkg-snapshot-authority-request-v0.1",
    "resultKind": "genesis/pkg-snapshot-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_SNAPSHOT_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_snapshot_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_SNAPSHOT_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "h2-package-resolution",
    "package-manifest-and-source-frontend-authority", "publish-and-registry-authority",
    "r4-2-e-closure", "release-qualification", "sh-c-closure", "workspace-authority",
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


def text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    try:
        return (root / relative).read_text()
    except OSError as error:
        fail(f"cannot read {relative}: {error}")


def require_markers(source: str, markers, label: str) -> None:
    for marker in markers:
        if marker not in source:
            fail(f"{label} missing marker: {marker}")


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


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = text(root, profile["sourceModule"], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_effects/src/pkg_snapshot_authority.rs", overrides)
    loader = text(root, "crates/gc_effects/src/pkg_lock_read_authority.rs", overrides)
    route = text(root, "crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs", overrides)
    dispatch = text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs", overrides)
    tests = text(root, "crates/gc_effects/tests/sync_registry/cases_a.rs", overrides)
    ledger = parse_json(text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger")

    require_markers(module, (
        "(def core/pkg::snapshot-authority", profile["requestKind"], profile["resultKind"],
        "selfhost/hash::hash-module forms", "selfhost/printer::print-term term",
        "core/crypto::blake3 bytes", ":type (quote :vcs/snapshot)",
        "[:modules :name :obligations :pkg :version]", ":artifacts", ":snapshot",
    ), "GenesisCode snapshot authority")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody snapshot module and binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain snapshot module and binding")
    require_markers(adapter, (
        "pub(crate) fn construct_snapshot", "fn decode_snapshot_result",
        "fn decode_snapshot_object", "one artifact per module plus the snapshot",
        "snapshot object :term, :bytes, and :h are malformed or contradictory",
        "snapshot result :snapshot contradicts the final artifact",
        "authority_constructs_exact_module_and_snapshot_objects",
        "authority_rejects_module_identity_substitution",
        "object_decoder_rejects_bytes_and_hash_substitution",
    ), "Rust snapshot adapter")
    require_markers(loader, (
        '#[path = "pkg_snapshot_authority.rs"]', "snapshot_authority: Option<Value>",
        "environment.get(snapshot::SNAPSHOT_BINDING)", '"core/pkg-low::snapshot"',
    ), "snapshot authority loader")
    require_markers(dispatch, ("handle_snapshot(", "pkg_lock_read_authority,"), "snapshot dispatch")
    snapshot_route_marker = "pub(super) fn handle_snapshot"
    if snapshot_route_marker not in route:
        fail("snapshot mechanism route is absent")
    snapshot_route = route[route.index(snapshot_route_marker):]
    require_markers(snapshot_route, (
        "missing selfhost package snapshot authority", "snapshot_authority.construct_snapshot(facts)?",
        "for artifact in &plan.artifacts", "store_put_with_budget(",
        "selfhost package snapshot authority/store identity contradiction",
    ), "snapshot mechanism route")
    if snapshot_route.index("missing selfhost package snapshot authority") > snapshot_route.index("PackageManifest::load"):
        fail("snapshot authority is checked after package I/O")
    retired = (":vcs/snapshot", "snapshot_bytes", "Term::Vector(modules_out)")
    if any(marker in snapshot_route for marker in retired):
        fail("production snapshot route retains retired object construction")
    if len(module.splitlines()) > 700 or len(adapter.splitlines()) > 700 or len(route.splitlines()) > 700:
        fail("snapshot authority decomposition exceeds 700 lines")
    require_markers(tests, (
        "pkg_snapshot_authority_constructs_exact_objects_and_is_required_before_storage",
        "missing selfhost package snapshot authority", "hash_module(&canonical)",
        "read_dir(&missing_store).unwrap().count(), 0",
    ), "snapshot integration controls")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (profile["sourceModule"], "crates/gc_effects/src/pkg_snapshot_authority.rs",
                 "crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs"):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"ledger missing production path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("ledger missing snapshot specification")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("ledger missing snapshot verifier")
    limitations = "\n".join(row.get("limitations", [])).lower()
    if "snapshot" not in limitations or "h0" not in limitations or "manifest" not in limitations:
        fail("ledger does not disclose snapshot authority and residual boundary")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("snapshot source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [profile["sourceModule"], "selfhost/toolchain_manifest.gc", profile["artifact"],
             "crates/gc_effects/src/pkg_snapshot_authority.rs",
             "crates/gc_effects/src/pkg_lock_read_authority.rs",
             "crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs",
             "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs",
             "crates/gc_effects/tests/sync_registry/cases_a.rs",
             "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json"]
    sources = {path: text(root, path, {}) for path in paths}
    mutations = []

    def mutate_profile(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (("binding", "core/pkg::legacy-snapshot"),
                        ("decisionInventory", profile["decisionInventory"][:-1]),
                        ("hostMechanisms", profile["hostMechanisms"][:-1]),
                        ("nonclaims", profile["nonclaims"][:-1]),
                        ("sourceSha256", "f" * 64)):
        mutate_profile(name, value)
    opened = copy.deepcopy(profile)
    opened["extra"] = True
    mutations.append((opened, {}, "profile-closure"))

    def mutate_source(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent: {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    mutate_source(profile["sourceModule"], "selfhost/hash::hash-module forms", "module-h", "module-hash")
    mutate_source(profile["sourceModule"], "core/crypto::blake3 bytes", "bytes", "object-hash")
    mutate_source(profile["sourceModule"], "[:facts :kind :v]", "[:facts :kind]", "request-closure")
    mutate_source("selfhost/toolchain_manifest.gc", profile["sourceModule"], "selfhost/missing.gc", "manifest")
    mutate_source(profile["artifact"], f':path "{profile["sourceModule"]}"', ':path "selfhost/missing.gc"', "artifact")
    mutate_source("crates/gc_effects/src/pkg_snapshot_authority.rs", "fn decode_snapshot_result", "fn decode_legacy_result", "decoder")
    mutate_source("crates/gc_effects/src/pkg_snapshot_authority.rs", "snapshot result :snapshot contradicts the final artifact", "snapshot accepted", "result-identity")
    mutate_source("crates/gc_effects/src/pkg_lock_read_authority.rs", '"core/pkg-low::snapshot"', '"core/pkg-low::legacy-snapshot"', "lazy-route")
    mutate_source("crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs", "snapshot_authority.construct_snapshot(facts)?", "legacy_snapshot(facts)?", "authority-route")
    mutate_source("crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs", "for artifact in &plan.artifacts", "for artifact in &[]", "exact-write")
    mutate_source("crates/gc_effects/tests/sync_registry/cases_a.rs", "pkg_snapshot_authority_constructs_exact_objects_and_is_required_before_storage", "legacy_snapshot_test", "integration-control")
    controls = 0
    for changed, overrides, name in mutations:
        try:
            validate_all(root, changed, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 17:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-pkg-snapshot-authority: self-test ok (negative_controls={controls})")
    return controls


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
        profile = parse_json(profile_path.read_text(), str(profile_path))
        schema = parse_json(schema_path.read_text(), str(schema_path))
        if args.write_identities:
            validate_profile(profile, schema, check_identity=False)
            profile["sourceSha256"] = source_identity(profile["sourceModule"], (root / profile["sourceModule"]).read_bytes())
            profile["contentIdentitySha256"] = canonical_identity(profile)
            profile_path.write_text(json.dumps(profile, indent=2) + "\n")
        validate_all(root, profile, schema)
        if args.artifact and args.artifact.resolve() != (root / profile["artifact"]).resolve():
            fail("artifact argument does not match profile")
        controls = self_test(root, profile, schema) if args.self_test else 0
        print(f"selfhost-pkg-snapshot-authority: ok profile={profile['contentIdentitySha256']} controls={controls}")
        return 0
    except (CheckError, OSError) as error:
        print(f"selfhost-pkg-snapshot-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
