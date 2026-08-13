#!/usr/bin/env python3
"""Independent custody and residual verifier for H2 SD-EVIDENCE-VERIFY."""

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
    "productionEntrypoints", "packageSourceModule", "packageSourceSha256", "requestKind",
    "resultKind", "runtimeEvidence", "schema", "sourceModule", "sourceSha256", "spec", "version",
}
DECISIONS = [
    "package-module-dependency-integrity-verdict",
    "acceptance-artifact-schema-and-reference-verdict",
    "registry-signature-policy-verdict",
    "transparency-chain-verdict",
    "genesisbench-dsse-verdict",
]
MECHANISMS = [
    "bounded-filesystem-read-and-content-addressed-store-access",
    "blake3-and-sha256-hashing",
    "ed25519-verification",
    "base64-toml-json-and-coreform-decoding",
    "bounded-observation-transport",
]
NONCLAIMS = {
    "bootstrap-fixpoint", "h3-h4-closure", "independent-verifier-replacement",
    "package-lock-and-resolution-authority",
    "registry-or-benchmark-publication-readiness", "release-qualification", "sh-c-closure",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/security::evidence-verify-authority",
    "decisionInventory": DECISIONS,
    "hostMechanisms": MECHANISMS,
    "hostOracle": {"removalTask": "R4.2.d", "required": False},
    "independentVerifier": "scripts/lib/selfhost_evidence_verify_authority.py",
    "kind": "genesis/selfhost-evidence-verify-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "packageSourceModule": "selfhost/evidence_verify_package_v1.gc",
    "requestKind": "genesis/evidence-verification-authority-request-v0.1",
    "resultKind": "genesis/evidence-verification-authority-result-v0.1",
    "runtimeEvidence": {
        "allocationLimit": 64_000_000,
        "maxPayloadBytes": 16_777_216,
        "maxVectorEntries": 16_384,
        "stepLimit": 20_000_000,
    },
    "schema": "docs/spec/SELFHOST_EVIDENCE_VERIFY_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/evidence_verify_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_EVIDENCE_VERIFY_AUTHORITY_v0.1.md",
    "version": "0.1.0",
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
    for name in ("contentIdentitySha256", "packageSourceSha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(name, ""))):
            fail(f"profile {name} invalid")
    if check_identity and profile["contentIdentitySha256"] != canonical_identity(profile):
        fail("profile content identity mismatch")


def text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    return (root / relative).read_text()


def require_all(haystack: str, needles, context: str) -> None:
    for needle in needles:
        if needle not in haystack:
            fail(f"{context} missing {needle!r}")


def static_check(root: Path, profile, overrides=None, artifact_path=None, check_artifact=True) -> None:
    overrides = overrides or {}
    relative = profile["sourceModule"]
    source_path = root / relative
    if source_path.is_symlink() or not source_path.is_file() or root.resolve() not in source_path.resolve().parents:
        fail("authority source is missing, escaping, or symlinked")
    source = text(root, relative, overrides)
    if source_identity(relative, source.encode()) != profile["sourceSha256"]:
        fail("authority source identity mismatch")
    package_relative = profile["packageSourceModule"]
    package_path = root / package_relative
    if package_path.is_symlink() or not package_path.is_file() or root.resolve() not in package_path.resolve().parents:
        fail("package authority source is missing, escaping, or symlinked")
    package_source = text(root, package_relative, overrides)
    if source_identity(package_relative, package_source.encode()) != profile["packageSourceSha256"]:
        fail("package authority source identity mismatch")
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    if (manifest.count(f'"{relative}"') != 1 or manifest.count(f'"{package_relative}"') != 1
            or manifest.index(f'"{package_relative}"') > manifest.index(f'"{relative}"')
            or manifest.count(profile["binding"]) != 1):
        fail("toolchain manifest custody drift")
    require_all(source, [
        "(def core/security::evidence-verify-authority", "(quote :package)",
        "(quote :transparency)", "(quote :dsse)", "selfhost/evidence-verify-package::evaluate",
        "vector-equal?", ":observed-h", "transparency/cycle", "transparency/truncated-chain",
        "transparency/link-mismatch", "bench/evidence-verification", profile["requestKind"],
        profile["resultKind"], ":request-h (selfhost/hash::hash-term request)",
    ], "GenesisCode authority")
    require_all(package_source, [
        "acceptance-valid?", "acceptance-store-has?", "obligations-loop",
        "store-valid?", "policy-shape-valid?", "signature-set-loop",
        "signature-valid?", "key-admitted-loop?", "policy/signature-threshold",
        "policy/signature-set-closure", "evidence/acceptance-schema-or-reference",
        "((selfhost/evidence-verify-package::field key) (quote :key-valid))\n              false)",
        "acceptance-store-has? store acceptance-h 0",
        "(((selfhost/evidence-verify-package::key-admitted-loop? keys)\n                                    ((selfhost/evidence-verify-package::field term) (quote :pk))) 0)",
    ], "GenesisCode package authority")
    if any(token in source + package_source for token in ("core/effect::", "core/host::")):
        fail("authority source contains an ambient host/effect operation")

    bridge = text(root, "crates/gc_obligations/src/evidence_verify_authority.rs", overrides)
    require_all(bridge, [
        f'const BINDING: &str = "{profile["binding"]}"',
        "SelfhostBootstrapMode::ArtifactOnly", "max_alloc_units: Some(ALLOC_LIMIT)",
        "max_vec_len: Some(16_384)", "context.step_limit = Some(STEP_LIMIT)",
        "decode_result(term, request_hash)", "authority verdict/error inconsistency",
        "field set mismatch", "pub fn package", "pub fn transparency", "pub fn dsse",
    ], "Rust authority bridge")
    if "unwrap_or(true)" in bridge or "unwrap_or_default()" in bridge:
        fail("authority bridge contains success-capable defaulting")

    verify = text(root, "crates/gc_obligations/src/verify.rs", overrides)
    transparency = text(root, "crates/gc_obligations/src/transparency.rs", overrides)
    security = text(root, "crates/gc_cli_driver/src/cmd_security_ops.rs", overrides)
    bench = text(root, "crates/gc_cli_driver/src/cmd_bench.rs", overrides)
    require_all(verify, [
        "verify_package_with_policy_and_authority", "EvidenceVerifyAuthority::load(authority_artifact)",
        ".package(PackageVerificationRequest", "read_last_acceptance(&pkg_dir)", "ErrorKind::NotFound",
        "observe_dep_hashes", "observe_store_payload", "verify_acceptance_signature_mechanism",
        "RegistryPolicy::observe", "proposed_signature_artifacts", "proposed_referenced_artifacts",
        "\":signature\",",
        "malformed acceptance pointer",
    ], "package route")
    if verify.count("observe_store_payload(") != 4:
        fail("package route does not bind acceptance and signature terms to single-read observations")
    for residual in (
        "check_dep_hashes", "verify_acceptance_kind", "AcceptanceSignature::from_term",
        "rec.verify(&allowed)", "mechanism_fact", "store.verify_hex",
    ):
        if residual in verify:
            fail(f"package route retains host semantic residual {residual!r}")
    require_all(transparency, [
        "MAX_TRANSPARENCY_ENTRIES: usize = 16_384", "read_head_observation",
        "ErrorKind::NotFound", "malformed transparency head", "BTreeSet", ".observe_bytes(",
        "observed_hash", ".transparency(",
    ], "transparency route")
    verify_transparency = transparency[
        transparency.index("pub fn verify_transparency_log"):transparency.index("fn read_head_observation")
    ]
    if ".ok()" in verify_transparency:
        fail("transparency head still collapses errors into absence")
    require_all(security, [
        'require_explicit_selfhost_artifact(cli, "transparency verification authority")',
        "verify_transparency_log(&store, &pkg_dir, &artifact)",
        'require_explicit_selfhost_artifact(cli, "package evidence verification authority")',
        "verify_package_with_policy_and_authority(",
    ], "CLI security routes")
    require_all(bench, [
        'require_explicit_selfhost_artifact(cli, "GenesisBench evidence verification authority")',
        "EvidenceVerifyAuthority::load(&artifact)", ".dsse(gc_obligations::DsseVerificationFacts",
        "if !decision.verified", "signature_valid", "envelope_fields.sort()",
        "signature_fields.sort()",
    ], "GenesisBench route")

    verifier_manifest = text(root, "tools/genesis-evidence-verifier/Cargo.toml", overrides)
    if re.search(r"gc_(?:coreform|kernel|obligations|cli|prelude)", verifier_manifest):
        fail("standalone evidence verifier depends on production Genesis crates")
    verifier_guard = text(root, "scripts/check_genesis_evidence_verifier.sh", overrides)
    require_all(verifier_guard, ["standalone=true", "read_only=true"], "standalone verifier guard")

    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-EVIDENCE-VERIFY"]
    if len(rows) != 1:
        fail("SD-EVIDENCE-VERIFY ledger row missing or duplicated")
    row = rows[0]
    if (row.get("currentLevel") != "H2" or row.get("fallbackReachability") != "none-proven"
            or relative not in row.get("producingImplementationPaths", [])
            or package_relative not in row.get("producingImplementationPaths", [])
            or "crates/gc_obligations/src/verify.rs" in row.get("producingImplementationPaths", [])
            or "crates/gc_obligations/src/verify.rs" in row.get("productionAuthorityPaths", [])
            or profile["spec"] not in row.get("specAuthorityPaths", [])
            or profile["independentVerifier"] not in row.get("verifierPaths", [])):
        fail("SD-EVIDENCE-VERIFY H2 custody drift")

    if check_artifact:
        artifact = artifact_path or (root / profile["artifact"])
        data = artifact.read_bytes()
        if (relative.encode() not in data or package_relative.encode() not in data
                or profile["binding"].encode() not in data):
            fail("authority source or binding absent from admitted artifact")


def mutation_controls(root: Path, profile) -> int:
    source = (root / profile["sourceModule"]).read_text()
    package_source = (root / profile["packageSourceModule"]).read_text()
    bridge_path = "crates/gc_obligations/src/evidence_verify_authority.rs"
    bridge = (root / bridge_path).read_text()
    verify_path = "crates/gc_obligations/src/verify.rs"
    verify = (root / verify_path).read_text()
    transparency_path = "crates/gc_obligations/src/transparency.rs"
    transparency = (root / transparency_path).read_text()
    security_path = "crates/gc_cli_driver/src/cmd_security_ops.rs"
    security = (root / security_path).read_text()
    bench_path = "crates/gc_cli_driver/src/cmd_bench.rs"
    bench = (root / bench_path).read_text()
    manifest_path = "selfhost/toolchain_manifest.gc"
    manifest = (root / manifest_path).read_text()
    mutations = [
        ({profile["sourceModule"]: source.replace("(quote :package)", "(quote :removed)", 1)}, "package phase"),
        ({profile["packageSourceModule"]: package_source.replace("acceptance-store-has? store acceptance-h 0", "removed-store-has? store acceptance-h 0", 1)}, "acceptance store binding"),
        ({profile["packageSourceModule"]: package_source.replace("(((selfhost/evidence-verify-package::key-admitted-loop? keys)\n                                    ((selfhost/evidence-verify-package::field term) (quote :pk))) 0)", "false", 1)}, "key admission"),
        ({profile["packageSourceModule"]: package_source.replace("policy/signature-threshold", "policy/removed-threshold", 1)}, "signature threshold"),
        ({profile["packageSourceModule"]: package_source.replace("((selfhost/evidence-verify-package::field key) (quote :key-valid))\n              false)", "true\n              false)", 1)}, "key mechanism admission"),
        ({profile["packageSourceModule"]: package_source.replace("policy/signature-set-closure", "policy/removed-set-closure", 1)}, "signature-set closure"),
        ({profile["sourceModule"]: source.replace("transparency/cycle", "transparency/removed", 1)}, "cycle"),
        ({profile["sourceModule"]: source.replace(":request-h (selfhost/hash::hash-term request)", ":request-h nil", 1)}, "request binding"),
        ({manifest_path: manifest.replace(f'    "{profile["sourceModule"]}"\n', "", 1)}, "module custody"),
        ({manifest_path: manifest.replace(f"    {profile['binding']}\n", "", 1)}, "binding custody"),
        ({bridge_path: bridge.replace("SelfhostBootstrapMode::ArtifactOnly", "SelfhostBootstrapMode::Embedded", 1)}, "artifact only"),
        ({bridge_path: bridge.replace("max_vec_len: Some(16_384)", "max_vec_len: None", 1)}, "vector bound"),
        ({bridge_path: bridge.replace("decode_result(term, request_hash)", "Ok(term)", 1)}, "result decoder"),
        ({verify_path: verify.replace(".package(PackageVerificationRequest", ".package(removed", 1)}, "package request"),
        ({verify_path: verify.replace("\":signature\",", "\":removed\",", 1)}, "signature store observation"),
        ({verify_path: verify.replace("observe_store_payload(&store, \":acceptance\"", "observe_store(&store, \":acceptance\"", 1)}, "acceptance single-read binding"),
        ({verify_path: verify.replace("ErrorKind::NotFound", "ErrorKind::Other", 1)}, "pointer distinction"),
        ({transparency_path: transparency.replace("MAX_TRANSPARENCY_ENTRIES: usize = 16_384", "MAX_TRANSPARENCY_ENTRIES: usize = usize::MAX", 1)}, "chain bound"),
        ({transparency_path: transparency.replace("malformed transparency head", "empty transparency head", 1)}, "malformed head"),
        ({transparency_path: transparency.replace("store.observe_bytes(hex)", "store.observe_hex(hex)", 1)}, "transparency single-read binding"),
        ({security_path: security.replace("verify_package_with_policy_and_authority(", "verify_package_with_policy(", 1)}, "package route"),
        ({security_path: security.replace("verify_transparency_log(&store, &pkg_dir, &artifact)", "verify_transparency_log(&store, &pkg_dir)", 1)}, "transparency route"),
        ({bench_path: bench.replace("if !decision.verified", "if false", 1)}, "DSSE custody"),
        ({bench_path: bench.replace("envelope_fields.sort()", "envelope_fields.clear()", 1)}, "DSSE envelope inventory"),
    ]
    passed = 0
    for overrides, name in mutations:
        mutated = copy.deepcopy(profile)
        if profile["sourceModule"] in overrides:
            mutated["sourceSha256"] = source_identity(profile["sourceModule"], overrides[profile["sourceModule"]].encode())
        if profile["packageSourceModule"] in overrides:
            mutated["packageSourceSha256"] = source_identity(
                profile["packageSourceModule"], overrides[profile["packageSourceModule"]].encode())
        try:
            static_check(root, mutated, overrides, check_artifact=False)
        except CheckError:
            passed += 1
        else:
            fail(f"mutation control survived: {name}")
    return passed


def update(root: Path, profile_path: Path, schema_path: Path) -> None:
    profile = load_json(profile_path)
    schema = load_json(schema_path)
    validate_profile(profile, schema, check_identity=False)
    relative = profile["sourceModule"]
    profile["sourceSha256"] = source_identity(relative, (root / relative).read_bytes())
    package_relative = profile["packageSourceModule"]
    profile["packageSourceSha256"] = source_identity(
        package_relative, (root / package_relative).read_bytes())
    profile["contentIdentitySha256"] = canonical_identity(profile)
    profile_path.write_text(json.dumps(profile, indent=2) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--update", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    profile_path = args.profile if args.profile.is_absolute() else root / args.profile
    schema_path = args.schema if args.schema.is_absolute() else root / args.schema
    try:
        if args.update:
            update(root, profile_path, schema_path)
            print("selfhost-evidence-verify-authority: profile identities updated")
            return 0
        profile = load_json(profile_path)
        schema = load_json(schema_path)
        validate_profile(profile, schema)
        static_check(root, profile, artifact_path=args.artifact)
        controls = mutation_controls(root, profile)
        print(f"selfhost-evidence-verify-authority: pass ({controls} mutation controls)")
        return 0
    except (CheckError, OSError) as error:
        print(f"selfhost-evidence-verify-authority: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
