#!/usr/bin/env python3
"""Independent verifier for H2 GenesisCode signing authority."""

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
    value = {}
    for key, item in pairs:
        if key in value:
            fail(f"duplicate JSON key: {key}")
        value[key] = item
    return value


def load_json(path: Path):
    try:
        value = json.loads(path.read_text(), object_pairs_hook=unique_object)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"JSON root is not an object: {path}")
    return value


FIELDS = {
    "artifact",
    "auditDate",
    "binding",
    "contentIdentitySha256",
    "decisionInventory",
    "hostMechanisms",
    "hostOracle",
    "independentVerifier",
    "kind",
    "nonclaims",
    "productionEntrypoints",
    "requestKind",
    "resultKind",
    "runtimeEvidence",
    "schema",
    "sourceModule",
    "sourceSha256",
    "spec",
    "version",
}

DECISIONS = [
    "keypair-admission",
    "acceptance-message-construction",
    "acceptance-signature-artifact-construction",
    "signature-set-canonicalization",
    "transparency-entry-construction",
    "dsse-pae-construction",
    "dsse-signature-artifact-construction",
]

HOST_MECHANISMS = [
    "os-csprng",
    "ed25519-key-derivation-sign-and-verify",
    "sha256-and-content-addressed-hashing",
    "base64-toml-coreform-json-codec",
    "bounded-owner-only-secret-file-transport",
    "content-addressed-storage-and-state-pointer-write",
]

NONCLAIMS = {
    "bootstrap-fixpoint",
    "evidence-verification-authority",
    "h3-h4-closure",
    "r4-2-d-closure",
    "registry-or-benchmark-publication-readiness",
    "release-qualification",
    "sh-c-closure",
}

CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/security::signing-authority",
    "decisionInventory": DECISIONS,
    "hostMechanisms": HOST_MECHANISMS,
    "hostOracle": {"removalTask": "R4.2.d", "required": False},
    "independentVerifier": "scripts/lib/selfhost_signing_authority.py",
    "kind": "genesis/selfhost-signing-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/signing-authority-request-v0.1",
    "resultKind": "genesis/signing-authority-result-v0.1",
    "runtimeEvidence": {
        "allocationLimit": 64_000_000,
        "maxPayloadBytes": 16_777_216,
        "stepLimit": 20_000_000,
    },
    "schema": "docs/spec/SELFHOST_SIGNING_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/signing_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_SIGNING_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}


def profile_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


def source_identity(relative: str, data: bytes) -> str:
    digest = hashlib.sha256()
    digest.update(relative.encode())
    digest.update(b"\0")
    digest.update(data)
    digest.update(b"\0")
    return digest.hexdigest()


def validate(profile, schema, check_identity=True) -> None:
    if (
        schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != FIELDS
        or set(schema.get("properties", {})) != FIELDS
    ):
        fail("schema closure drift")
    if set(profile) != FIELDS:
        fail("profile field drift")
    for name, expected in CONSTANTS.items():
        if profile.get(name) != expected:
            fail(f"profile {name} drift")
        if name not in {"hostOracle", "runtimeEvidence"}:
            schema_value = schema["properties"].get(name, {}).get("const")
            if schema_value is not None and schema_value != expected:
                fail(f"schema {name} drift")
    if set(profile.get("nonclaims", [])) != NONCLAIMS:
        fail("profile nonclaim inventory drift")
    for name in ("contentIdentitySha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(name, ""))):
            fail(f"profile {name} is invalid")
    if check_identity and profile["contentIdentitySha256"] != profile_identity(profile):
        fail("profile content identity mismatch")


def read_text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    return (root / relative).read_text()


def function_slice(source: str, start: str, end: str) -> str:
    begin = source.find(start)
    finish = source.find(end, begin + len(start))
    if begin < 0 or finish < 0:
        fail(f"cannot isolate production function between {start!r} and {end!r}")
    return source[begin:finish]


def static_check(
    root: Path,
    profile,
    overrides=None,
    artifact_path: Path | None = None,
    check_artifact=True,
) -> None:
    overrides = overrides or {}
    source_relative = profile["sourceModule"]
    source_path = root / source_relative
    if (
        source_path.is_symlink()
        or not source_path.is_file()
        or root.resolve() not in source_path.resolve().parents
    ):
        fail("signing authority source is missing, escaping, or symlinked")
    source = read_text(root, source_relative, overrides)
    if source_identity(source_relative, source.encode()) != profile["sourceSha256"]:
        fail("signing authority source identity mismatch")

    manifest = read_text(root, "selfhost/toolchain_manifest.gc", overrides)
    if (
        manifest.count(f'"{source_relative}"') != 1
        or manifest.count(profile["binding"]) != 1
    ):
        fail("signing authority manifest custody drift")

    compact = re.sub(r"\s+", " ", source)
    markers = [
        "selfhost/signing::keygen-valid?",
        "selfhost/signing::acceptance-plan-valid?",
        "selfhost/signing::acceptance-finalize-valid?",
        "selfhost/signing::commit-valid?",
        "selfhost/signing::dsse-plan-valid?",
        "selfhost/signing::dsse-finalize-valid?",
        "selfhost/signing::acceptance-message",
        "selfhost/signing::dsse-message",
        "selfhost/signing::sort-hashes",
        '"genesis/acceptance-signature-v0.2"',
        '"genesis/transparency-entry-v0.2"',
        '"genesis/genesisbench-dsse-signature-v0.1"',
        profile["requestKind"],
        profile["resultKind"],
    ]
    for marker in markers:
        if marker not in compact:
            fail(f"signing authority source marker missing: {marker}")
    if compact.count("selfhost/signing::exact-map?") < 7:
        fail("signing authority does not close every protocol map")

    bridge = read_text(root, "crates/gc_obligations/src/signing_authority.rs", overrides)
    decoder = read_text(
        root, "crates/gc_obligations/src/signing_authority_decode.rs", overrides
    )
    for marker in [
        'const BINDING: &str = "core/security::signing-authority"',
        "const STEP_LIMIT: u64 = 20_000_000",
        "const ALLOC_LIMIT: u64 = 64_000_000",
        "bootstrap_mode != SelfhostBootstrapMode::ArtifactOnly",
        "load_selfhost_coreform_toolchain_v1_with_mode",
        "context.reset_counters()",
        "pub fn acceptance_message(",
        "pub fn acceptance_artifact(",
        "pub fn commit(",
        "pub fn dsse_message(",
        "pub fn dsse_artifact(",
    ]:
        if marker not in bridge:
            fail(f"signing host boundary marker missing: {marker}")
    for marker in [
        '"authority result"',
        "field set mismatch",
        '":request-h"',
        "hash_hex(request_hash)",
        "accepted result must carry nil :code and :message",
        "rejected result must carry nil :data",
        "must be strictly sorted and unique",
        "validate_transparency_entry",
        "DSSE artifact byte facts mismatch",
        "result_decoder_rejects_open_results",
        "result_decoder_rejects_unbound_results",
        "authority_rejects_failed_cryptographic_mechanism_facts",
    ]:
        if marker not in decoder:
            fail(f"signing result decoder/control marker missing: {marker}")

    security = read_text(root, "crates/gc_cli_driver/src/cmd_security_signing.rs", overrides)
    keygen = function_slice(security, "pub(super) fn cmd_keygen(", "pub(super) fn cmd_sign(")
    sign = function_slice(
        security, "pub(super) fn cmd_sign(", "// SIGNING_AUTHORITY_ROUTES_END"
    )
    if "SigningAuthority::load" not in security:
        fail("shared signing authority loader is missing")
    for marker in [".keygen("]:
        if marker not in keygen:
            fail(f"keygen authority route missing: {marker}")
    for marker in [
        ".acceptance_message(",
        ".acceptance_artifact(",
        ".commit(",
        "put_term(&signature_term)",
        "put_term(&commit.transparency_entry)",
        "sign/signature-set",
        "sign/transparency-head",
    ]:
        if marker not in sign:
            fail(f"package signing authority route missing: {marker}")
    for forbidden in [
        "sign_acceptance_hash",
        "write_signature_set",
        "append_transparency_entry",
        "unwrap_or_default",
    ]:
        if forbidden in sign:
            fail(f"package signing retains host semantic fallback: {forbidden}")

    bench = read_text(root, "crates/gc_cli_driver/src/cmd_bench.rs", overrides)
    crypto_sign = function_slice(bench, "fn crypto_sign(", "fn crypto_verify(")
    for marker in [
        "SigningAuthority::load",
        ".dsse_message(",
        ".dsse_artifact(",
        "signature_valid",
    ]:
        if marker not in crypto_sign:
            fail(f"GenesisBench signing authority route missing: {marker}")
    for forbidden in ["key_id(&public)", "pae(payload_type", "serde_json::json!({\n            \"kind\": \"genesis/"]:
        if forbidden in crypto_sign:
            fail(f"GenesisBench signer retains host semantic fallback: {forbidden}")

    signing = read_text(root, "crates/gc_obligations/src/signing.rs", overrides)
    for marker in [
        "signing key must be a regular non-symlink file",
        "signing key permissions must deny group and other access",
        "signing and public key material do not match",
        "options.custom_flags(libc::O_NOFOLLOW)",
        ".create_new(true)",
        "file.sync_all()",
        '#[cfg(feature = "parity-oracle")]\n    pub fn to_term',
        '#[cfg(feature = "parity-oracle")]\npub fn sign_acceptance_hash',
        '#[cfg(feature = "parity-oracle")]\npub fn write_signature_set',
    ]:
        if marker not in signing:
            fail(f"secret custody/parity marker missing: {marker}")
    transparency = read_text(root, "crates/gc_obligations/src/transparency.rs", overrides)
    if '#[cfg(feature = "parity-oracle")]\npub fn append_transparency_entry' not in transparency:
        fail("legacy transparency constructor is not parity-only")

    tests = read_text(root, "crates/gc_cli/tests/cli_smoke.rs", overrides)
    for marker in [
        "sign_and_verify_with_policy_succeeds",
        "sign/signature-set",
        "sign/transparency-head",
    ]:
        if marker not in tests:
            fail(f"package signing regression control missing: {marker}")

    ledger = json.loads(
        read_text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides),
        object_pairs_hook=unique_object,
    )
    rows = [
        row
        for row in ledger.get("semanticDecisions", [])
        if row.get("id") == "SD-SIGNING"
    ]
    if len(rows) != 1:
        fail("SD-SIGNING ledger row missing or duplicated")
    row = rows[0]
    if row.get("currentLevel") != "H2" or row.get("fallbackReachability") != "none-proven":
        fail("SD-SIGNING ledger authority claim drift")
    for relative in [source_relative, profile["artifact"]]:
        if relative not in row.get("productionAuthorityPaths", []):
            fail(f"SD-SIGNING production authority omits {relative}")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("SD-SIGNING verifier custody drift")

    if check_artifact:
        selected_artifact = artifact_path or (root / profile["artifact"])
        artifact = selected_artifact.read_text()
        if artifact.count(source_relative) != 1:
            fail("signing authority artifact source custody drift")
        for marker in (profile["requestKind"], profile["resultKind"]):
            if marker not in artifact:
                fail(f"signing authority artifact marker missing: {marker}")


def self_test(root: Path, profile, schema, artifact_path: Path | None) -> None:
    controls = 0

    def reject_profile(mutator):
        nonlocal controls
        candidate = copy.deepcopy(profile)
        mutator(candidate)
        try:
            validate(candidate, schema)
        except CheckError:
            controls += 1
            return
        fail("profile mutation was accepted")

    reject_profile(lambda value: value.__setitem__("binding", "core/security::legacy"))
    reject_profile(lambda value: value["decisionInventory"].pop())
    reject_profile(lambda value: value["hostMechanisms"].append("semantic-oracle"))
    reject_profile(lambda value: value["hostOracle"].__setitem__("required", True))
    reject_profile(lambda value: value["nonclaims"].remove("evidence-verification-authority"))
    reject_profile(lambda value: value["runtimeEvidence"].__setitem__("stepLimit", 0))
    reject_profile(lambda value: value.__setitem__("kind", "wrong"))
    reject_profile(lambda value: value.__setitem__("contentIdentitySha256", "0" * 64))

    static_mutations = [
        ("selfhost/toolchain_manifest.gc", lambda text: text.replace(profile["binding"], "legacy", 1)),
        ("crates/gc_obligations/src/signing_authority.rs", lambda text: text.replace("context.reset_counters()", "", 1)),
        ("crates/gc_obligations/src/signing_authority_decode.rs", lambda text: text.replace("field set mismatch", "open result")),
        ("crates/gc_cli_driver/src/cmd_security_signing.rs", lambda text: text.replace(".acceptance_artifact(", ".sign_acceptance_hash(", 1)),
        ("crates/gc_cli_driver/src/cmd_bench.rs", lambda text: text.replace(".dsse_message(", "pae(", 1)),
        ("crates/gc_obligations/src/signing.rs", lambda text: text.replace(".create_new(true)", ".create(true)", 1)),
        ("crates/gc_obligations/src/transparency.rs", lambda text: text.replace('#[cfg(feature = "parity-oracle")]\npub fn append_transparency_entry', "pub fn append_transparency_entry", 1)),
        ("crates/gc_cli/tests/cli_smoke.rs", lambda text: text.replace('"sign/transparency-head"', '"ignored"', 1)),
    ]
    for relative, mutate in static_mutations:
        overrides = {relative: mutate((root / relative).read_text())}
        try:
            static_check(
                root,
                profile,
                overrides,
                artifact_path=artifact_path,
                check_artifact=False,
            )
        except CheckError:
            controls += 1
        else:
            fail(f"static mutation was accepted: {relative}")

    if controls != 16:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-signing-authority-self-test: ok (negative_controls={controls})")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--refresh-identity", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        profile_path = root / args.profile
        profile = load_json(profile_path)
        schema = load_json(root / args.schema)
        if args.refresh_identity:
            source = (root / profile["sourceModule"]).read_bytes()
            profile["sourceSha256"] = source_identity(profile["sourceModule"], source)
            profile["contentIdentitySha256"] = profile_identity(profile)
            profile_path.write_text(json.dumps(profile, indent=2) + "\n")
        validate(profile, schema)
        artifact = args.artifact.resolve() if args.artifact else None
        static_check(root, profile, artifact_path=artifact)
        if args.self_test:
            self_test(root, profile, schema, artifact)
        print(
            "selfhost-signing-authority: ok "
            f"(decisions={len(DECISIONS)} host_oracle=none level=H2)"
        )
        return 0
    except (CheckError, OSError, json.JSONDecodeError) as error:
        print(f"selfhost-signing-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
