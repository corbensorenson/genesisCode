#!/usr/bin/env python3
"""Independent custody verifier for the partial self-hosted store authority."""

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
    "productionEntrypoints", "requestKind", "resultKind", "runtimeEvidence", "schema",
    "sourceModule", "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/store::authority",
    "decisionInventory": [
        "put-payload-admission", "put-canonical-bytes", "put-operation-budget-admission",
        "put-cumulative-budget-admission", "put-content-hash-identity",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "authorized-policy-limit-transport",
        "blake3-and-byte-count-contradiction-checking",
        "atomic-write-once-filesystem-storage-and-durability",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_store_authority.py",
    "kind": "genesis/selfhost-store-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/store-authority-request-v0.1",
    "resultKind": "genesis/store-authority-result-v0.1",
    "runtimeEvidence": {
        "allocationLimit": 160_000_000, "maxMapEntries": 32,
        "maxPayloadBytes": 41_943_040, "maxVectorEntries": 16_384,
        "stepLimit": 20_000_000,
    },
    "schema": "docs/spec/SELFHOST_STORE_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/store_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_STORE_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "get-has-verify-authority", "h2-sd-store",
    "package-registry-vcs-authority", "r4-2-e-closure", "release-qualification",
    "sh-c-closure",
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
    source_relative = profile["sourceModule"]
    source_path = root / source_relative
    if source_path.is_symlink() or not source_path.is_file() or root.resolve() not in source_path.resolve().parents:
        fail("authority source is missing, escaping, or symlinked")
    source = text(root, source_relative, overrides)
    if source_identity(source_relative, source.encode()) != profile["sourceSha256"]:
        fail("authority source identity mismatch")
    require_all(source, [
        "(def core/store::authority", "(quote :put)", profile["requestKind"],
        profile["resultKind"], ":request-h (selfhost/hash::hash-term request)",
        "selfhost/printer::print-term", "core/str::to-utf8", "core/crypto::blake3",
        "core/bytes::to-hex", "store put exceeds max_bytes",
        "store put exceeds max_run_bytes", "store put payload must contain exactly :artifact",
    ], "GenesisCode store authority")
    if "core/effect::" in source or "core/host::" in source:
        fail("store authority contains an ambient effect or host operation")

    manifest_path = "selfhost/toolchain_manifest.gc"
    manifest = text(root, manifest_path, overrides)
    if manifest.count(f'"{source_relative}"') != 1 or manifest.count(profile["binding"]) != 1:
        fail("toolchain manifest custody drift")

    bridge_path = "crates/gc_effects/src/store_authority.rs"
    bridge = text(root, bridge_path, overrides)
    require_all(bridge, [
        f'const BINDING: &str = "{profile["binding"]}"',
        "load_selfhost_coreform_toolchain_v1_with_mode", "max_alloc_units: Some(ALLOC_LIMIT)",
        "max_bytes_len: Some(PAYLOAD_LIMIT)", "max_map_len: Some(32)",
        "max_vec_len: Some(16_384)", "decode_put_result(term, request_hash)",
        "result field set mismatch", "write byte count contradiction",
        "write hash/bytes contradiction", "context.reset_counters()",
    ], "Rust store authority bridge")
    if "unwrap_or_default()" in bridge or "unwrap_or(true)" in bridge:
        fail("store authority bridge contains success-capable defaulting")

    policy_path = "crates/gc_effects/src/policy.rs"
    policy = text(root, policy_path, overrides)
    require_all(policy, [
        "selfhost_authority: Option<SelfhostAuthorityConfig>", 'mod policy_selfhost;',
    ], "policy authority custody")
    policy_selfhost_path = "crates/gc_effects/src/policy_selfhost.rs"
    policy_selfhost = text(root, policy_selfhost_path, overrides)
    require_all(policy_selfhost, [
        "policy.selfhost_authority = Some(SelfhostAuthorityConfig",
        "artifact: artifact.map(Path::to_path_buf)", "selfhost_authority_config(&self)",
    ], "policy authority propagation")

    runner_path = "crates/gc_effects/src/runner.rs"
    runner = text(root, runner_path, overrides)
    require_all(runner, [
        'policy.is_allowed("core/store::put")', ".map(StoreAuthority::load)",
        "store_authority.as_mut()",
    ], "runner authority custody")

    cap_path = "crates/gc_effects/src/runner_cap_store.rs"
    cap = text(root, cap_path, overrides)
    require_all(cap, [
        "let Some(authority) = authority else", '#[cfg(any(test, feature = "parity-oracle"))]',
        "requires the artifact-loaded GenesisCode store authority", "authority.put(",
        "StorePutDecision::Write", ".put_bytes(&bytes)",
        "store write mechanism contradicted GenesisCode-authorized hash",
        "fn cap_store_put_parity(",
    ], "store put production route")
    parity_boundary = '#[cfg(any(test, feature = "parity-oracle"))]\nfn cap_store_put_parity'
    boundary_at = cap.find(parity_boundary)
    if boundary_at < 0:
        fail("store parity oracle is not compile-time isolated")
    production = cap[:boundary_at]
    for residual in ("payload_store_artifact(payload)", "store_put_with_budget(", "print_term(&art)"):
        if residual in production:
            fail(f"production store put retains host semantic residual {residual!r}")
    if production.index("authority.put(") > production.index(".put_bytes(&bytes)"):
        fail("store write occurs before authority decision")

    dispatch_path = "crates/gc_effects/src/runner_capability_dispatch.rs"
    dispatch = text(root, dispatch_path, overrides)
    require_all(dispatch, [
        "store_authority: Option<&mut StoreAuthority>", '"core/store::put" => cap_store_put(',
        "store_authority,", "None,\n        &mut bridge_runtime",
    ], "store dispatch")

    cli_path = "crates/gc_cli_driver/src/lib.rs"
    cli = text(root, cli_path, overrides)
    require_all(cli, [
        "CapsPolicy::load_with_selfhost_authority(", "config.bootstrap_mode",
        "config.artifact.as_deref()", "Rust effect-policy authority is not compiled into production",
    ], "production CLI policy route")

    tests_path = "crates/gc_effects/tests/store_caps.rs"
    tests = text(root, tests_path, overrides)
    require_all(tests, [
        "store_put_without_artifact_authority_fails_closed",
        "store_put_payload_shape_is_decided_as_a_sealed_error",
        "semantic rejection must happen before any store write",
        "store_put_enforces_cumulative_store_run_budget",
    ], "store authority tests")

    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-STORE"]
    if len(rows) != 1:
        fail("SD-STORE ledger row missing or duplicated")
    row = rows[0]
    limitations = " ".join(row.get("limitations", []))
    if (row.get("currentLevel") != "H0" or source_relative not in row.get("producingImplementationPaths", [])
            or bridge_path not in row.get("productionAuthorityPaths", [])
            or profile["spec"] not in row.get("specAuthorityPaths", [])
            or profile["independentVerifier"] not in row.get("verifierPaths", [])
            or "remain host-authoritative" not in limitations):
        fail("SD-STORE partial H0 custody drift")

    spec = text(root, profile["spec"], overrides)
    require_all(spec, [
        "does not promote `SD-STORE` above H0", "No write occurs before authority acceptance",
        "compiled only for unit tests and the explicit `parity-oracle` feature",
        "cannot promote `SD-STORE`",
    ], "store authority specification")

    if check_artifact:
        artifact = artifact_path or (root / profile["artifact"])
        data = artifact.read_bytes()
        if source_relative.encode() not in data or profile["binding"].encode() not in data:
            fail("authority source or binding absent from admitted artifact")


def mutation_controls(root: Path, profile) -> int:
    paths = {
        name: (root / name).read_text() for name in (
            profile["sourceModule"], "selfhost/toolchain_manifest.gc",
            "crates/gc_effects/src/store_authority.rs", "crates/gc_effects/src/policy.rs",
            "crates/gc_effects/src/policy_selfhost.rs",
            "crates/gc_effects/src/runner.rs", "crates/gc_effects/src/runner_cap_store.rs",
            "crates/gc_effects/src/runner_capability_dispatch.rs", "crates/gc_cli_driver/src/lib.rs",
            "crates/gc_effects/tests/store_caps.rs",
        )
    }
    source = paths[profile["sourceModule"]]
    mutations = [
        ({profile["sourceModule"]: source.replace("(quote :put)", "(quote :removed)", 1)}, "put phase"),
        ({profile["sourceModule"]: source.replace(":request-h (selfhost/hash::hash-term request)", ":request-h nil", 1)}, "request binding"),
        ({profile["sourceModule"]: source.replace("selfhost/printer::print-term", "selfhost/printer::removed", 1)}, "canonical bytes"),
        ({profile["sourceModule"]: source.replace("store put exceeds max_bytes", "removed max", 1)}, "operation limit"),
        ({profile["sourceModule"]: source.replace("store put exceeds max_run_bytes", "removed run max", 1)}, "run limit"),
        ({profile["sourceModule"]: source.replace("core/crypto::blake3", "core/crypto::removed", 1)}, "hash identity"),
        ({"selfhost/toolchain_manifest.gc": paths["selfhost/toolchain_manifest.gc"].replace(f'    "{profile["sourceModule"]}"\n', "", 1)}, "module custody"),
        ({"selfhost/toolchain_manifest.gc": paths["selfhost/toolchain_manifest.gc"].replace(f"    {profile['binding']}\n", "", 1)}, "binding custody"),
        ({"crates/gc_effects/src/store_authority.rs": paths["crates/gc_effects/src/store_authority.rs"].replace("decode_put_result(term, request_hash)", "Ok(StorePutDecision::Error { code: String::new(), message: String::new() })", 1)}, "strict decode"),
        ({"crates/gc_effects/src/policy_selfhost.rs": paths["crates/gc_effects/src/policy_selfhost.rs"].replace("policy.selfhost_authority = Some(SelfhostAuthorityConfig", "let removed_authority = Some(SelfhostAuthorityConfig", 1)}, "policy propagation"),
        ({"crates/gc_effects/src/runner.rs": paths["crates/gc_effects/src/runner.rs"].replace(".map(StoreAuthority::load)", ".map(removed_authority)", 1)}, "runner load"),
        ({"crates/gc_effects/src/runner_cap_store.rs": paths["crates/gc_effects/src/runner_cap_store.rs"].replace("authority.put(", "removed.put(", 1)}, "authority call"),
        ({"crates/gc_effects/src/runner_cap_store.rs": paths["crates/gc_effects/src/runner_cap_store.rs"].replace('#[cfg(any(test, feature = "parity-oracle"))]\nfn cap_store_put_parity', "fn cap_store_put_parity", 1)}, "parity isolation"),
        ({"crates/gc_effects/src/runner_cap_store.rs": paths["crates/gc_effects/src/runner_cap_store.rs"].replace(".put_bytes(&bytes)", ".put_bytes(b\"host-substitution\")", 1)}, "exact write"),
        ({"crates/gc_cli_driver/src/lib.rs": paths["crates/gc_cli_driver/src/lib.rs"].replace("CapsPolicy::load_with_selfhost_authority(", "CapsPolicy::load_without_authority(", 1)}, "CLI custody"),
        ({"crates/gc_effects/tests/store_caps.rs": paths["crates/gc_effects/tests/store_caps.rs"].replace("store_put_without_artifact_authority_fails_closed", "removed_fail_closed_control", 1)}, "negative control"),
    ]
    passed = 0
    for overrides, name in mutations:
        candidate = copy.deepcopy(profile)
        if profile["sourceModule"] in overrides:
            candidate["sourceSha256"] = source_identity(
                profile["sourceModule"], overrides[profile["sourceModule"]].encode())
        try:
            static_check(root, candidate, overrides, check_artifact=False)
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
    profile["contentIdentitySha256"] = canonical_identity(profile)
    profile_path.write_text(json.dumps(profile, indent=2) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--profile", required=True)
    parser.add_argument("--schema", required=True)
    parser.add_argument("--artifact")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--update", action="store_true")
    args = parser.parse_args()
    root = Path(args.root).resolve()
    profile_path = (root / args.profile).resolve()
    schema_path = (root / args.schema).resolve()
    if args.update:
        update(root, profile_path, schema_path)
    profile = load_json(profile_path)
    schema = load_json(schema_path)
    validate_profile(profile, schema)
    artifact = Path(args.artifact).resolve() if args.artifact else None
    static_check(root, profile, artifact_path=artifact)
    controls = mutation_controls(root, profile) if args.self_test else 0
    print(json.dumps({
        "artifact": str(artifact or (root / profile["artifact"])),
        "contentIdentitySha256": profile["contentIdentitySha256"],
        "kind": profile["kind"],
        "mutationControls": controls,
        "ok": True,
        "sourceSha256": profile["sourceSha256"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CheckError as error:
        print(f"selfhost-store-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
