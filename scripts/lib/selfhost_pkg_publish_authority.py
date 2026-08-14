#!/usr/bin/env python3
"""Independent custody verifier for self-hosted package publish authority."""

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
    result = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


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
    "productionOperations", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
SOURCE_MODULES = [
    "selfhost/pkg_publish_glob_v1.gc",
    "selfhost/pkg_publish_policy_core_v1.gc",
    "selfhost/pkg_publish_policy_v1.gc",
    "selfhost/pkg_publish_authority_core_v1.gc",
    "selfhost/pkg_publish_authority_inspect_v1.gc",
    "selfhost/pkg_publish_authority_objects_v1.gc",
    "selfhost/pkg_publish_authority_assurance_core_v1.gc",
    "selfhost/pkg_publish_authority_requirements_v1.gc",
    "selfhost/pkg_publish_authority_qualification_v1.gc",
    "selfhost/pkg_publish_authority_crypto_v1.gc",
    "selfhost/pkg_publish_authority_prepare_v1.gc",
    "selfhost/pkg_publish_authority_finalize_crypto_v1.gc",
    "selfhost/pkg_publish_authority_finalize_policy_v1.gc",
    "selfhost/pkg_publish_authority_finalize_v1.gc",
    "selfhost/pkg_publish_authority_v1.gc",
]
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::publish-authority",
    "decisionInventory": [
        "policy-commit-evidence-and-assurance-admission",
        "ordered-object-and-cryptographic-request-construction",
        "signer-threshold-role-and-independence-decision",
        "exact-publication-provenance-and-sync-plan",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "bounded-ref-and-content-addressed-artifact-transport",
        "canonical-term-byte-and-hash-contradiction-checking",
        "strict-ed25519-mechanism-verification",
        "capability-timeout-and-byte-budget-enforcement",
        "exact-authority-returned-sync-plan-execution",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_publish_authority.py",
    "kind": "genesis/selfhost-pkg-publish-authority-v0.1",
    "productionOperations": ["core/pkg-low::publish"],
    "requestKind": "genesis/pkg-publish-authority-request-v0.1",
    "resultKind": "genesis/pkg-publish-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_PUBLISH_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_PUBLISH_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "h2-package-resolution",
    "package-manifest-and-source-frontend-authority", "r4-2-e-closure",
    "registry-transport-authority", "release-qualification", "sh-c-closure",
    "workspace-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def source_identity(modules: list[str], sources: dict[str, str]) -> str:
    digest = hashlib.sha256()
    for relative in modules:
        digest.update(relative.encode())
        digest.update(b"\0")
        digest.update(sources[relative].encode())
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
    sources = {path: text(root, path, overrides) for path in SOURCE_MODULES}
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_effects/src/pkg_publish_authority.rs", overrides)
    crypto = text(root, "crates/gc_effects/src/pkg_publish_authority_crypto.rs", overrides)
    loader = text(root, "crates/gc_effects/src/pkg_lock_read_authority.rs", overrides)
    dispatch = text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs", overrides)
    route_path = "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/publish_authority.rs"
    route = text(root, route_path, overrides)
    payload = text(root, "crates/gc_effects/src/runner_pkg_payload.rs", overrides)
    tests = text(root, "crates/gc_effects/tests/sync_registry/cases_a.rs", overrides)
    adapter_tests = text(root, "crates/gc_effects/src/pkg_publish_authority_adapter_tests.rs", overrides)
    ledger = parse_json(text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger")

    require_markers(sources[SOURCE_MODULES[-1]], (
        "(def core/pkg::publish-authority", ":inspect", ":prepare", ":finalize",
        "core/pkg/bad-authority-request",
    ), "GenesisCode publish dispatcher")
    require_markers(sources["selfhost/pkg_publish_policy_v1.gc"], (
        "selfhost/pkg-publish-policy::parse", "selfhost/pkg-publish-policy::select",
        "(quote :tags)", "(quote :main)", "(quote :dev)",
    ), "GenesisCode publish policy")
    require_markers(sources["selfhost/pkg_publish_authority_prepare_v1.gc"], (
        "selfhost/pkg-publish-authority::prepare", ":crypto-requests", ":prepare-h",
    ), "GenesisCode publish prepare")
    require_markers(sources["selfhost/pkg_publish_authority_finalize_v1.gc"], (
        "selfhost/pkg-publish-authority::finalize", "publication-provenance",
        "publication-sync", ":crypto-facts",
    ), "GenesisCode publish finalize")
    for source in SOURCE_MODULES:
        if f'    "{source}"' not in manifest:
            fail(f"toolchain manifest does not custody {source}")
        if f':path "{source}"' not in artifact:
            fail(f"published artifact does not contain {source}")
    if profile["binding"] not in manifest or profile["binding"] not in artifact:
        fail("toolchain artifact does not export publish authority")
    require_markers(adapter, (
        "pub(crate) fn inspect_publish", "pub(crate) fn prepare_publish",
        "pub(crate) fn finalize_publish", "fn decode_phase_result",
        "fn decode_finalize_value", "require_embedded_hash", "publish_exact_map",
        "expected_provenance", "if sync != expected_sync", "verify_crypto_request",
    ), "Rust publish adapter")
    require_markers(crypto, (
        'alg != "ed25519"', "verify_strict", "vcs\\0commit-sign\\0",
        "vcs\\0commit-signing-hash\\0", "mechanical_signing_hash",
    ), "Rust publish crypto mechanism")
    require_markers(loader, (
        '#[path = "pkg_publish_authority.rs"]', "publish_authority: Option<Value>",
        "environment.get(publish::PUBLISH_BINDING)", '"core/pkg-low::publish"',
    ), "publish authority loader")
    require_markers(dispatch, (
        '#[path = "dispatch_publish/publish_authority.rs"]',
        '"core/pkg-low::publish" => publish_authority::handle_publish(',
    ), "publish dispatch")
    require_markers(route, (
        "pub(super) fn handle_publish", "requires the artifact-loaded GenesisCode publish authority",
        "authority.inspect_publish(&facts)?", "authority.prepare_publish(",
        "authority.finalize_publish(",
        'call_capability_with_runtime(\n        "core/sync::push",',
        "append_authority_result(",
        "MAX_PUBLISH_OBJECT_BYTES", "MAX_PUBLISH_TOTAL_BYTES", "MAX_PUBLISH_OBJECTS",
    ), "publish mechanism route")
    if route.index("requires the artifact-loaded GenesisCode publish authority") > route.index("load_object(store"):
        fail("publish authority is checked after artifact access")
    if route.index("authority.finalize_publish(") > route.index("call_capability_with_runtime("):
        fail("remote mutation precedes final authority acceptance")
    retired = (
        "gc_vcs::Policy", "gc_vcs::Commit", "gc_vcs::Evidence", "gc_vcs::Attestation",
        "verify_commit_attestation", "commit_provenance_term", "class_for_ref",
    )
    if any(marker in dispatch or marker in route for marker in retired):
        fail("production publish route retains a native semantic authority")
    require_markers(payload, (
        "payload_pkg_publish_depth(payload: &Term) -> Result<u64, String>",
        '":depth must be a nonnegative u64"',
    ), "publish payload boundary")
    require_markers(tests, (
        "pkg_publish_requires_selfhost_authority_before_local_or_remote_io",
        "pkg_publish_validates_policy_and_pushes_commit_closure",
        "pkg_publish_enforces_obligation_bound_evidence_kinds",
        "authority failure must precede artifact access", "reg.upload_counts(), (0, 0, 0)",
    ), "publish integration controls")
    require_markers(adapter_tests, (
        "adapter_rejects_open_wrong_hash_and_undeclared_phase_results",
        "adapter_rejects_contradictory_finalize_provenance_and_sync",
        "adapter_reports_invalid_crypto_as_false_but_rejects_protocol_poisoning",
        "mechanical_signing_hash_matches_native_assurance_oracle",
    ), "publish adapter poison controls")
    for name, body in (("adapter", adapter), ("crypto", crypto), ("route", route)):
        if len(body.splitlines()) > 700:
            fail(f"publish {name} exceeds 700 lines")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    required_production = set(SOURCE_MODULES) | {
        "crates/gc_effects/src/pkg_publish_authority.rs",
        "crates/gc_effects/src/pkg_publish_authority_crypto.rs",
        route_path,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs",
    }
    for path in required_production:
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"ledger missing production path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("ledger missing publish specification")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("ledger missing publish verifier")
    limitations = "\n".join(row.get("limitations", [])).lower()
    if "publish" not in limitations or "h0" not in limitations or "registry" not in limitations:
        fail("ledger does not disclose publish authority and residual boundary")
    if source_identity(SOURCE_MODULES, sources) != profile["sourceSha256"]:
        fail("publish source closure identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_publish_authority.rs",
        "crates/gc_effects/src/pkg_publish_authority_crypto.rs",
        "crates/gc_effects/src/pkg_lock_read_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/publish_authority.rs",
        "crates/gc_effects/src/runner_pkg_payload.rs",
        "crates/gc_effects/tests/sync_registry/cases_a.rs",
        "crates/gc_effects/src/pkg_publish_authority_adapter_tests.rs",
        "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json",
    ]
    sources = {path: text(root, path, {}) for path in paths}
    mutations = []

    def mutate_profile(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-publish"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceModules", profile["sourceModules"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        mutate_profile(name, value)
    opened = copy.deepcopy(profile)
    opened["extra"] = True
    mutations.append((opened, {}, "profile-closure"))

    def mutate_source(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent: {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    mutate_source(SOURCE_MODULES[-1], "(def core/pkg::publish-authority", "(def core/pkg::legacy-publish", "binding")
    mutate_source("selfhost/pkg_publish_policy_v1.gc", "selfhost/pkg-publish-policy::select", "selfhost/pkg-publish-policy::legacy-select", "policy")
    mutate_source("selfhost/pkg_publish_authority_prepare_v1.gc", ":crypto-requests", ":legacy-requests", "prepare")
    mutate_source("selfhost/pkg_publish_authority_finalize_v1.gc", "publication-sync", "legacy-sync", "finalize")
    mutate_source("selfhost/toolchain_manifest.gc", SOURCE_MODULES[-1], "selfhost/missing.gc", "manifest")
    mutate_source(profile["artifact"], f':path "{SOURCE_MODULES[-1]}"', ':path "selfhost/missing.gc"', "artifact")
    mutate_source("crates/gc_effects/src/pkg_publish_authority.rs", "fn decode_phase_result", "fn decode_legacy_result", "decoder")
    mutate_source(
        "crates/gc_effects/src/pkg_publish_authority.rs",
        "if sync != expected_sync",
        "if sync == expected_sync",
        "sync-contradiction",
    )
    mutate_source("crates/gc_effects/src/pkg_publish_authority_crypto.rs", 'alg != "ed25519"', 'alg != "rsa"', "algorithm")
    mutate_source("crates/gc_effects/src/pkg_publish_authority_crypto.rs", "vcs\\0commit-sign\\0", "vcs\\0legacy-sign\\0", "sign-domain")
    mutate_source("crates/gc_effects/src/pkg_lock_read_authority.rs", '"core/pkg-low::publish"', '"core/pkg-low::legacy-publish"', "lazy-route")
    mutate_source("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs", "publish_authority::handle_publish(", "legacy_publish(", "dispatch")
    route_path = "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/publish_authority.rs"
    mutate_source(route_path, "authority.inspect_publish(&facts)?", "legacy_inspect(&facts)?", "inspect-route")
    mutate_source(route_path, "authority.finalize_publish(", "legacy_finalize(", "finalize-route")
    mutate_source(
        route_path,
        'call_capability_with_runtime(\n        "core/sync::push",',
        'call_capability_with_runtime(\n        "core/sync::legacy-push",',
        "sync-route",
    )
    mutate_source("crates/gc_effects/src/runner_pkg_payload.rs", "-> Result<u64, String>", "-> Option<u64>", "depth-boundary")
    mutate_source("crates/gc_effects/tests/sync_registry/cases_a.rs", "pkg_publish_requires_selfhost_authority_before_local_or_remote_io", "legacy_missing_authority", "integration")
    mutate_source("crates/gc_effects/src/pkg_publish_authority_adapter_tests.rs", "adapter_rejects_contradictory_finalize_provenance_and_sync", "legacy_adapter_test", "adapter-control")
    ledger_path = "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json"
    mutate_source(
        ledger_path,
        '"scripts/lib/selfhost_pkg_publish_authority.py"',
        '"scripts/lib/selfhost_pkg_publish_authority_legacy.py"',
        "ledger-custody",
    )

    controls = 0
    for changed, overrides, name in mutations:
        try:
            validate_all(root, changed, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 26:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-pkg-publish-authority: self-test ok (negative_controls={controls})")
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
            sources = {path: (root / path).read_text() for path in SOURCE_MODULES}
            profile["sourceSha256"] = source_identity(SOURCE_MODULES, sources)
            profile["contentIdentitySha256"] = canonical_identity(profile)
            profile_path.write_text(json.dumps(profile, indent=2) + "\n")
        validate_all(root, profile, schema)
        if args.artifact and args.artifact.resolve() != (root / profile["artifact"]).resolve():
            fail("artifact argument does not match profile")
        controls = self_test(root, profile, schema) if args.self_test else 0
        print(f"selfhost-pkg-publish-authority: ok profile={profile['contentIdentitySha256']} controls={controls}")
        return 0
    except (CheckError, OSError) as error:
        print(f"selfhost-pkg-publish-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
