#!/usr/bin/env python3
"""Independent custody verifier for self-hosted package bridge objects."""

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


def load_json(path: Path):
    try:
        return parse_json(path.read_text(), str(path))
    except OSError as error:
        fail(f"cannot read {path}: {error}")


FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionOperations", "requestKind", "resultKind", "schema", "sourceModule",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::bridge-authority",
    "decisionInventory": [
        "external-provenance-object-and-content-identity",
        "conversion-data-and-evidence-object-identities",
        "bridge-patch-and-package-snapshot-identities",
        "unsigned-commit-and-domain-separated-signing-message",
        "cryptographic-mechanism-fact-admission",
        "attestation-and-final-commit-identities",
        "request-and-plan-bound-two-phase-result",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "capability-policy-payload-and-store-admission",
        "ed25519-sign-mechanism-and-supplied-key-verification",
        "exact-authorized-byte-content-addressed-storage",
        "strict-result-and-vcs-contradiction-checking",
        "separately-custodied-conditional-lock-persistence",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_bridge_authority.py",
    "kind": "genesis/selfhost-pkg-bridge-authority-v0.1",
    "productionOperations": ["core/pkg-low::bridge"],
    "requestKind": "genesis/pkg-bridge-authority-request-v0.1",
    "resultKind": "genesis/pkg-bridge-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_BRIDGE_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_bridge_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_BRIDGE_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "graph-and-semver-mechanism-authority",
    "h2-package-resolution", "payload-transport-and-cryptographic-mechanism-authority",
    "publish-and-registry-authority", "r4-2-e-closure", "release-qualification",
    "selfhost-toml-codec", "sh-c-closure", "workspace-authority",
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


def require_markers(text: str, markers, label: str) -> None:
    for marker in markers:
        if marker not in text:
            fail(f"{label} missing marker: {marker}")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = source_text(root, profile["sourceModule"], overrides)
    manifest = source_text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = source_text(root, profile["artifact"], overrides)
    adapter = source_text(root, "crates/gc_effects/src/pkg_bridge_authority.rs", overrides)
    reader = source_text(root, "crates/gc_effects/src/pkg_lock_read_authority.rs", overrides)
    publish = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs", overrides)
    bridge = source_text(
        root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs", overrides
    )
    integration = source_text(root, "crates/gc_effects/tests/sync_registry/cases_a.rs", overrides)
    ledger_text = source_text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides)
    ledger = parse_json(ledger_text, "semantic ownership ledger")

    require_markers(module, (
        "(def core/pkg::bridge-authority", profile["requestKind"], profile["resultKind"],
        "(def selfhost/pkg-bridge::object", "selfhost/printer::print-term term",
        "core/crypto::blake3 bytes", ":type (quote :gcpm/external-provenance)",
        ":type (quote :gcpm/bridge-conversion)", ":type (quote :vcs/evidence)",
        ":type (quote :vcs/patch)", ":type (quote :vcs/snapshot)",
        ":type (quote :vcs/attestation)", ":type (quote :vcs/commit)",
        'b"vcs\\x00commit-signing-hash\\x00"', 'b"vcs\\x00commit-sign\\x00"',
        "selfhost/hash::hash-term plan", ":signature-valid", ":plan", ":finalize",
        "bridge signature failed cryptographic verification",
    ), "GenesisCode bridge authority")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody bridge module and binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain bridge module and binding")

    require_markers(adapter, (
        "pub(crate) fn plan_bridge", "pub(crate) fn finalize_bridge", "fn decode_bridge_envelope",
        "fn decode_object", "bridge plan value and :plan-h are malformed or contradictory",
        "bridge plan :signing-h contradicts the unsigned commit",
        "bridge plan :sign-message contradicts the VCS attestation domain",
        "bridge object :term, :bytes, and :h are malformed or contradictory",
        "verify_commit_attestation", "bridge final commit contradicts its plan or attestation",
        "authority_owns_exact_bridge_objects_and_valid_attestation",
        "authority_rejects_false_crypto_fact_and_result_substitution",
        "object_decoder_rejects_bytes_and_hash_substitution",
    ), "Rust bridge authority adapter")
    require_markers(reader, (
        '#[path = "pkg_bridge_authority.rs"]', "bridge_authority: Option<Value>",
        "let bridge_authority = environment.get(bridge::BRIDGE_BINDING);",
        'matches!(op, "core/pkg-low::bridge" | "core/pkg-low::snapshot")',
    ), "lazy bridge authority loader")
    require_markers(publish, (
        '#[path = "dispatch_publish/bridge_objects.rs"]',
        '"core/pkg-low::bridge" => bridge_objects::dispatch_bridge(',
    ), "package bridge route")
    require_markers(bridge, (
        "requires the artifact-loaded GenesisCode bridge authority",
        "let plan = match authority.plan_bridge(facts)?", "let provenance_root = put!(&plan.provenance)",
        "Term::Bytes(plan.sign_message.clone().into())", "key.verify_strict(",
        "authority.finalize_bridge(facts, &plan, public_key, signature, signature_valid)?",
        "let attestation_h = put!(&finalized.attestation)", "let commit_h = put!(&finalized.commit)",
        "bridge store identity contradiction", "bridge_lock::update_lock(",
    ), "bridge mechanism adapter")
    if bridge.index("requires the artifact-loaded GenesisCode bridge authority") > bridge.index("let provenance_root = put!"):
        fail("bridge authority availability is checked after storage side effects")
    if bridge.index("authority.finalize_bridge") > bridge.index("let attestation_h = put!"):
        fail("bridge final objects are stored before authority finalization")
    for marker in (
        ":gcpm/external-provenance", ":gcpm/bridge-conversion", ":vcs/evidence",
        ":vcs/patch", ":vcs/snapshot", ":vcs/attestation",
    ):
        if marker in bridge or marker in publish:
            fail(f"production bridge mechanism retains object constructor: {marker}")
    if "Term::Bytes(plan.signing_hash" in bridge:
        fail("bridge sign route passes bare signing hash instead of domain-separated message")
    if len(adapter.splitlines()) > 700 or len(bridge.splitlines()) > 700 or len(publish.splitlines()) > 700:
        fail("bridge authority decomposition exceeds the 700-line production ceiling")

    require_markers(integration, (
        "pkg_bridge_creates_signed_commit_and_updates_lock",
        "pkg_bridge_missing_lock_authority_fails_before_store_side_effects",
        "pkg_bridge_without_lock_still_requires_authority_before_store_side_effects",
        "verify_commit_attestation", "assert_eq!(std::fs::read_dir(&store_dir).unwrap().count(), 0)",
    ), "bridge integration controls")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    for path in (
        profile["sourceModule"], "crates/gc_effects/src/pkg_bridge_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs",
    ):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing production authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing bridge authority specification")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("semantic ledger missing bridge authority verifier")
    if "crates/gc_effects/tests/sync_registry.rs" not in row.get("testPaths", []):
        fail("semantic ledger missing bridge integration controls")
    limitations = "\n".join(row.get("limitations", [])).lower()
    if "bridge object" not in limitations or "h0" not in limitations or "ed25519" not in limitations:
        fail("semantic ledger does not disclose bridge authority and retained mechanism boundary")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("bridge authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_bridge_authority.rs",
        "crates/gc_effects/src/pkg_lock_read_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs",
        "crates/gc_effects/tests/sync_registry/cases_a.rs",
        "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json",
    ]
    sources = {path: source_text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-bridge-authority"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("productionOperations", []),
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
    source_mutation(profile["sourceModule"], "[:facts :kind :mechanism :op :v]", "[:facts :kind :op :v]", "request-closure")
    source_mutation(profile["sourceModule"], 'b"vcs\\x00commit-sign\\x00"', 'b"vcs\\x00legacy-sign\\x00"', "sign-domain")
    source_mutation(profile["sourceModule"], "core/crypto::blake3 bytes", "core/crypto::sha256 bytes", "object-hash")
    source_mutation(profile["sourceModule"], ":signature-valid", ":signature-trusted", "mechanism-fact")
    source_mutation("selfhost/toolchain_manifest.gc", profile["sourceModule"], "selfhost/missing.gc", "manifest")
    source_mutation(profile["artifact"], f':path "{profile["sourceModule"]}"', ':path "selfhost/missing.gc"', "artifact")
    source_mutation("crates/gc_effects/src/pkg_bridge_authority.rs", "fn decode_bridge_envelope", "fn decode_legacy_envelope", "result-decoder")
    source_mutation("crates/gc_effects/src/pkg_bridge_authority.rs", "bridge plan :sign-message contradicts the VCS attestation domain", "sign message accepted", "sign-contradiction")
    source_mutation("crates/gc_effects/src/pkg_bridge_authority.rs", "bridge object :term, :bytes, and :h are malformed or contradictory", "object accepted", "object-contradiction")
    source_mutation("crates/gc_effects/src/pkg_lock_read_authority.rs", 'matches!(op, "core/pkg-low::bridge" | "core/pkg-low::snapshot")', 'matches!(op, "core/pkg-low::snapshot")', "lazy-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish.rs", '"core/pkg-low::bridge" => bridge_objects::dispatch_bridge(', '"core/pkg-low::bridge" => legacy_bridge(', "dispatch")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs", "Term::Bytes(plan.sign_message.clone().into())", "Term::Bytes(plan.signing_hash.to_vec().into())", "sign-message-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs", "key.verify_strict(", "key.verify_legacy(", "crypto-check")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs", "authority.finalize_bridge(facts, &plan, public_key, signature, signature_valid)?", "legacy_finalize(facts)?", "finalize-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/bridge_objects.rs", "bridge store identity contradiction", "store accepted contradiction", "store-identity")
    source_mutation("crates/gc_effects/tests/sync_registry/cases_a.rs", "pkg_bridge_without_lock_still_requires_authority_before_store_side_effects", "legacy_missing_authority_test", "missing-authority-control")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides, check_identity=True)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 23:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-pkg-bridge-authority: self-test ok (negative_controls={controls})")
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
            "selfhost-pkg-bridge-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-bridge-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
