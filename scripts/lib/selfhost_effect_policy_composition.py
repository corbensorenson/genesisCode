#!/usr/bin/env python3
"""Independent verifier for H2 GenesisCode effect-policy composition authority."""

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


def identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(canonical).hexdigest()


FIELDS = {
    "artifact",
    "auditDate",
    "binding",
    "contentIdentitySha256",
    "decisionInventory",
    "hostOracle",
    "independentVerifier",
    "inventoryBinding",
    "inventoryRequestKind",
    "inventoryResultKind",
    "kind",
    "maxPolicyOperations",
    "nonclaims",
    "productionEntrypoints",
    "requestKind",
    "resourceBinding",
    "resourceRequestKind",
    "resourceResultKind",
    "residualDecisionInventory",
    "resultKind",
    "runtimeEvidence",
    "schema",
    "sourceModule",
    "sourceModules",
    "sourceSha256",
    "spec",
    "version",
}

DECISIONS = [
    "baseline-operation-admission",
    "candidate-operation-inventory",
    "canonical-log-cap-descriptor",
    "global-log-and-store-resource-limits",
    "global-log-store-refs-location-defaults",
    "global-store-credential-and-tls-policy",
    "global-store-remote-target-policy",
    "per-operation-allow-precedence",
    "per-operation-base-directory-selection",
    "per-operation-bridge-activation-policy",
    "per-operation-bridge-command-allowlist-policy",
    "per-operation-bridge-digest-pin-policy",
    "per-operation-bridge-invocation-policy",
    "per-operation-credential-and-tls-policy",
    "per-operation-crypto-policy",
    "per-operation-database-policy",
    "per-operation-enforcement-control-selection",
    "per-operation-ffi-allowlist-and-bound-policy",
    "per-operation-ffi-signed-policy-admission",
    "per-operation-ffi-signed-policy-metadata",
    "per-operation-gfx-first-party-profile-policy",
    "per-operation-gpu-backend-policy",
    "per-operation-gpu-fallback-policy",
    "per-operation-max-bytes-policy",
    "per-operation-network-policy",
    "per-operation-plugin-allowlist-policy",
    "per-operation-process-program-policy",
    "per-operation-xr-device-and-backend-policy",
    "runtime-resource-limits",
    "task-resource-limits-and-default-workers",
]

RESIDUALS = {
    "effect-execution-and-hard-cancellation",
    "bridge-identity-validation-execution-and-model-provider-lifecycle",
    "path-and-secret-resolution",
    "replay-execution-and-validation",
    "toml-syntax-and-type-decoding",
}


def validate(profile, schema, check_identity=True):
    if (
        schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != FIELDS
        or set(schema.get("properties", {})) != FIELDS
    ):
        fail("schema closure drift")
    if set(profile) != FIELDS:
        fail("profile field drift")
    constants = {
        "artifact": "selfhost/toolchain.gc",
        "binding": "core/effects::policy-authority",
        "decisionInventory": DECISIONS,
        "hostOracle": {"required": False, "removalTask": "R4.2.d"},
        "independentVerifier": "scripts/lib/selfhost_effect_policy_composition.py",
        "inventoryBinding": "core/effects::policy-inventory-authority",
        "inventoryRequestKind": "genesis/effect-policy-inventory-request-v0.1",
        "inventoryResultKind": "genesis/effect-policy-inventory-result-v0.1",
        "kind": "genesis/selfhost-effect-policy-composition-v0.1",
        "maxPolicyOperations": 4096,
        "productionEntrypoints": ["genesis", "genesis_wasi"],
        "requestKind": "genesis/effect-policy-authority-request-v0.20",
        "resourceBinding": "core/effects::resource-policy-authority",
        "resourceRequestKind": "genesis/effect-resource-policy-request-v0.5",
        "resourceResultKind": "genesis/effect-resource-policy-result-v0.5",
        "resultKind": "genesis/effect-policy-authority-result-v0.20",
        "runtimeEvidence": {
            "allocationLimit": 40_000_000,
            "stepLimit": 20_000_000,
            "timeoutSeconds": 30,
        },
        "schema": "docs/spec/SELFHOST_EFFECT_POLICY_COMPOSITION_v0.1.schema.json",
        "sourceModule": "selfhost/effect_policy_authority_v1.gc",
        "sourceModules": [
            "selfhost/effect_policy_crypto_v1.gc",
            "selfhost/effect_policy_network_v1.gc",
            "selfhost/effect_policy_plugin_v1.gc",
            "selfhost/effect_policy_ffi_v1.gc",
            "selfhost/effect_policy_bridge_v1.gc",
            "selfhost/effect_policy_gpu_v1.gc",
            "selfhost/effect_policy_gfx_v1.gc",
            "selfhost/effect_policy_xr_v1.gc",
            "selfhost/effect_policy_resource_authority_v1.gc",
            "selfhost/effect_policy_authority_v1.gc",
        ],
        "spec": "docs/spec/SELFHOST_EFFECT_POLICY_COMPOSITION_v0.1.md",
        "version": "0.1.28",
    }
    for key, expected in constants.items():
        if profile.get(key) != expected:
            fail(f"profile {key} drift")
    for key in (
        "decisionInventory",
        "requestKind",
        "resourceRequestKind",
        "resourceResultKind",
        "resultKind",
        "sourceModules",
        "version",
    ):
        if schema["properties"].get(key, {}).get("const") != constants[key]:
            fail(f"schema {key} drift")
    if set(profile.get("residualDecisionInventory", [])) != RESIDUALS:
        fail("residual decision inventory drift")
    if set(profile.get("nonclaims", [])) != {
        "bootstrap-fixpoint",
        "r4-2-d-closure",
        "release-qualification",
        "replay-authority",
        "sh-c-closure",
    }:
        fail("nonclaim inventory drift")
    for key in ("contentIdentitySha256", "sourceSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(key, ""))):
            fail(f"invalid {key}")
    if check_identity and profile["contentIdentitySha256"] != identity(profile):
        fail("profile content identity mismatch")


def source_files(root: Path):
    for crate in ("gc_cli_driver", "gc_obligations"):
        yield from (root / "crates" / crate / "src").rglob("*.rs")


def source_identity(root: Path, source_modules) -> str:
    digest = hashlib.sha256()
    for relative in source_modules:
        path = root / relative
        digest.update(relative.encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def static_check(root: Path, profile):
    source_paths = [root / relative for relative in profile["sourceModules"]]
    for source_path in source_paths:
        if source_path.is_symlink() or not source_path.is_file() or root not in source_path.resolve().parents:
            fail("effect-policy source is missing, escaping, or symlinked")
    source_hash = source_identity(root, profile["sourceModules"])
    if source_hash != profile["sourceSha256"]:
        fail("effect-policy source identity mismatch")

    manifest = (root / "selfhost/toolchain_manifest.gc").read_text()
    for source_module in profile["sourceModules"]:
        if manifest.count(f'"{source_module}"') != 1:
            fail("effect-policy source manifest custody drift")
    if manifest.count(profile["binding"]) != 1:
        fail("effect-policy binding manifest custody drift")
    if manifest.count(profile["inventoryBinding"]) != 1:
        fail("effect-policy inventory binding manifest custody drift")
    if manifest.count(profile["resourceBinding"]) != 1:
        fail("effect-policy resource binding manifest custody drift")

    authority_root = (root / "crates/gc_effects/src/policy_authority.rs").read_text()
    if authority_root.count('#[path = "policy_authority_resource.rs"]') != 1:
        fail("effect-policy resource boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_gpu.rs"]') != 1:
        fail("effect-policy GPU boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_gfx.rs"]') != 1:
        fail("effect-policy GFX boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_xr.rs"]') != 1:
        fail("effect-policy XR boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_request.rs"]') != 1:
        fail("effect-policy request boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_process.rs"]') != 1:
        fail("effect-policy process boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_database.rs"]') != 1:
        fail("effect-policy database boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_network.rs"]') != 1:
        fail("effect-policy network boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_cap.rs"]') != 1:
        fail("effect-policy capability boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_crypto.rs"]') != 1:
        fail("effect-policy crypto boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_ffi.rs"]') != 1:
        fail("effect-policy ffi boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_plugin.rs"]') != 1:
        fail("effect-policy plugin boundary decomposition drift")
    if authority_root.count('#[path = "policy_authority_store_credentials.rs"]') != 1:
        fail("effect-policy store credential boundary decomposition drift")
    resource_boundary_path = root / "crates/gc_effects/src/policy_authority_resource.rs"
    if resource_boundary_path.is_symlink() or not resource_boundary_path.is_file():
        fail("effect-policy resource host boundary is missing or symlinked")
    transport_boundary_path = root / "crates/gc_effects/src/policy_transport.rs"
    if transport_boundary_path.is_symlink() or not transport_boundary_path.is_file():
        fail("effect-policy neutral transport boundary is missing or symlinked")
    process_boundary_path = root / "crates/gc_effects/src/policy_authority_process.rs"
    if process_boundary_path.is_symlink() or not process_boundary_path.is_file():
        fail("effect-policy process host boundary is missing or symlinked")
    database_boundary_path = root / "crates/gc_effects/src/policy_authority_database.rs"
    if database_boundary_path.is_symlink() or not database_boundary_path.is_file():
        fail("effect-policy database host boundary is missing or symlinked")
    network_boundary_path = root / "crates/gc_effects/src/policy_authority_network.rs"
    if network_boundary_path.is_symlink() or not network_boundary_path.is_file():
        fail("effect-policy network host boundary is missing or symlinked")
    override_boundary_path = root / "crates/gc_effects/src/policy_authority_override.rs"
    if override_boundary_path.is_symlink() or not override_boundary_path.is_file():
        fail("effect-policy operation override boundary is missing or symlinked")
    cap_boundary_path = root / "crates/gc_effects/src/policy_authority_cap.rs"
    if cap_boundary_path.is_symlink() or not cap_boundary_path.is_file():
        fail("effect-policy capability host boundary is missing or symlinked")
    crypto_boundary_path = root / "crates/gc_effects/src/policy_authority_crypto.rs"
    if crypto_boundary_path.is_symlink() or not crypto_boundary_path.is_file():
        fail("effect-policy crypto host boundary is missing or symlinked")
    ffi_boundary_path = root / "crates/gc_effects/src/policy_authority_ffi.rs"
    if ffi_boundary_path.is_symlink() or not ffi_boundary_path.is_file():
        fail("effect-policy ffi host boundary is missing or symlinked")
    plugin_boundary_path = root / "crates/gc_effects/src/policy_authority_plugin.rs"
    if plugin_boundary_path.is_symlink() or not plugin_boundary_path.is_file():
        fail("effect-policy plugin host boundary is missing or symlinked")
    store_credentials_path = root / "crates/gc_effects/src/policy_authority_store_credentials.rs"
    if store_credentials_path.is_symlink() or not store_credentials_path.is_file():
        fail("effect-policy store credential boundary is missing or symlinked")
    gfx_boundary_path = root / "crates/gc_effects/src/policy_authority_gfx.rs"
    if gfx_boundary_path.is_symlink() or not gfx_boundary_path.is_file():
        fail("effect-policy GFX host boundary is missing or symlinked")
    xr_boundary_path = root / "crates/gc_effects/src/policy_authority_xr.rs"
    if xr_boundary_path.is_symlink() or not xr_boundary_path.is_file():
        fail("effect-policy XR host boundary is missing or symlinked")
    xr_decode_path = root / "crates/gc_effects/src/policy_authority_xr_decode.rs"
    if xr_decode_path.is_symlink() or not xr_decode_path.is_file():
        fail("effect-policy XR strict decoder is missing or symlinked")
    if xr_boundary_path.read_text().count(
        '#[path = "policy_authority_xr_decode.rs"]'
    ) != 1:
        fail("effect-policy XR decoder decomposition drift")
    request_boundary_path = root / "crates/gc_effects/src/policy_authority_request.rs"
    if request_boundary_path.is_symlink() or not request_boundary_path.is_file():
        fail("effect-policy request host boundary is missing or symlinked")
    authority = (
        authority_root
        + resource_boundary_path.read_text()
        + process_boundary_path.read_text()
        + database_boundary_path.read_text()
        + network_boundary_path.read_text()
        + override_boundary_path.read_text()
        + cap_boundary_path.read_text()
        + crypto_boundary_path.read_text()
        + ffi_boundary_path.read_text()
        + plugin_boundary_path.read_text()
        + store_credentials_path.read_text()
        + gfx_boundary_path.read_text()
        + xr_boundary_path.read_text()
        + xr_decode_path.read_text()
        + request_boundary_path.read_text()
    )
    required_authority = [
        "const MAX_POLICY_OPS: usize = 4_096;",
        "const POLICY_AUTHORITY_STEP_LIMIT: u64 = 20_000_000;",
        "const POLICY_AUTHORITY_ALLOC_LIMIT: u64 = 40_000_000;",
        profile["requestKind"],
        profile["resultKind"],
        profile["inventoryRequestKind"],
        profile["inventoryResultKind"],
        profile["resourceRequestKind"],
        profile["resourceResultKind"],
        'get("core/effects::policy-authority")',
        'get("core/effects::policy-inventory-authority")',
        'get("core/effects::resource-policy-authority")',
        "inventory result invented operation",
        "let candidate_domain: BTreeSet<String> = policy.ops.keys().cloned().collect();",
        "let candidate_states = std::mem::take(&mut policy.ops);",
        "authorized op is outside the transported candidate domain",
        "let request_hash = hash_term(&request);",
        "op_policy.authorized_cap = Some(authorized.cap);",
        "op_policy.authorized_xr_policy = Some(authorized.xr);",
        "op_policy.base_dir = authorized.base_dir;",
        "op_policy.create_dirs = authorized.create_dirs;",
        "op_policy.timeout_ms = authorized.timeout_ms;",
        "op_policy.log_inline_max_bytes = authorized.log_inline_max_bytes;",
        "op_policy.authorized_max_bytes = Some(authorized.max_bytes);",
        "op_policy.authorized_process_programs = Some(authorized.process_programs);",
        "op_policy.authorized_database = Some(authorized.database);",
        "op_policy.authorized_network = Some(authorized.network);",
        "op_policy.authorized_crypto = Some(authorized.crypto);",
        "op_policy.authorized_sync_credentials = Some(authorized.credentials);",
        "op_policy.authorized_ffi = Some(authorized.ffi);",
        "op_policy.authorized_plugin = Some(authorized.plugin);",
        "op_policy.authorized_gfx_profile = Some(authorized.gfx);",
        "policy.task = authorized_resources.task;",
        "policy.runtime = authorized_resources.runtime;",
        "policy.log.inline_max_bytes = authorized_resources.log_inline_max_bytes;",
        "policy.log.max_artifact_bytes_per_run =",
        "policy.log.store_dir = authorized_resources.log_store_dir;",
        "policy.refs.path = authorized_resources.refs_path;",
        "policy.store.dir = authorized_resources.store_dir;",
        "policy.store.max_run_bytes = authorized_resources.store_max_run_bytes;",
        "policy.store.authorized_credentials = Some(authorized_resources.store_credentials);",
        "super::store_credentials::input(store)",
        "super::store_credentials::decode(map_field(store_map, \":credential-policy\")?, raw_store)",
        "inline bearer decision has no retained inline token",
        "environment bearer decision substituted its environment name",
        "store_credentials::decode_operation(",
        "store credential decision substituted an mTLS path",
    ]
    for token in required_authority:
        if token not in authority:
            fail(f"missing effect-policy boundary token: {token}")

    policy = (root / "crates/gc_effects/src/policy.rs").read_text()
    policy_selfhost = (root / "crates/gc_effects/src/policy_selfhost.rs").read_text()
    transport = transport_boundary_path.read_text()
    if policy.count('#[path = "policy_transport.rs"]') != 1:
        fail("neutral policy transport decomposition drift")
    if "pub fn load_with_selfhost_authority(" not in policy:
        fail("self-host policy loader is missing")
    if policy.count('#[path = "policy_selfhost.rs"]') != 1:
        fail("self-host policy authority propagation decomposition drift")
    selfhost_loader = policy.split("pub fn load_with_selfhost_authority(", 1)[1].split(
        "pub(crate) fn authorized_cap", 1
    )[0]
    if (
        selfhost_loader.find("Self::from_toml_str_with_selfhost_authority")
        >= selfhost_loader.find("policy.resolve_relative_paths")
    ):
        fail("self-host policy loader must authorize base directories before host path resolution")
    selfhost_source_loader = policy_selfhost.split(
        "pub fn from_toml_str_with_selfhost_authority(", 1
    )[1].split("pub(crate) fn selfhost_authority_config", 1)[0]
    if (
        selfhost_source_loader.count("policy_transport::decode_selfhost_transport(source)") != 1
        or selfhost_source_loader.count("policy_authority::authorize_policy") != 1
        or "Self::from_toml_str" in selfhost_source_loader
    ):
        fail("self-host policy source loader bypasses the neutral transport boundary")
    for token in (
        "toml::from_str(source)",
        ".entry(op.to_string())",
        "policy.ops.insert(op.clone(), empty_operation(extra));",
        "policy.store.auth_token = retain_string(store, \"auth_token\");",
        "policy.store.authorized_credentials = None;",
    ):
        if token not in transport:
            fail(f"neutral policy transport custody drift: {token}")
    for token in (
        "contradicts independently reconstructed candidate operations",
        "contradicts independently reconstructed policy composition",
        "resource result contradicts independently reconstructed log/runtime/store/task policy",
        "legacy_policy_term",
        "let legacy_ops =",
        "let legacy_candidates =",
    ):
        if token in authority_root or token in resource_boundary_path.read_text():
            fail(f"live Rust effect-policy oracle remains reachable: {token}")
    legacy_defaults = (
        'pol.log.store_dir = Some(base.join(".genesis").join("store"));',
        'pol.store.dir = Some(base.join(".genesis").join("store"));',
        'pol.refs.path = Some(base.join(".genesis").join("refs.gc"));',
    )
    for token in legacy_defaults:
        if policy.count(token) != 1:
            fail(f"legacy location-default oracle drift: {token}")
    for token in (
        'policy.log.store_dir = Some(base.join(".genesis").join("store"));',
        'policy.store.dir = Some(base.join(".genesis").join("store"));',
        'policy.refs.path = Some(base.join(".genesis").join("refs.gc"));',
    ):
        if token in policy:
            fail(f"production host still selects a location default: {token}")
    runner = (root / "crates/gc_effects/src/runner_response_budget.rs").read_text()
    if "policy.authorized_cap(op)" not in runner:
        fail("effect log does not consume the authorized capability descriptor")
    authority_lookup = "let Some(authorized) = &pol.authorized_max_bytes"
    legacy_lookup = 'pol.extra.get(key)'
    if runner.count(authority_lookup) != 1 or runner.count(legacy_lookup) != 1:
        fail("generic max-byte enforcement authority inventory drift")
    if runner.find(authority_lookup) >= runner.find(legacy_lookup):
        fail("generic max-byte enforcement consults raw policy before authority state")
    bridge_policy = (root / "crates/gc_effects/src/runner_host_bridge_policy.rs").read_text()
    bridge_authority_lookup = "if let Some(authorized) = &pol.authorized_max_bytes"
    bridge_legacy_lookup = 'pol.extra.get("max_bytes")'
    if bridge_policy.count(bridge_authority_lookup) != 1 or bridge_policy.count(bridge_legacy_lookup) != 1:
        fail("bridge max-byte enforcement authority inventory drift")
    if bridge_policy.find(bridge_authority_lookup) >= bridge_policy.find(bridge_legacy_lookup):
        fail("bridge max-byte enforcement consults raw policy before authority state")
    process_policy = (
        root / "crates/gc_effects/src/runner_capability_dispatch/process.rs"
    ).read_text()
    process_authority_lookup = "if let Some(authorized) = &pol.authorized_process_programs"
    process_legacy_lookup = 'pol.extra.get("allow_programs")'
    if process_policy.count(process_authority_lookup) != 1 or process_policy.count(process_legacy_lookup) != 1:
        fail("process-program enforcement authority inventory drift")
    if process_policy.find(process_authority_lookup) >= process_policy.find(process_legacy_lookup):
        fail("process-program enforcement consults raw policy before authority state")
    database_policy = (
        root / "crates/gc_effects/src/runner_capability_dispatch/db.rs"
    ).read_text()
    database_bound_authority_lookup = "if let Some(authorized) = &pol.authorized_database"
    database_list_authority_lookup = (
        "if let Some(authorized) = pol.and_then(|policy| policy.authorized_database.as_ref())"
    )
    if database_policy.count(database_bound_authority_lookup) != 1 or database_policy.count(database_list_authority_lookup) != 2:
        fail("database-bound enforcement authority inventory drift")
    if (
        database_policy.count("parse_nonempty_string_array(") != 2
        or database_policy.find(database_list_authority_lookup)
        >= database_policy.find("parse_nonempty_string_array(")
        or database_policy.count("pol.extra.get(key)") != 1
        or database_policy.find(database_bound_authority_lookup)
        >= database_policy.find("pol.extra.get(key)")
    ):
        fail("database enforcement consults raw policy before authority state")
    network_policy = (
        root / "crates/gc_effects/src/runner_capability_dispatch/net_policy.rs"
    ).read_text()
    for start, end, fallback in (
        ("fn net_allowlist_from_policy", "fn net_allow_http_from_policy", "pol.extra"),
        ("fn net_allow_http_from_policy", "fn net_wasi_network_profile_from_policy", "pol.extra"),
        ("fn net_wasi_network_profile_from_policy", "fn net_bind_hosts_from_policy", "pol.extra"),
        ("fn net_bind_hosts_from_policy", "#[derive(Debug, Clone)]", "parse_nonempty_string_array"),
        ("fn net_bind_ports_from_policy", "pub(super) fn net_max_request_bytes_from_policy", "parse_nonempty_u16_array"),
        ("pub(super) fn net_max_request_bytes_from_policy", "fn validate_net_wasi_profile", "pol.extra"),
    ):
        body = network_policy.split(start, 1)[1].split(end, 1)[0]
        if "authorized_network" not in body or body.find("authorized_network") >= body.find(fallback):
            fail(f"network enforcement consults raw policy before authority state: {start}")
    remote_policy = (
        root / "crates/gc_effects/src/runner_remote_ops/policy_auth.rs"
    ).read_text()
    if remote_policy.count('#[path = "policy_auth_store.rs"]') != 1:
        fail("store credential consumer decomposition drift")
    store_auth_path = root / "crates/gc_effects/src/runner_remote_ops/policy_auth_store.rs"
    if store_auth_path.is_symlink() or not store_auth_path.is_file():
        fail("store credential consumer boundary is missing or symlinked")
    store_auth_policy = store_auth_path.read_text()
    remote_production = remote_policy.split("#[cfg(test)]", 1)[0] + store_auth_policy
    for start, end in (
        ("fn parse_wasi_network_profile", "fn validate_wasi_remote_profile"),
        ("pub(super) fn sync_policy_from_op", "pub(super) fn sync_normalize_and_check_remote"),
    ):
        body = remote_policy.split(start, 1)[1].split(end, 1)[0]
        if "authorized_network" not in body or body.find("authorized_network") >= body.find("pol.extra"):
            fail(f"remote enforcement consults raw policy before authority state: {start}")
    sync_policy = remote_policy.split("pub(super) fn sync_policy_from_op", 1)[1].split(
        "pub(super) fn sync_normalize_and_check_remote", 1
    )[0]
    if "authorized_sync_credentials" not in sync_policy:
        fail("sync credential consumer does not require installed authority state")
    for key in (
        "auth_token",
        "auth_token_env",
        "basic_username",
        "basic_password",
        "basic_password_env",
        "mtls_ca_pem",
        "mtls_identity_pem",
    ):
        if f'pol.extra.get("{key}")' in sync_policy:
            fail(f"sync credential consumer rereads raw policy field: {key}")
    crypto_policy = (
        root / "crates/gc_effects/src/runner_capability_dispatch/crypto.rs"
    ).read_text()
    for start, end, fallback in (
        ("fn crypto_positive_usize_from_policy", "fn crypto_allow_algorithms_from_policy", "op_extra_positive_usize"),
        ("fn crypto_allow_algorithms_from_policy", "fn crypto_allow_key_ids_from_policy", "parse_nonempty_string_array"),
        ("fn crypto_allow_key_ids_from_policy", "fn authorized_crypto_allowlist", "parse_nonempty_string_array"),
    ):
        body = crypto_policy.split(start, 1)[1].split(end, 1)[0]
        if "authorized_crypto" not in body or body.find("authorized_crypto") >= body.find(fallback):
            fail(f"crypto enforcement consults raw policy before authority state: {start}")
    for operation in ("hash", "sign", "verify", "kdf", "aead_seal", "aead_open"):
        if crypto_policy.count(f"fn capability_core_crypto_{operation}(") != 1:
            fail(f"crypto operation policy inventory drift: {operation}")
    plugin_policy = (
        root / "crates/gc_effects/src/runner_capability_dispatch/plugin.rs"
    ).read_text()
    for start, end in (
        ("fn plugin_allowlist_from_policy", "fn plugin_command_allowlist_from_policy"),
        ("fn plugin_command_allowlist_from_policy", "fn plugin_schema_allowlist_from_policy"),
        ("fn plugin_schema_allowlist_from_policy", "fn plugin_bridge_digest_pin_is_required"),
    ):
        body = plugin_policy.split(start, 1)[1].split(end, 1)[0]
        if "authorized_plugin" not in body or body.find("authorized_plugin") >= body.find("parse_nonempty_string_array"):
            fail(f"plugin enforcement consults raw policy before authority state: {start}")
    if plugin_policy.count("fn capability_host_plugin_command(") != 1:
        fail("plugin operation policy inventory drift")
    ffi_policy = (
        root / "crates/gc_effects/src/runner_capability_dispatch/ffi_policy.rs"
    ).read_text()
    for start in (
        "pub(super) fn signed_policy_from_authority",
        "pub(super) fn allowlist_from_policy",
        "pub(super) fn schema_allowlist_from_policy",
        "pub(super) fn positive_usize_from_policy",
    ):
        body = ffi_policy.split(start, 1)[1].split("\n}\n", 1)[0]
        if "authorized_ffi" not in body:
            fail(f"ffi enforcement does not consume authority state: {start}")
    ffi_dispatch = (
        root / "crates/gc_effects/src/runner_capability_dispatch/ffi.rs"
    ).read_text()
    for operation in ("call", "buffer_pin", "buffer_unpin"):
        if ffi_dispatch.count(f"fn capability_host_ffi_{operation}(") != 1:
            fail(f"ffi operation policy inventory drift: {operation}")
    for token in (
        "fn is_hex64",
        "policy::required_signed_string",
        "policy::signed_policy_required",
        'extra.get("signed_policy_required")',
        'extra.get("policy_artifact_h")',
        'extra.get("policy_signature_h")',
        'extra.get("policy_key_id")',
        'extra.get("evidence_mode")',
    ):
        if token in ffi_dispatch:
            fail(f"ffi dispatch bypasses signed-policy authority state: {token}")
    bridge_policy = (
        root / "crates/gc_effects/src/runner_host_bridge_policy.rs"
    ).read_text()
    bridge_production = bridge_policy.split("#[cfg(test)]", 1)[0]
    for source, label in (
        (plugin_policy, "plugin"),
        (ffi_dispatch, "ffi"),
        (bridge_production, "host bridge"),
    ):
        for token in (
            'extra.get("bridge_args")',
            'extra.get("bridge_cmd")',
            'extra.get("bridge_cmd_allowlist")',
            'extra.get("bridge_cmd_sha256")',
            'extra.get("bridge_transport")',
            'extra.get("wasi_bridge_profile")',
            "plugin_bridge_digest_pin_is_required",
            "ffi_bridge_digest_pin_is_required",
            "normalize_sha256_hex",
        ):
            if token in source:
                fail(f"{label} bridge identity bypasses authority state: {token}")
    if bridge_production.count("fn bridge_digest_pin_is_missing(") != 1:
        fail("bridge digest-pin preflight authority consumer inventory drift")
    if bridge_production.count("fn bridge_cmd_sha256(") != 1:
        fail("bridge digest enforcement authority consumer inventory drift")
    if bridge_production.count("fn bridge_cmd_allowlist(") != 1:
        fail("bridge command allowlist authority consumer inventory drift")
    for consumer in (
        "fn bridge_profile_active(",
        "fn wasi_bridge_profile_enabled(",
        "fn bridge_cmd(",
        "fn bridge_args(",
        "fn bridge_transport(",
    ):
        if bridge_production.count(consumer) != 1:
            fail(f"bridge invocation authority consumer inventory drift: {consumer}")
    if plugin_policy.count("bridge_digest_pin_is_missing(pol)") != 1:
        fail("plugin bridge digest authority consumer inventory drift")
    if ffi_dispatch.count("bridge_digest_pin_is_missing(pol)") != 1:
        fail("ffi bridge digest authority consumer inventory drift")
    bridge_source = (root / "selfhost/effect_policy_bridge_v1.gc").read_text()
    gpu_source = (root / "selfhost/effect_policy_gpu_v1.gc").read_text()
    gfx_source = (root / "selfhost/effect_policy_gfx_v1.gc").read_text()
    xr_source = (root / "selfhost/effect_policy_xr_v1.gc").read_text()
    for binding in (
        "selfhost/effect-bridge::input-valid?",
        "selfhost/effect-bridge::digest-policy",
        "selfhost/effect-bridge::pin-required?",
        "selfhost/effect-bridge::policy",
    ):
        if bridge_source.count(f"(def {binding}") != 1:
            fail(f"bridge policy authority binding inventory drift: {binding}")
    if bridge_source.count("(def selfhost/effect-bridge::allowlist-policy\n") != 1:
        fail("bridge command allowlist policy binding inventory drift")
    for binding in (
        "active?",
        "args-policy",
        "command-policy",
        "transport-policy",
        "wasi-profile-policy",
    ):
        if bridge_source.count(f"(def selfhost/effect-bridge::{binding}\n") != 1:
            fail(f"bridge invocation policy binding inventory drift: {binding}")
    for binding in ("input-valid?", "backend-policy", "fallback-policy", "policy"):
        if gpu_source.count(f"(def selfhost/effect-gpu::{binding}\n") != 1:
            fail(f"GPU policy binding inventory drift: {binding}")
    for binding in (
        "profile?",
        "input-valid?",
        "selected",
        "normalized-string",
        "production-runtime?",
        "explicit-profile",
        "policy",
    ):
        if gfx_source.count(f"(def selfhost/effect-gfx::{binding}\n") != 1:
            fail(f"GFX policy binding inventory drift: {binding}")
    for binding in (
        "identity-input-valid?",
        "list-input-valid?",
        "bool-input-valid?",
        "limit-input-valid?",
        "input-valid?",
        "selected",
        "normalized-string",
        "production?",
        "backend-decision",
        "production-decision",
        "explicit-decision",
        "positive-decision",
        "bool-decision",
        "list-decision-loop",
        "list-decision",
        "backend-policy",
        "policy",
    ):
        if xr_source.count(f"(def selfhost/effect-xr::{binding}\n") != 1:
            fail(f"XR policy binding inventory drift: {binding}")
    gpu_consumer = (root / "crates/gc_effects/src/runner_gpu_backend_policy.rs").read_text()
    gpu_production = gpu_consumer.split("#[cfg(test)]", 1)[0]
    if gpu_production.count("fn authorized_gpu_policy(") != 1:
        fail("GPU authority consumer inventory drift")
    if ".extra" in gpu_production or "std::env" in gpu_production:
        fail("GPU production selection bypasses authority state")
    if gpu_consumer.count(
        "fn gpu_backend_selection_consumes_authority_before_raw_policy()"
    ) != 1:
        fail("missing focused GPU authority precedence control")
    gfx_consumer = (root / "crates/gc_effects/src/runner_gfx_host/profile.rs").read_text()
    gfx_production = gfx_consumer.split("#[cfg(test)]", 1)[0]
    if gfx_production.count("fn first_party_profile(") != 1:
        fail("GFX profile authority consumer inventory drift")
    if ".extra" in gfx_production or "std::env" in gfx_production:
        fail("GFX production profile selection bypasses authority state")
    if gfx_consumer.count(
        "fn gfx_profile_consumes_authority_before_raw_policy()"
    ) != 1:
        fail("missing focused GFX authority precedence control")
    xr_consumer = (root / "crates/gc_effects/src/runner_xr_host/backend_policy.rs").read_text()
    xr_production = xr_consumer.split("#[cfg(test)]", 1)[0]
    if xr_production.count("fn authorized_policy(") != 1:
        fail("XR device authority consumer inventory drift")
    if ".extra" in xr_production or "std::env" in xr_production:
        fail("XR production policy selection bypasses authority state")
    if xr_consumer.count("fn xr_consumes_authority_before_raw_policy()") != 1:
        fail("missing focused XR device authority precedence control")
    xr_dispatch = "".join(
        (root / relative).read_text().split("#[cfg(test)]", 1)[0]
        for relative in (
            "crates/gc_effects/src/runner_xr_host.rs",
            "crates/gc_effects/src/runner_xr_host/helpers.rs",
            "crates/gc_effects/src/runner_xr_host/advanced.rs",
            "crates/gc_effects/src/runner_xr_host/advanced_layers.rs",
        )
    )
    if ".extra" in xr_dispatch or "toml::" in xr_dispatch:
        fail("XR first-party dispatch bypasses installed authority state")
    for token in (
        "allow_haptics_inputs",
        "max_haptics_amplitude",
        "max_haptics_duration_ms",
        "allow_hand_tracking",
        "max_hand_joints",
        "allow_hit_test",
        "max_hit_results",
        "allow_spatial_mesh",
        "max_meshes",
        "max_mesh_vertices",
        "allow_anchor_spaces",
        "max_anchors",
        "allow_layer_types",
        "max_layers",
        "max_layer_opacity",
    ):
        if f"policy.{token}" not in xr_dispatch:
            fail(f"XR dispatch no longer consumes authorized field: {token}")
    if xr_boundary_path.read_text().count("Term::Vector(vec![") != 1:
        fail("XR authority input is not the exact compact vector transport")
    if "core/vec::get" not in xr_source or "(core/vec::len value)) 18" not in xr_source:
        fail("XR authority source input shape drift")
    for relative in (
        "crates/gc_effects/src/runner_capability_dispatch.rs",
        "crates/gc_effects/src/runner_editor_host.rs",
        "crates/gc_effects/src/runner_browser_host.rs",
        "crates/gc_effects/src/runner_gpu_host.rs",
        "crates/gc_effects/src/runner_gfx_host.rs",
        "crates/gc_effects/src/runner_xr_host.rs",
    ):
        consumer = (root / relative).read_text()
        if consumer.count("fn has_explicit_bridge_profile(") != 1:
            fail(f"bridge activation consumer inventory drift: {relative}")
        body = consumer.split("fn has_explicit_bridge_profile(", 1)[1].split("\n}", 1)[0]
        if body.count("bridge_profile_active(pol)") != 1 or ".extra" in body:
            fail(f"bridge activation bypasses authority state: {relative}")
    for token in (
        "policy.store.remote",
        "policy.store.remote_allow",
        "policy.store.allow_http",
        "policy.store.auth_token",
        "policy.store.auth_token_env",
        "policy.store.basic_username",
        "policy.store.basic_password",
        "policy.store.basic_password_env",
        "policy.store.mtls_ca_pem",
        "policy.store.mtls_identity_pem",
    ):
        if token in remote_production:
            fail(f"global store dispatch bypasses authority state: {token}")
    if remote_production.count("fn store_remote_from_policy(") != 1:
        fail("store remote target authority consumer inventory drift")
    if remote_production.count("fn store_registry_auth(") != 1:
        fail("store credential authority consumer inventory drift")
    if remote_production.count("fn registry_auth_from_authority(") != 1:
        fail("shared credential authority consumer inventory drift")
    store_auth = remote_production.split("fn store_registry_auth(", 1)[1].split(
        "pub(super) fn sync_registry_auth", 1
    )[0]
    if "authorized_store_credentials()" not in store_auth or "policy.store." in store_auth:
        fail("store credential consumer bypasses installed authority state")
    focused_tests = remote_policy + (root / "crates/gc_effects/src/policy_tests.rs").read_text()
    for name in (
        "sync_auth_consumes_authority_before_poisoned_raw_fields",
        "sync_auth_fails_closed_without_or_with_rejected_authority",
        "selfhost_authority_owns_per_operation_sync_credentials",
        "selfhost_authority_denies_operation_without_credential_state",
        "operation_credential_decoder_rejects_substitution_and_open_results",
    ):
        if focused_tests.count(f"fn {name}()") != 1:
            fail(f"missing focused sync credential authority control: {name}")
    store_resource = (
        root / "selfhost/effect_policy_resource_authority_v1.gc"
    ).read_text()
    for binding in (
        "selfhost/effect-store-remote::input-valid?",
        "selfhost/effect-store-remote::policy",
        "selfhost/effect-store-credential::input-valid?",
        "selfhost/effect-store-credential::policy",
        "core/effects::resource-policy-authority",
    ):
        if store_resource.count(f"(def {binding}") != 1:
            fail(f"store remote authority binding inventory drift: {binding}")
    effect_source = (root / profile["sourceModule"]).read_text()
    cap_body = effect_source.split("(def selfhost/effect-policy::cap", 1)[1].split(
        "(def core/effects::policy-authority", 1
    )[0]
    if ":max-bytes" in cap_body:
        fail("private max-byte policy leaked into the logged capability descriptor")

    driver = (root / "crates/gc_cli_driver/src/lib.rs").read_text()
    if driver.count("CapsPolicy::load(path)") != 1 or driver.count("load_with_selfhost_authority(") != 1:
        fail("CLI policy authority loader inventory drift")
    if '#[cfg(feature = "parity-harness")]\n        gc_obligations::CoreformFrontend::Rust => CapsPolicy::load(path)' not in driver:
        fail("CLI Rust compatibility loader is not compile-time gated")
    if 'Rust effect-policy authority is not compiled into production' not in driver:
        fail("CLI production Rust route does not fail closed")

    call_sites = 0
    direct_loads = []
    for path in source_files(root):
        text = path.read_text()
        call_sites += text.count("load_caps_policy(cli,")
        if "CapsPolicy::load(" in text:
            direct_loads.append(path.relative_to(root).as_posix())
    if call_sites != 10:
        fail(f"production self-host policy call-site inventory drift: {call_sites}")
    if sorted(direct_loads) != [
        "crates/gc_cli_driver/src/lib.rs",
        "crates/gc_obligations/src/obligation_authority_preflight.rs",
    ]:
        fail(f"unexpected production direct policy loader: {direct_loads}")

    preflight = (root / "crates/gc_obligations/src/obligation_authority_preflight.rs").read_text()
    if preflight.count("load_with_selfhost_authority(") != 1 or "CoreformFrontend::Rust => CapsPolicy::load(path)" not in preflight:
        fail("preflight effect-policy authority routing drift")
    task_profile = (root / "crates/gc_cli_driver/src/pkg_runtime_profile.rs").read_text()
    if (
        task_profile.count("CapsPolicy::from_toml_str_with_selfhost_authority(") != 1
        or task_profile.count('#[cfg(feature = "parity-harness")]') != 1
        or "Rust effect-policy authority is not compiled into production" not in task_profile
    ):
        fail("runtime-profile probe does not use the selected self-host policy authority")

    runtime_budget = (root / "crates/gc_effects/src/runner_runtime_budget.rs").read_text()
    for field in (
        "max_effect_ops",
        "max_payload_bytes_per_op",
        "max_payload_bytes_per_run",
        "max_response_bytes_per_op",
        "max_response_bytes_per_run",
    ):
        if f"policy.runtime.{field}" not in runtime_budget:
            fail(f"runtime enforcement no longer consumes authorized field: {field}")
    task_enforcement = (
        (root / "crates/gc_effects/src/runner_task_policy.rs").read_text()
        + (root / "crates/gc_effects/src/runner_task.rs").read_text()
    )
    for field in (
        "default_workers",
        "max_tasks",
        "max_workers",
        "max_queue",
        "max_steps_per_task",
        "max_time_ms_per_task",
    ):
        if f"policy.task.{field}" not in task_enforcement:
            fail(f"task enforcement no longer consumes authorized field: {field}")
    response_budget = (root / "crates/gc_effects/src/runner_response_budget.rs").read_text()
    for token in (
        "policy.inline_max_bytes_for(op)",
        "policy.log.max_artifact_bytes_per_run",
        "policy.store.max_run_bytes",
    ):
        if token not in response_budget:
            fail(f"global resource enforcement no longer consumes authorized field: {token}")

    tests = (root / "crates/gc_effects/src/policy_tests.rs").read_text()
    for name in (
        "selfhost_authority_composes_admission_and_canonical_caps",
        "selfhost_authority_owns_per_operation_base_directory",
        "selfhost_authority_discards_denied_operation_base_directory",
        "selfhost_authority_installs_normalized_operation_controls",
        "selfhost_authority_rejects_noncanonical_operation_controls",
        "selfhost_authority_owns_sorted_unique_candidate_inventory",
        "selfhost_authority_owns_runtime_and_task_resource_composition",
        "selfhost_authority_owns_adaptive_task_worker_default",
        "selfhost_authority_owns_global_log_and_store_resource_limits",
        "selfhost_authority_normalizes_nonpositive_global_resource_limits",
        "selfhost_authority_owns_default_global_storage_locations",
        "selfhost_authority_preserves_explicit_global_storage_locations",
        "selfhost_authority_rejects_unbounded_operation_inventories_before_evaluation",
        "selfhost_authority_installs_valid_and_absent_max_byte_controls",
        "selfhost_authority_preserves_invalid_max_byte_effect_errors",
        "selfhost_authority_rejects_malformed_max_byte_decisions",
        "selfhost_authority_installs_process_program_policy",
        "selfhost_authority_preserves_invalid_process_program_states",
        "selfhost_authority_rejects_malformed_process_program_decisions",
        "selfhost_authority_installs_database_policy",
        "selfhost_authority_preserves_invalid_database_policy_states",
        "selfhost_authority_rejects_malformed_database_decisions",
        "selfhost_authority_installs_network_policy",
        "selfhost_authority_preserves_invalid_network_policy_states",
        "selfhost_authority_rejects_malformed_network_decisions",
        "selfhost_authority_installs_crypto_policy",
        "selfhost_authority_preserves_invalid_crypto_policy_states",
        "selfhost_authority_rejects_malformed_crypto_decisions",
        "selfhost_authority_installs_plugin_policy",
        "selfhost_authority_preserves_invalid_plugin_policy_states",
        "selfhost_authority_rejects_malformed_plugin_decisions",
        "selfhost_authority_installs_ffi_policy",
        "selfhost_authority_preserves_invalid_ffi_policy_states",
        "selfhost_authority_rejects_malformed_ffi_decisions",
        "selfhost_authority_installs_bridge_digest_pin_policy",
        "selfhost_authority_preserves_bridge_digest_states_and_wasi_precedence",
        "selfhost_authority_rejects_malformed_bridge_digest_decisions",
        "selfhost_authority_normalizes_bridge_allowlist_without_changing_empty_semantics",
        "selfhost_authority_rejects_malformed_bridge_allowlist_decisions",
        "selfhost_authority_owns_bridge_invocation_configuration",
        "selfhost_authority_preserves_bridge_invocation_compatibility_defaults",
        "selfhost_authority_rejects_malformed_bridge_invocation_decisions",
        "selfhost_authority_installs_gfx_profile_with_exact_legacy_precedence",
        "selfhost_authority_rejects_malformed_gfx_profile_decisions",
        "selfhost_authority_installs_xr_backend_with_exact_legacy_precedence",
        "selfhost_authority_rejects_malformed_xr_backend_decisions",
        "selfhost_authority_installs_all_xr_device_policy_states",
        "selfhost_authority_rejects_malformed_xr_device_decisions",
        "selfhost_authority_owns_global_store_credential_and_tls_selection",
        "selfhost_authority_selects_basic_environment_credentials",
        "selfhost_authority_preserves_store_credential_errors_for_effect_use",
        "store_credential_decoder_rejects_open_contradictory_and_substituted_results",
    ):
        if tests.count(f"fn {name}()") != 1:
            fail(f"missing focused authority control: {name}")
    for source, names in (
        (
            authority_root,
            (
                "inventory_decoder_allows_authoritative_denial_by_omission",
                "inventory_decoder_rejects_invented_operations",
            ),
        ),
        (
            transport,
            (
                "candidate_transport_keeps_denied_overrides_and_opaque_host_fields",
                "transport_rejects_structure_but_does_not_apply_allow_precedence",
            ),
        ),
    ):
        for name in names:
            if source.count(f"fn {name}()") != 1:
                fail(f"missing no-oracle transport control: {name}")
    for name in (
        "process_dispatch_consumes_authorized_programs_before_raw_policy",
        "process_dispatch_preserves_authorized_policy_errors",
    ):
        if process_policy.count(f"fn {name}()") != 1:
            fail(f"missing focused process authority control: {name}")
    for name in (
        "database_dispatch_consumes_authorized_policy_before_raw_policy",
        "database_dispatch_preserves_authorized_policy_errors",
    ):
        if database_policy.count(f"fn {name}()") != 1:
            fail(f"missing focused database authority control: {name}")
    for name in (
        "network_dispatch_consumes_authorized_policy_before_raw_policy",
        "network_dispatch_preserves_authorized_policy_errors",
    ):
        if network_policy.count(f"fn {name}()") != 1:
            fail(f"missing focused network authority control: {name}")
    for name in (
        "remote_dispatch_consumes_authorized_network_policy_before_raw_policy",
        "remote_dispatch_preserves_authorized_network_policy_errors",
        "store_auth_consumes_authorized_credentials_before_poisoned_raw_fields",
        "store_auth_fails_closed_without_credential_authority",
    ):
        if remote_policy.count(f"fn {name}()") != 1:
            fail(f"missing focused remote network authority control: {name}")
    for name in (
        "crypto_dispatch_consumes_authorized_policy_before_raw_policy",
        "crypto_dispatch_preserves_authorized_policy_errors",
    ):
        if crypto_policy.count(f"fn {name}()") != 1:
            fail(f"missing focused crypto authority control: {name}")
    for name in (
        "plugin_dispatch_consumes_authorized_policy_before_raw_policy",
        "plugin_dispatch_preserves_authorized_policy_errors_and_optional_schema",
    ):
        if plugin_policy.count(f"fn {name}()") != 1:
            fail(f"missing focused plugin authority control: {name}")
    for name in (
        "ffi_dispatch_consumes_authorized_policy_before_raw_policy",
        "ffi_dispatch_preserves_authorized_policy_errors_and_optional_schema",
    ):
        if ffi_policy.count(f"fn {name}()") != 1:
            fail(f"missing focused ffi authority control: {name}")
    if bridge_policy.count(
        "fn bridge_identity_enforcement_consumes_authority_before_raw_policy()"
    ) != 1:
        fail("missing focused bridge identity authority precedence control")
    if bridge_policy.count(
        "fn bridge_allowlist_enforcement_consumes_authority_before_raw_policy()"
    ) != 1:
        fail("missing focused bridge allowlist authority precedence control")
    if bridge_policy.count(
        "fn bridge_invocation_consumes_authority_before_raw_policy()"
    ) != 1:
        fail("missing focused bridge invocation authority precedence control")

    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-EFFECT-POLICY"]
    if (
        len(rows) != 1
        or rows[0].get("currentLevel") != "H2"
        or rows[0].get("fallbackReachability") != "none-proven"
        or "scripts/lib/selfhost_effect_policy_composition.py"
        not in rows[0].get("verifierPaths", [])
    ):
        fail("effect-policy H2 ledger authority drift")
    return {
        "callSites": call_sites,
        "decisions": len(DECISIONS),
        "residualDecisions": len(RESIDUALS),
        "sourceSha256": source_hash,
    }


def mutation_controls(profile, schema):
    edits = [
        ("binding", lambda item: item.__setitem__("binding", "core/cli::policy-authority")),
        ("inventory-binding", lambda item: item.__setitem__("inventoryBinding", "core/cli::policy-authority")),
        ("resource-binding", lambda item: item.__setitem__("resourceBinding", "core/cli::policy-authority")),
        ("resource-request", lambda item: item.__setitem__("resourceRequestKind", "unknown")),
        ("resource-result", lambda item: item.__setitem__("resourceResultKind", "unknown")),
        ("decision", lambda item: item["decisionInventory"].pop()),
        ("oracle", lambda item: item["hostOracle"].__setitem__("required", True)),
        ("limit", lambda item: item.__setitem__("maxPolicyOperations", 0)),
        ("nonclaim", lambda item: item["nonclaims"].pop()),
        ("residual", lambda item: item["residualDecisionInventory"].pop()),
        ("request", lambda item: item.__setitem__("requestKind", "unknown")),
        ("result", lambda item: item.__setitem__("resultKind", "unknown")),
        ("runtime", lambda item: item["runtimeEvidence"].__setitem__("stepLimit", 0)),
        ("source", lambda item: item.__setitem__("sourceModule", "selfhost/unknown.gc")),
        ("crypto-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_crypto_v1.gc")),
        ("plugin-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_plugin_v1.gc")),
        ("ffi-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_ffi_v1.gc")),
        ("bridge-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_bridge_v1.gc")),
        ("gpu-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_gpu_v1.gc")),
        ("gfx-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_gfx_v1.gc")),
        ("xr-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_xr_v1.gc")),
        ("resource-source", lambda item: item["sourceModules"].remove("selfhost/effect_policy_resource_authority_v1.gc")),
        ("source-order", lambda item: item["sourceModules"].reverse()),
        ("unknown", lambda item: item.__setitem__("unexpected", True)),
    ]
    rejected = 0
    for label, edit in edits:
        candidate = copy.deepcopy(profile)
        edit(candidate)
        candidate["contentIdentitySha256"] = identity(candidate)
        try:
            validate(candidate, schema)
        except CheckError:
            rejected += 1
            continue
        fail(f"self-test accepted authority mutation: {label}")
    stale = copy.deepcopy(profile)
    stale["auditDate"] = "2026-08-14"
    try:
        validate(stale, schema)
    except CheckError:
        return rejected + 1
    fail("self-test accepted stale profile identity")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--refresh-identity", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    profile_path = root / args.profile
    profile = load_json(profile_path)
    schema = load_json(root / args.schema)
    if args.refresh_identity:
        profile["sourceSha256"] = source_identity(root, profile["sourceModules"])
        profile["contentIdentitySha256"] = identity(profile)
        profile_path.write_text(json.dumps(profile, indent=2) + "\n")
        print(f"selfhost-effect-policy-composition: refreshed {args.profile}")
        return
    validate(profile, schema)
    static = static_check(root, profile)
    controls = mutation_controls(profile, schema) if args.self_test else 0
    print(json.dumps({
        "kind": "genesis/selfhost-effect-policy-composition-check-v0.1",
        "mutationControls": controls,
        "ok": True,
        "profileIdentitySha256": identity(profile),
        "static": static,
    }, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    try:
        main()
    except CheckError as error:
        print(f"selfhost-effect-policy-composition: {error}", file=sys.stderr)
        raise SystemExit(1)
