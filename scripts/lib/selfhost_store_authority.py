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
    "hostMechanisms", "hostOracle", "hasRequestKind", "hasResultKind",
    "independentVerifier", "kind", "nonclaims", "productionEntrypoints",
    "getRequestKind", "getResultKind", "requestKind", "resultKind", "runtimeEvidence", "schema",
    "sourceModule", "sourceSha256", "spec", "verifyBinding", "verifyRequestKind",
    "verifyResultKind", "verifySourceModule", "verifySourceSha256", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/store::authority",
    "decisionInventory": [
        "put-payload-admission", "put-canonical-bytes", "put-operation-budget-admission",
        "put-cumulative-budget-admission", "put-content-hash-identity",
        "has-payload-hash-admission", "has-local-integrity",
        "has-remote-fallback-and-result", "get-payload-hash-admission",
        "get-source-selection", "get-byte-limit-and-integrity",
        "get-selfhost-coreform-parse", "get-cache-budget-admission",
        "verify-payload-and-hash-admission", "verify-inventory-selection-and-order",
        "verify-observation-binding", "verify-artifact-and-cumulative-limits",
        "verify-integrity-and-first-failure",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "authorized-policy-limit-transport",
        "blake3-and-byte-count-contradiction-checking",
        "atomic-write-once-filesystem-storage-and-durability",
        "bounded-stable-local-byte-observation",
        "policy-authorized-remote-presence-byte-and-transport-integrity-observation",
        "bounded-raw-inventory-enumeration-and-file-type-observation",
        "bounded-streamed-artifact-hash-observation",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_store_authority.py",
    "hasRequestKind": "genesis/store-has-authority-request-v0.1",
    "hasResultKind": "genesis/store-has-authority-result-v0.1",
    "kind": "genesis/selfhost-store-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "getRequestKind": "genesis/store-get-authority-request-v0.1",
    "getResultKind": "genesis/store-get-authority-result-v0.1",
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
    "verifyBinding": "core/store::verify-authority",
    "verifyRequestKind": "genesis/store-verify-authority-request-v0.1",
    "verifyResultKind": "genesis/store-verify-authority-result-v0.1",
    "verifySourceModule": "selfhost/store_verify_authority_v1.gc",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "internal-direct-store-consumer-authority", "h2-sd-store",
    "package-registry-vcs-authority", "r4-2-e-closure", "release-qualification",
    "sh-c-closure",
}
PRODUCTION_CLI_LOADER = """CapsPolicy::load_with_selfhost_authority(
                path,
                config.bootstrap_mode,
                config.artifact.as_deref(),
            )"""


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
    for name in ("contentIdentitySha256", "sourceSha256", "verifySourceSha256"):
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
        profile["hasRequestKind"], profile["hasResultKind"], profile["getRequestKind"],
        profile["getResultKind"], "(quote :observe-local)", "(quote :fetch-remote)",
        "store has payload must contain exactly one lowercase :hash",
        "store get payload must contain exactly one lowercase :hash",
        "selfhost/parse::parse-term", "remote cache write exceeds max_run_bytes",
    ], "GenesisCode store authority")
    if "core/effect::" in source or "core/host::" in source:
        fail("store authority contains an ambient effect or host operation")

    verify_source_relative = profile["verifySourceModule"]
    verify_source_path = root / verify_source_relative
    if (verify_source_path.is_symlink() or not verify_source_path.is_file()
            or root.resolve() not in verify_source_path.resolve().parents):
        fail("verify authority source is missing, escaping, or symlinked")
    verify_source = text(root, verify_source_relative, overrides)
    if source_identity(verify_source_relative, verify_source.encode()) != profile["verifySourceSha256"]:
        fail("verify authority source identity mismatch")
    require_all(verify_source, [
        f"(def {profile['verifyBinding']}", profile["verifyRequestKind"],
        profile["verifyResultKind"], "(quote :plan)", "(quote :inventory)",
        "(quote :observed)", "bytes-less?", "canonical-file-hash",
        "hash-vectors-equal?", "verify selected hash inventory mismatch",
        "store verify exceeded a bounded observation limit", "artifact bytes hash mismatch",
        "unsupported verify observation status",
    ], "GenesisCode store verify authority")
    if "core/effect::" in verify_source or "core/host::" in verify_source:
        fail("store verify authority contains an ambient effect or host operation")

    manifest_path = "selfhost/toolchain_manifest.gc"
    manifest = text(root, manifest_path, overrides)
    if (manifest.count(f'"{source_relative}"') != 1
            or manifest.count(f'"{verify_source_relative}"') != 1
            or manifest.count(profile["binding"]) != 1
            or manifest.count(profile["verifyBinding"]) != 1):
        fail("toolchain manifest custody drift")

    bridge_path = "crates/gc_effects/src/store_authority.rs"
    bridge = text(root, bridge_path, overrides)
    require_all(bridge, [
        f'const BINDING: &str = "{profile["binding"]}"',
        f'const VERIFY_BINDING: &str = "{profile["verifyBinding"]}"',
        "load_selfhost_coreform_toolchain_v1_with_mode", "max_alloc_units: Some(ALLOC_LIMIT)",
        "max_bytes_len: Some(PAYLOAD_LIMIT)", "max_map_len: Some(32)",
        "max_vec_len: Some(16_384)", "decode_put_result(term, request_hash)",
        "result field set mismatch", "write byte count contradiction",
        "write hash/bytes contradiction", "context.reset_counters()",
        ".get(VERIFY_BINDING)", "evaluate_with(self.verify_authority.clone(), request)",
    ], "Rust store authority bridge")
    if "unwrap_or_default()" in bridge or "unwrap_or(true)" in bridge:
        fail("store authority bridge contains success-capable defaulting")
    read_bridge_path = "crates/gc_effects/src/store_authority_read.rs"
    read_bridge = text(root, read_bridge_path, overrides)
    require_all(read_bridge, [
        f'const HAS_REQUEST_KIND: &str = "{profile["hasRequestKind"]}"',
        f'const HAS_RESULT_KIND: &str = "{profile["hasResultKind"]}"',
        f'const GET_REQUEST_KIND: &str = "{profile["getRequestKind"]}"',
        f'const GET_RESULT_KIND: &str = "{profile["getResultKind"]}"',
        "decode_has_result(term, request_hash)", "decode_get_result(term, request_hash)",
        "result hash must be lowercase hex64", "cache byte count contradiction",
        "cache hash/bytes contradiction",
    ], "Rust store read authority bridge")
    verify_bridge_path = "crates/gc_effects/src/store_authority_verify.rs"
    verify_bridge = text(root, verify_bridge_path, overrides)
    require_all(verify_bridge, [
        f'const VERIFY_REQUEST_KIND: &str = "{profile["verifyRequestKind"]}"',
        f'const VERIFY_RESULT_KIND: &str = "{profile["verifyResultKind"]}"',
        "decode_verify_result(term, request_hash)", "exact_map(",
        "result {name} must be strictly sorted and unique", "optional_checked_hash",
        "print_term(value)",
    ], "Rust store verify authority bridge")
    if "unwrap_or_default()" in verify_bridge or "unwrap_or(true)" in verify_bridge:
        fail("store verify authority bridge contains success-capable defaulting")

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
        '"core/store::put"', '"core/store::has"', '"core/store::get"',
        '"core/store::verify"',
        "let mut store_authority = None", "req.op.as_str()",
        ".map(StoreAuthority::load)", "store_authority.as_mut()",
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
    for name in ("cap_store_has_parity", "cap_store_get_parity"):
        if f'#[cfg(any(test, feature = "parity-oracle"))]\npub(super) fn {name}' not in cap:
            fail(f"store read parity oracle is not compile-time isolated: {name}")
    if '#[cfg(any(test, feature = "parity-oracle"))]\npub(super) fn cap_store_verify_parity' not in cap:
        fail("store verify parity oracle is not compile-time isolated")

    read_cap_path = "crates/gc_effects/src/runner_cap_store_read.rs"
    read_cap = text(root, read_cap_path, overrides)
    require_all(read_cap, [
        'authority.has(payload, ":plan"', 'decide(":plan", None, false, None)',
        "store.observe_bytes_limited(&hash", "StoreHasDecision::FetchRemote",
        "StoreGetDecision::FetchRemote", "client.store_has(",
        "client.store_get_opt_bounded(", '":remote-hash-mismatch"',
        "StoreGetDecision::CacheReturn",
        "store.put_bytes(&bytes)", "planner-approved hash",
        "requires the artifact-loaded GenesisCode store authority",
        'Some("artifact store read failed")',
        '"remote artifact store authentication failed"',
        '"remote artifact store request failed"',
        '"artifact store cache write failed"',
    ], "store read production route")
    if (read_cap.index('authority.has(payload, ":plan"')
            > read_cap.index("store.observe_bytes_limited(&hash")):
        fail("has filesystem observation occurs before authority plan")
    get_plan = read_cap.index('decide(":plan", None, false, None)')
    get_observe = read_cap.index("store.observe_bytes_limited(&hash", get_plan)
    if get_plan > get_observe:
        fail("get filesystem observation occurs before authority plan")
    verify_cap_path = "crates/gc_effects/src/runner_cap_store_verify.rs"
    verify_cap = text(root, verify_cap_path, overrides)
    require_all(verify_cap, [
        "const VERIFY_MAX_ENTRIES: usize = 8_192;",
        "const VERIFY_MAX_NAME_BYTES: usize = 2 * 1024 * 1024;",
        "const VERIFY_MAX_ARTIFACT_BYTES: usize = HARD_REMOTE_ARTIFACT_MAX_BYTES;",
        "const VERIFY_MAX_TOTAL_BYTES: usize = 512 * 1024 * 1024;",
        'authority.verify(\n        payload,\n        ":plan"',
        "store.observe_inventory(VERIFY_MAX_ENTRIES, VERIFY_MAX_NAME_BYTES)",
        "store.observe_hash_limited(hash, limit)", "saturating_sub(total)",
        "saturating_add(bytes)", 'Err(_error) => (":io-error", None, None)',
        "exact observed inventory binding", "failure hash at checked index",
        "one specific hash", "requires the artifact-loaded GenesisCode store authority",
        "specific_return_without_a_hash_fails_closed_without_panicking",
    ], "store verify production route")
    if verify_cap.index('authority.verify(\n        payload,\n        ":plan"') > verify_cap.index("store.observe_inventory("):
        fail("verify inventory observation occurs before authority plan")
    verify_production = verify_cap.split("#[cfg(test)]", 1)[0]
    if ("hashes[0]" in verify_production or "path.display()" in verify_production
            or "error.to_string()" in verify_production):
        fail("store verify route retains panic-capable or disclosing host transport")
    mechanism_path = "crates/gc_effects/src/store.rs"
    mechanism = text(root, mechanism_path, overrides)
    require_all(mechanism, [
        "pub(crate) enum ArtifactObservation", "pub(crate) fn observe_bytes_limited(",
        "STABLE_READ_RETRIES", "ArtifactObservation::Missing",
        "ArtifactObservation::TooLarge", "ArtifactObservation::Bytes",
        "pub(crate) enum StoreInventoryObservation", "pub(crate) fn observe_inventory(",
        "entries.sort_by(|left, right| left.name.cmp(&right.name))",
        "pub(crate) enum ArtifactHashObservation", "pub(crate) fn observe_hash_limited(",
        "hasher.update(&chunk[..count])", "StoreInventoryObservation::ResourceLimit",
    ], "bounded local store observation")
    for disclosure in (
        "Some(&error.to_string())", "Some(&rendered)",
        "artifact store read instability for {}", "path.display()",
    ):
        if disclosure in read_cap or disclosure in verify_cap or disclosure in mechanism:
            fail(f"store read route retains disclosing host error transport {disclosure!r}")

    dispatch_path = "crates/gc_effects/src/runner_capability_dispatch.rs"
    dispatch = text(root, dispatch_path, overrides)
    require_all(dispatch, [
        "store_authority: Option<&mut StoreAuthority>", '"core/store::put" => cap_store_put(',
        '"core/store::has" => runner_cap_store_read::cap_store_has(',
        '"core/store::get" => runner_cap_store_read::cap_store_get(',
        '"core/store::verify" => runner_cap_store_verify::cap_store_verify(',
        "store_authority,", "None,\n        &mut bridge_runtime",
    ], "store dispatch")

    cli_path = "crates/gc_cli_driver/src/lib.rs"
    cli = text(root, cli_path, overrides)
    require_all(cli, [
        PRODUCTION_CLI_LOADER, "Rust effect-policy authority is not compiled into production",
    ], "production CLI policy route")

    tests_path = "crates/gc_effects/tests/store_caps.rs"
    tests = text(root, tests_path, overrides)
    require_all(tests, [
        "store_put_without_artifact_authority_fails_closed",
        "store_put_payload_shape_is_decided_as_a_sealed_error",
        "semantic rejection must happen before any store write",
        "store_put_enforces_cumulative_store_run_budget",
        "store_get_without_artifact_authority_fails_closed",
        "store_read_hash_admission_precedes_filesystem_observation",
        "store_has_and_get_classify_local_hash_mismatch_as_corruption",
        "store_read_io_errors_are_stable_and_nondisclosing",
        "store_verify_without_artifact_authority_fails_closed",
        "store_verify_hash_admission_precedes_inventory_observation",
        "store_verify_filters_inventory_and_reports_first_corruption",
        "store_verify_io_errors_are_stable_and_nondisclosing",
    ], "store authority tests")
    cli_tests_path = "crates/gc_cli/tests/cli_store_verify_authority.rs"
    cli_tests = text(root, cli_tests_path, overrides)
    require_all(cli_tests, [
        "production_store_verify_supports_specific_and_filtered_scan_modes",
        "production_store_verify_reports_authoritative_corruption_code",
        'arg("--selfhost-artifact")', 'stderr(predicate::str::contains("core/store/corruption"))',
    ], "native store verify CLI tests")

    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-STORE"]
    if len(rows) != 1:
        fail("SD-STORE ledger row missing or duplicated")
    row = rows[0]
    limitations = " ".join(row.get("limitations", []))
    if (row.get("currentLevel") != "H0" or source_relative not in row.get("producingImplementationPaths", [])
            or verify_source_relative not in row.get("producingImplementationPaths", [])
            or bridge_path not in row.get("productionAuthorityPaths", [])
            or verify_bridge_path not in row.get("productionAuthorityPaths", [])
            or verify_cap_path not in row.get("productionAuthorityPaths", [])
            or profile["spec"] not in row.get("specAuthorityPaths", [])
            or profile["independentVerifier"] not in row.get("verifierPaths", [])
            or "direct-store consumers" not in limitations):
        fail("SD-STORE partial H0 custody drift")

    spec = text(root, profile["spec"], overrides)
    require_all(spec, [
        "does not promote `SD-STORE` above H0", "No write occurs before authority acceptance",
        "compiled only for unit tests and the explicit `parity-oracle` feature",
        "cannot promote `SD-STORE`", "at most 8,192 raw entries",
        "512 MiB of cumulative artifact bytes", "first raw-byte-ordered failure",
    ], "store authority specification")

    if check_artifact:
        artifact = artifact_path or (root / profile["artifact"])
        data = artifact.read_bytes()
        if (source_relative.encode() not in data or profile["binding"].encode() not in data
                or verify_source_relative.encode() not in data
                or profile["verifyBinding"].encode() not in data):
            fail("authority source or binding absent from admitted artifact")


def mutation_controls(root: Path, profile) -> int:
    paths = {
        name: (root / name).read_text() for name in (
            profile["sourceModule"], profile["verifySourceModule"],
            "selfhost/toolchain_manifest.gc",
            "crates/gc_effects/src/store_authority.rs",
            "crates/gc_effects/src/store_authority_read.rs",
            "crates/gc_effects/src/store_authority_verify.rs",
            "crates/gc_effects/src/policy.rs",
            "crates/gc_effects/src/policy_selfhost.rs",
            "crates/gc_effects/src/runner.rs", "crates/gc_effects/src/runner_cap_store.rs",
            "crates/gc_effects/src/runner_cap_store_read.rs",
            "crates/gc_effects/src/runner_cap_store_verify.rs",
            "crates/gc_effects/src/store.rs",
            "crates/gc_effects/src/runner_capability_dispatch.rs", "crates/gc_cli_driver/src/lib.rs",
            "crates/gc_effects/tests/store_caps.rs",
            "crates/gc_cli/tests/cli_store_verify_authority.rs",
        )
    }
    source = paths[profile["sourceModule"]]
    verify_source = paths[profile["verifySourceModule"]]
    mutations = [
        ({profile["sourceModule"]: source.replace("(quote :put)", "(quote :removed)")}, "put phase"),
        ({profile["sourceModule"]: source.replace(":request-h (selfhost/hash::hash-term request)", ":request-h nil")}, "request binding"),
        ({profile["sourceModule"]: source.replace("selfhost/printer::print-term", "selfhost/printer::removed", 1)}, "canonical bytes"),
        ({profile["sourceModule"]: source.replace("store put exceeds max_bytes", "removed max", 1)}, "operation limit"),
        ({profile["sourceModule"]: source.replace("store put exceeds max_run_bytes", "removed run max", 1)}, "run limit"),
        ({profile["sourceModule"]: source.replace("core/crypto::blake3", "core/crypto::removed")}, "hash identity"),
        ({profile["sourceModule"]: source.replace("(quote :observe-local)", "(quote :removed-local)")}, "read plan"),
        ({profile["sourceModule"]: source.replace("selfhost/parse::parse-term", "selfhost/parse::removed", 1)}, "selfhost parse"),
        ({profile["sourceModule"]: source.replace("remote cache write exceeds max_run_bytes", "removed cache limit", 1)}, "cache budget"),
        ({profile["verifySourceModule"]: verify_source.replace("(quote :plan)", "(quote :removed-plan)", 1)}, "verify plan"),
        ({profile["verifySourceModule"]: verify_source.replace("bytes-less?", "removed-order")}, "verify raw ordering"),
        ({profile["verifySourceModule"]: verify_source.replace("canonical-file-hash", "removed-selection")}, "verify inventory selection"),
        ({profile["verifySourceModule"]: verify_source.replace("hash-vectors-equal?", "removed-binding")}, "verify observation binding"),
        ({profile["verifySourceModule"]: verify_source.replace("store verify exceeded a bounded observation limit", "removed verify bound")}, "verify cumulative bound"),
        ({profile["verifySourceModule"]: verify_source.replace("artifact bytes hash mismatch", "removed hash mismatch")}, "verify integrity verdict"),
        ({profile["verifySourceModule"]: verify_source.replace("unsupported verify observation status", "removed status closure")}, "verify status closure"),
        ({"selfhost/toolchain_manifest.gc": paths["selfhost/toolchain_manifest.gc"].replace(f'    "{profile["sourceModule"]}"\n', "", 1)}, "module custody"),
        ({"selfhost/toolchain_manifest.gc": paths["selfhost/toolchain_manifest.gc"].replace(f"    {profile['binding']}\n", "", 1)}, "binding custody"),
        ({"selfhost/toolchain_manifest.gc": paths["selfhost/toolchain_manifest.gc"].replace(f'    "{profile["verifySourceModule"]}"\n', "", 1)}, "verify module custody"),
        ({"selfhost/toolchain_manifest.gc": paths["selfhost/toolchain_manifest.gc"].replace(f"    {profile['verifyBinding']}\n", "", 1)}, "verify binding custody"),
        ({"crates/gc_effects/src/store_authority.rs": paths["crates/gc_effects/src/store_authority.rs"].replace("decode_put_result(term, request_hash)", "Ok(StorePutDecision::Error { code: String::new(), message: String::new() })", 1)}, "strict decode"),
        ({"crates/gc_effects/src/store_authority.rs": paths["crates/gc_effects/src/store_authority.rs"].replace(".get(VERIFY_BINDING)", ".get(BINDING)", 1)}, "verify binding load"),
        ({"crates/gc_effects/src/store_authority_read.rs": paths["crates/gc_effects/src/store_authority_read.rs"].replace("decode_get_result(term, request_hash)", "panic!(\"removed\")", 1)}, "read strict decode"),
        ({"crates/gc_effects/src/store_authority_verify.rs": paths["crates/gc_effects/src/store_authority_verify.rs"].replace("decode_verify_result(term, request_hash)", "panic!(\"removed\")", 1)}, "verify strict decode"),
        ({"crates/gc_effects/src/policy_selfhost.rs": paths["crates/gc_effects/src/policy_selfhost.rs"].replace("policy.selfhost_authority = Some(SelfhostAuthorityConfig", "let removed_authority = Some(SelfhostAuthorityConfig", 1)}, "policy propagation"),
        ({"crates/gc_effects/src/runner.rs": paths["crates/gc_effects/src/runner.rs"].replace(".map(StoreAuthority::load)", ".map(removed_authority)", 1)}, "runner load"),
        ({"crates/gc_effects/src/runner_cap_store.rs": paths["crates/gc_effects/src/runner_cap_store.rs"].replace("authority.put(", "removed.put(", 1)}, "authority call"),
        ({"crates/gc_effects/src/runner_cap_store.rs": paths["crates/gc_effects/src/runner_cap_store.rs"].replace('#[cfg(any(test, feature = "parity-oracle"))]\nfn cap_store_put_parity', "fn cap_store_put_parity", 1)}, "parity isolation"),
        ({"crates/gc_effects/src/runner_cap_store.rs": paths["crates/gc_effects/src/runner_cap_store.rs"].replace(".put_bytes(&bytes)", ".put_bytes(b\"host-substitution\")", 1)}, "exact write"),
        ({"crates/gc_effects/src/runner_cap_store_read.rs": paths["crates/gc_effects/src/runner_cap_store_read.rs"].replace('authority.has(payload, ":plan"', 'authority.has(payload, ":removed"', 1)}, "has plan ordering"),
        ({"crates/gc_effects/src/runner_cap_store_read.rs": paths["crates/gc_effects/src/runner_cap_store_read.rs"].replace('decide(":plan", None, false, None)', 'decide(":removed", None, false, None)', 1)}, "get plan ordering"),
        ({"crates/gc_effects/src/runner_cap_store_read.rs": paths["crates/gc_effects/src/runner_cap_store_read.rs"].replace("store.put_bytes(&bytes)", "store.put_bytes(b\"substitution\")", 1)}, "cache exact write"),
        ({"crates/gc_effects/src/runner_cap_store_verify.rs": paths["crates/gc_effects/src/runner_cap_store_verify.rs"].replace('        ":plan",', '        ":removed",', 1)}, "verify authority-first plan"),
        ({"crates/gc_effects/src/runner_cap_store_verify.rs": paths["crates/gc_effects/src/runner_cap_store_verify.rs"].replace("const VERIFY_MAX_ENTRIES: usize = 8_192;", "const VERIFY_MAX_ENTRIES: usize = usize::MAX;", 1)}, "verify entry bound"),
        ({"crates/gc_effects/src/runner_cap_store_verify.rs": paths["crates/gc_effects/src/runner_cap_store_verify.rs"].replace("store.observe_hash_limited(hash, limit)", "store.get_bytes(hash).map(|_| ArtifactHashObservation::Missing)", 1)}, "verify streamed observation"),
        ({"crates/gc_effects/src/store.rs": paths["crates/gc_effects/src/store.rs"].replace("entries.sort_by(|left, right| left.name.cmp(&right.name));", "", 1)}, "verify inventory sorting"),
        ({"crates/gc_effects/src/store.rs": paths["crates/gc_effects/src/store.rs"].replace("hasher.update(&chunk[..count]);", "", 1)}, "verify streaming hash"),
        ({"crates/gc_effects/src/runner_capability_dispatch.rs": paths["crates/gc_effects/src/runner_capability_dispatch.rs"].replace('"core/store::verify" => runner_cap_store_verify::cap_store_verify(', '"core/store::verify" => runner_cap_store::cap_store_verify_parity(', 1)}, "verify dispatch"),
        ({"crates/gc_effects/src/runner_cap_store.rs": paths["crates/gc_effects/src/runner_cap_store.rs"].replace('#[cfg(any(test, feature = "parity-oracle"))]\npub(super) fn cap_store_verify_parity', "pub(super) fn cap_store_verify_parity", 1)}, "verify parity isolation"),
        ({"crates/gc_cli_driver/src/lib.rs": paths["crates/gc_cli_driver/src/lib.rs"].replace(PRODUCTION_CLI_LOADER, PRODUCTION_CLI_LOADER.replace("load_with_selfhost_authority", "load_without_authority"), 1)}, "CLI custody"),
        ({"crates/gc_effects/tests/store_caps.rs": paths["crates/gc_effects/tests/store_caps.rs"].replace("store_put_without_artifact_authority_fails_closed", "removed_fail_closed_control", 1)}, "negative control"),
        ({"crates/gc_effects/tests/store_caps.rs": paths["crates/gc_effects/tests/store_caps.rs"].replace("store_verify_without_artifact_authority_fails_closed", "removed_verify_fail_closed_control", 1)}, "verify negative control"),
        ({"crates/gc_cli/tests/cli_store_verify_authority.rs": paths["crates/gc_cli/tests/cli_store_verify_authority.rs"].replace("production_store_verify_supports_specific_and_filtered_scan_modes", "removed_verify_cli_control", 1)}, "verify native CLI control"),
    ]
    passed = 0
    for overrides, name in mutations:
        candidate = copy.deepcopy(profile)
        if profile["sourceModule"] in overrides:
            candidate["sourceSha256"] = source_identity(
                profile["sourceModule"], overrides[profile["sourceModule"]].encode())
        if profile["verifySourceModule"] in overrides:
            candidate["verifySourceSha256"] = source_identity(
                profile["verifySourceModule"], overrides[profile["verifySourceModule"]].encode())
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
    verify_relative = profile["verifySourceModule"]
    profile["verifySourceSha256"] = source_identity(
        verify_relative, (root / verify_relative).read_bytes())
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
        "verifySourceSha256": profile["verifySourceSha256"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CheckError as error:
        print(f"selfhost-store-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
