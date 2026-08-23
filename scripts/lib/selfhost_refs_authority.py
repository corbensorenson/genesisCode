#!/usr/bin/env python3
"""Independent custody verifier for the partial self-hosted refs authority."""

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
    "policyBinding", "policyRequestKind", "policyResultKind", "policySourceModule",
    "policySourceSha256", "productionEntrypoints", "requestKind", "resultKind", "schema",
    "sourceModule", "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/refs::authority",
    "decisionInventory": [
        "direct-ref-lookup", "direct-ref-prefix-filter-and-order",
        "direct-ref-cas-conflict-verdict", "direct-ref-update-and-delete-transition",
        "direct-ref-response-construction", "bulk-ref-mode-and-input-admission",
        "bulk-ref-canonical-order-and-uniqueness", "bulk-ref-first-conflict-verdict",
        "bulk-ref-atomic-transition", "internal-consumer-ref-read-routing",
        "local-ref-policy-and-class-admission",
        "local-ref-obligation-evidence-and-assurance-admission",
        "local-ref-signature-threshold-role-and-independence-admission",
        "local-ref-policy-request-bound-verdict",
        "request-bound-result-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "bounded-artifact-observation-and-hash-verification",
        "ed25519-signature-verification-mechanism",
        "refs-db-locking-and-atomic-persistence", "optimistic-snapshot-retry",
        "result-contradiction-checking",
        "effect-log-and-diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_refs_authority.py",
    "kind": "genesis/selfhost-refs-authority-v0.1",
    "policyBinding": "core/refs::policy-authority",
    "policyRequestKind": "genesis/refs-policy-authority-request-v0.1",
    "policyResultKind": "genesis/refs-policy-authority-result-v0.1",
    "policySourceModule": "selfhost/pkg_publish_authority_v1.gc",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/refs-authority-request-v0.1",
    "resultKind": "genesis/refs-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_REFS_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/refs_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_REFS_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "all-internal-ref-consumer-authority", "bootstrap-fixpoint",
    "gpk-sync-policy-and-transport-authority", "h2-sd-refs",
    "r4-2-e-closure", "registry-ref-authority", "release-qualification", "sh-c-closure",
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


def validate_profile(profile, schema, check_identity: bool = True) -> None:
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
    for name in ("contentIdentitySha256", "policySourceSha256", "sourceSha256"):
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
    policy_module = source_text(root, profile["policySourceModule"], overrides)
    manifest = source_text(root, "selfhost/toolchain_manifest.gc", overrides)
    authority = source_text(root, "crates/gc_effects/src/refs_authority.rs", overrides)
    policy_adapter = source_text(
        root, "crates/gc_effects/src/refs_policy_authority.rs", overrides
    )
    policy_adapter_tests = source_text(
        root, "crates/gc_effects/src/refs_policy_authority_tests.rs", overrides
    )
    refs_ops = source_text(root, "crates/gc_effects/src/runner_refs_ops.rs", overrides)
    bulk = source_text(root, "crates/gc_effects/src/refs_authority_bulk.rs", overrides)
    refs_db = source_text(root, "crates/gc_effects/src/refs.rs", overrides)
    cap_refs = source_text(root, "crates/gc_effects/src/runner_cap_refs.rs", overrides)
    gpk = source_text(
        root, "crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs", overrides
    )
    gpk_root = source_text(
        root, "crates/gc_effects/src/runner_remote_ops/gpk.rs", overrides
    )
    publish = source_text(
        root,
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/publish_authority.rs",
        overrides,
    )
    resolution = source_text(
        root, "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs", overrides
    )
    vcs_meta = source_text(
        root, "crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs", overrides
    )
    vcs_history = source_text(
        root, "crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs", overrides
    )
    sync = source_text(
        root, "crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs", overrides
    )
    dispatch = source_text(
        root, "crates/gc_effects/src/runner_capability_dispatch.rs", overrides
    )
    sync_tests = source_text(
        root, "crates/gc_effects/tests/sync_registry/cases_b.rs", overrides
    )
    runner = source_text(root, "crates/gc_effects/src/runner.rs", overrides)

    required_module = [
        "(def core/refs::authority", profile["requestKind"], profile["resultKind"],
        "selfhost/refs::list-loop", "selfhost/refs::expected-matches?",
        "selfhost/refs::remove-loop", "selfhost/refs::set-many",
        "selfhost/refs::bulk-conflict-loop", ":same-or-absent", ":unconditional",
        "bulk ref update exceeds 4096 operations", "selfhost/hash::hash-term request",
    ]
    for marker in required_module:
        if marker not in module:
            fail(f"GenesisCode refs authority missing marker: {marker}")
    required_policy_module = [
        f"(def {profile['policyBinding']}", profile["policyRequestKind"],
        profile["policyResultKind"], "selfhost/refs-policy::selection-code",
        "selfhost/refs-policy::delete-facts?", "core/pkg::publish-authority",
        "(quote :inspect)", "(quote :prepare)", "(quote :finalize)",
        "selfhost/hash::hash-term request",
    ]
    for marker in required_policy_module:
        if marker not in policy_module:
            fail(f"GenesisCode refs policy authority missing marker: {marker}")
    for path in (profile["sourceModule"], profile["policySourceModule"]):
        if f'    "{path}"' not in manifest:
            fail(f"toolchain manifest does not custody refs authority module: {path}")
    for binding in (profile["binding"], profile["policyBinding"]):
        if binding not in manifest:
            fail(f"toolchain manifest does not custody refs authority binding: {binding}")
    for marker in (
        "pub(crate) struct RefsAuthority", "const MAX_RETRIES: usize = 16",
        "decode_get", "decode_list", "decode_set", "request-h",
        "pub(crate) fn required_for_request(",
        "pub(crate) fn consumer_get(", "pub(crate) fn consumer_list(",
        '#[cfg(feature = "parity-oracle")]',
        "local ref lookup requires the artifact-loaded GenesisCode refs authority",
        "local ref listing requires the artifact-loaded GenesisCode refs authority",
    ):
        if marker not in authority:
            fail(f"Rust refs authority adapter missing marker: {marker}")
    if '#[cfg(any(test, feature = "parity-oracle"))]' in authority:
        fail("consumer ref fallback is reachable from generic test builds")
    for marker in (
        "pub(crate) fn validate_policy_gate(", "POLICY_STEP_LIMIT: u64 = 50_000_000",
        "MAX_OBJECT_BYTES: usize = 4 * 1024 * 1024",
        "MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024", "MAX_OBJECTS: usize = 4096",
        "observe_bytes_limited", "mechanical_signing_hash", "verify_crypto_request",
        "decode_phase_result", "decode_inspection", "decode_preparation",
        "decode_admission", "require_embedded_hash",
    ):
        if marker not in policy_adapter:
            fail(f"Rust refs policy adapter missing marker: {marker}")
    for marker in (
        "artifact_loader_rejects_content_hash_substitution",
        "result_decoder_rejects_request_substitution_and_open_fields",
        "admission_decoder_rejects_identity_substitution_and_open_fields",
        "hash_inventory_decoder_enforces_shape_and_object_bound",
    ):
        if marker not in policy_adapter_tests:
            fail(f"Rust refs policy adapter tests missing marker: {marker}")
    for forbidden in (
        "gc_vcs::Policy", "gc_vcs::Commit", "gc_vcs::Evidence", "gc_vcs::Attestation",
        "gc_vcs::commit_signing_hash", "gc_vcs::verify_commit_attestation",
    ):
        if forbidden in refs_ops:
            fail(f"production refs gate retains native semantic authority: {forbidden}")
    for marker in (
        "const MAX_BULK_OPS: usize = 4096", "pub(crate) fn set_many(",
        'self.evaluate(":set-many", payload)', "decode_bulk_set",
        "bulk conflict attribution contradiction", "bulk replacement snapshot contradiction",
        "bulk ref inputs must be strictly sorted and unique",
    ):
        if marker not in bulk:
            fail(f"Rust bulk refs authority adapter missing marker: {marker}")
    for marker in ("pub(crate) fn snapshot", "pub(crate) fn replace_if_unchanged"):
        if marker not in refs_db:
            fail(f"refs persistence mechanism missing marker: {marker}")
    if not re.search(
        r'#\[cfg\(any\(test, feature = "parity-oracle"\)\)\]\s+pub fn set_many',
        refs_db,
    ):
        fail("native bulk refs oracle is not compile-isolated")
    for marker in ("authority.get(refs, &name)", "authority.list(refs, prefix.as_deref())", "authority.set("):
        if marker not in cap_refs:
            fail(f"production refs route missing authority call: {marker}")
    if len(re.findall(
        r"local_refs_validate_policy_gate\(\s*authority,\s*store,",
        cap_refs,
    )) != 2:
        fail("direct refs mutation routes do not pass the GenesisCode policy authority")
    for forbidden in ("let h = refs.get(&name)?", "let xs = refs.list(prefix.as_deref())?"):
        if forbidden in cap_refs:
            fail(f"production refs route retains direct semantic fallback: {forbidden}")
    for marker in (
        "local_refs_validate_policy_gate(\n                refs_authority,",
        "refs_authority.set_many(refs_db, &ops, BulkSetMode::CompareAndSet)",
        "BulkSetResult::Conflict { name, current }",
    ):
        if marker not in gpk:
            fail(f"GPK bulk ref route missing authority marker: {marker}")
    if "refs_db.set_many(" in gpk:
        fail("GPK production route retains native bulk mutation fallback")
    for marker in (
        "pending_refs.sort_by", "BulkSetMode::SameOrAbsent",
        "BulkSetMode::Unconditional", "authority.set_many(refs, &pending_refs, mode)",
        "core/sync::push set-refs requires the artifact-loaded GenesisCode refs authority",
        "local_refs_validate_policy_gate(\n                refs_authority,",
    ):
        if marker not in sync:
            fail(f"sync bulk ref route missing authority marker: {marker}")
    for forbidden in ("refs.set(rname", "let cur = refs.get(rname)"):
        if forbidden in sync:
            fail(f"sync production route retains per-ref mutation decision: {forbidden}")
    if "sync_pull_ref_conflict_leaves_the_entire_batch_unchanged" not in sync_tests:
        fail("sync atomic negative control missing")
    if not re.search(
        r'"core/sync::push"\s*=>\s*capability_sync_push\(.*?refs_authority,',
        dispatch,
        re.DOTALL,
    ):
        fail("sync push dispatch does not forward the GenesisCode refs authority")
    consumer_routes = {
        "GPK root": (gpk_root, "RefsAuthority::consumer_get(refs_authority, refs, &root)"),
        "GPK embedded refs": (gpk, "RefsAuthority::consumer_get(ctx.refs_authority.as_deref_mut(), refs, name)"),
        "package publish": (publish, "RefsAuthority::consumer_get(refs_authority.as_deref_mut(), refs, &refname)"),
        "package ref resolution": (resolution, "RefsAuthority::consumer_get(refs_authority.as_deref_mut(), refs, &rn)"),
        "package semver resolution": (resolution, "RefsAuthority::consumer_list("),
        "VCS root": (vcs_meta, "RefsAuthority::consumer_get("),
        "VCS history": (vcs_history, "RefsAuthority::consumer_list(refs_authority, refs, None)"),
    }
    for label, (source, marker) in consumer_routes.items():
        if marker not in source:
            fail(f"{label} does not route through GenesisCode refs authority")
    for label, source in {
        "GPK root": gpk_root,
        "GPK export": gpk,
        "package publish": publish,
        "package resolution": resolution,
        "VCS meta": vcs_meta,
        "VCS history": vcs_history,
    }.items():
        if re.search(r"\b(?:refs|rdb)\s*\.\s*(?:get|list)\s*\(", source):
            fail(f"{label} retains a direct local refs read")
    if "gpk_ref_export_fails_closed_without_authority_and_succeeds_with_it" not in sync_tests:
        fail("GPK internal-consumer authority control missing")
    if "refs_authority.as_deref_mut(),\n        None," not in publish:
        fail("package publish recursive sync push omits the GenesisCode refs authority")
    required_ops = (
        "core/refs::get", "core/refs::list", "core/refs::set", "core/refs::delete",
        "core/sync::pull", "core/sync::push", "core/gpk-low::export", "core/gpk-low::import",
        "core/pkg-low::publish", "core/pkg-low::lock", "core/pkg-low::update",
        "core/pkg-low::install", "core/vcs-low::log", "core/vcs-low::blame",
        "core/vcs-low::why",
    )
    inventory_match = re.search(
        r"pub\(crate\) fn required_for_request\(.*?\n\s*}\n\n\s*pub\(crate\) fn load",
        authority,
        re.DOTALL,
    )
    if inventory_match is None:
        fail("refs authority lazy-load operation inventory is not structurally bounded")
    inventory = tuple(re.findall(r'"([^"]+)"', inventory_match.group(0)))
    if inventory != required_ops:
        fail("refs authority lazy-load operation inventory drift")
    if (
        ".map(RefsAuthority::load)" not in runner
        or "refs_authority.as_mut()" not in runner
        or "RefsAuthority::required_for_request(&req.op)" not in runner
    ):
        fail("runner does not load and forward artifact refs authority")

    source_bytes = module.encode()
    if source_identity(profile["sourceModule"], source_bytes) != profile["sourceSha256"]:
        fail("refs authority source identity mismatch")
    policy_source_bytes = policy_module.encode()
    if source_identity(profile["policySourceModule"], policy_source_bytes) != profile["policySourceSha256"]:
        fail("refs policy authority source identity mismatch")


def validate_all(root: Path, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity=check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    module = (root / profile["sourceModule"]).read_text()
    policy_module = (root / profile["policySourceModule"]).read_text()
    manifest = (root / "selfhost/toolchain_manifest.gc").read_text()
    authority = (root / "crates/gc_effects/src/refs_authority.rs").read_text()
    policy_adapter = (root / "crates/gc_effects/src/refs_policy_authority.rs").read_text()
    refs_ops = (root / "crates/gc_effects/src/runner_refs_ops.rs").read_text()
    cap_refs = (root / "crates/gc_effects/src/runner_cap_refs.rs").read_text()
    bulk = (root / "crates/gc_effects/src/refs_authority_bulk.rs").read_text()
    refs_db = (root / "crates/gc_effects/src/refs.rs").read_text()
    gpk = (root / "crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs").read_text()
    gpk_root = (root / "crates/gc_effects/src/runner_remote_ops/gpk.rs").read_text()
    publish = (root / "crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/publish_authority.rs").read_text()
    resolution = (root / "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs").read_text()
    vcs_meta = (root / "crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs").read_text()
    vcs_history = (root / "crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs").read_text()
    sync = (root / "crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs").read_text()
    dispatch = (root / "crates/gc_effects/src/runner_capability_dispatch.rs").read_text()
    sync_tests = (root / "crates/gc_effects/tests/sync_registry/cases_b.rs").read_text()
    runner = (root / "crates/gc_effects/src/runner.rs").read_text()
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    profile_mutation("binding", "core/refs::legacy")
    profile_mutation("policyBinding", "core/refs::legacy-policy")
    profile_mutation("policyRequestKind", "genesis/refs-policy-authority-request-v0.0")
    profile_mutation("policyResultKind", "genesis/refs-policy-authority-result-v0.0")
    profile_mutation("policySourceModule", "selfhost/refs_policy_legacy.gc")
    profile_mutation("policySourceSha256", "e" * 64)
    profile_mutation("decisionInventory", profile["decisionInventory"][:-1])
    profile_mutation("hostMechanisms", profile["hostMechanisms"][:-1])
    profile_mutation("productionEntrypoints", ["genesis"])
    profile_mutation("nonclaims", profile["nonclaims"][:-1])
    profile_mutation("sourceSha256", "f" * 64)
    open_profile = copy.deepcopy(profile)
    open_profile["extra"] = True
    mutations.append((open_profile, {}, "profile closure"))
    mutations.extend([
        (profile, {profile["sourceModule"]: module.replace("(def core/refs::authority", "(def core/refs::legacy", 1)}, "source binding"),
        (profile, {profile["policySourceModule"]: policy_module.replace("(def core/refs::policy-authority", "(def core/refs::legacy-policy-authority", 1)}, "policy source binding"),
        (profile, {profile["policySourceModule"]: policy_module.replace("(core/pkg::publish-authority (selfhost/refs-policy::package-request request))", "(core/pkg::legacy-publish-authority (selfhost/refs-policy::package-request request))", 1)}, "shared policy delegation"),
        (profile, {profile["sourceModule"]: module.replace("(def selfhost/refs::set-many", "(def selfhost/refs::legacy-set-many", 1)}, "bulk source"),
        (profile, {"selfhost/toolchain_manifest.gc": manifest.replace(f'    "{profile["sourceModule"]}"\n', "", 1)}, "module custody"),
        (profile, {"selfhost/toolchain_manifest.gc": manifest.replace(f"    {profile['binding']}\n", "", 1)}, "binding custody"),
        (profile, {"selfhost/toolchain_manifest.gc": manifest.replace(f"    {profile['policyBinding']}\n", "", 1)}, "policy binding custody"),
        (profile, {"crates/gc_effects/src/refs_policy_authority.rs": policy_adapter.replace("pub(crate) fn validate_policy_gate(", "pub(crate) fn legacy_validate_policy_gate(", 1)}, "policy adapter"),
        (profile, {"crates/gc_effects/src/runner_refs_ops.rs": refs_ops + "\n// gc_vcs::Policy\n"}, "native policy authority"),
        (profile, {"crates/gc_effects/src/runner_cap_refs.rs": cap_refs.replace("authority.get(refs, &name)", "refs.get(&name)", 1)}, "lookup route"),
        (profile, {"crates/gc_effects/src/runner_cap_refs.rs": cap_refs.replace("authority,\n        store,", "store,\n        store,", 1)}, "direct policy route"),
        (profile, {"crates/gc_effects/src/refs_authority_bulk.rs": bulk.replace("pub(crate) fn set_many(", "pub(crate) fn legacy_set_many(", 1)}, "bulk adapter"),
        (profile, {"crates/gc_effects/src/refs_authority_bulk.rs": bulk.replace("bulk conflict attribution contradiction", "bulk conflict accepted", 1)}, "conflict binding"),
        (profile, {"crates/gc_effects/src/refs.rs": refs_db.replace("pub(crate) fn replace_if_unchanged", "pub(crate) fn replace_without_check", 1)}, "atomic adapter"),
        (profile, {"crates/gc_effects/src/refs.rs": refs_db.replace('#[cfg(any(test, feature = "parity-oracle"))]\n    pub fn set_many', "    pub fn set_many", 1)}, "native oracle isolation"),
        (profile, {"crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs": gpk.replace("refs_authority.set_many(refs_db, &ops, BulkSetMode::CompareAndSet)", "refs_db.set_many(&ops)", 1)}, "GPK authority route"),
        (profile, {"crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs": gpk.replace("local_refs_validate_policy_gate(\n                refs_authority,", "local_refs_validate_policy_gate(\n                store,", 1)}, "GPK policy route"),
        (profile, {"crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs": sync.replace("authority.set_many(refs, &pending_refs, mode)", "refs.set(rname, Some(&h), None)", 1)}, "sync authority route"),
        (profile, {"crates/gc_effects/src/runner_capability_dispatch.rs": dispatch.replace('"core/sync::push" => capability_sync_push(', '"core/sync::push" => capability_sync_push_without_authority(', 1)}, "sync push policy dispatch"),
        (profile, {"crates/gc_effects/tests/sync_registry/cases_b.rs": sync_tests.replace("sync_pull_ref_conflict_leaves_the_entire_batch_unchanged", "sync_pull_partial_update_allowed", 1)}, "sync atomic control"),
        (profile, {"crates/gc_effects/src/runner.rs": runner.replace(".map(RefsAuthority::load)", ".map(StoreAuthority::load)", 1)}, "runner load"),
        (profile, {"crates/gc_effects/src/refs_authority.rs": authority.replace('"core/vcs-low::why"', '"core/vcs-low::legacy-why"', 1)}, "lazy-load inventory"),
        (profile, {"crates/gc_effects/src/refs_authority.rs": authority.replace("pub(crate) fn consumer_get(", "pub(crate) fn legacy_consumer_get(", 1)}, "consumer adapter"),
        (profile, {"crates/gc_effects/src/refs_authority.rs": authority.replace('#[cfg(feature = "parity-oracle")]', '#[cfg(any(test, feature = "parity-oracle"))]', 1)}, "generic test fallback"),
        (profile, {"crates/gc_effects/src/runner_remote_ops/gpk.rs": gpk_root.replace("RefsAuthority::consumer_get(refs_authority, refs, &root)", "refs.get(&root)", 1)}, "GPK root read"),
        (profile, {"crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs": gpk.replace("RefsAuthority::consumer_get(ctx.refs_authority.as_deref_mut(), refs, name)", "refs.get(name)", 1)}, "GPK embedded read"),
        (profile, {"crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/publish_authority.rs": publish.replace("RefsAuthority::consumer_get(refs_authority.as_deref_mut(), refs, &refname)", "refs.get(&refname)", 1)}, "publish read"),
        (profile, {"crates/gc_effects/src/runner_cap_pkg_low/dispatch_publish/publish_authority.rs": publish.replace("refs_authority.as_deref_mut(),\n        None,", "None,\n        None,", 1)}, "publish recursive policy route"),
        (profile, {"crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs": resolution.replace("RefsAuthority::consumer_get(refs_authority.as_deref_mut(), refs, &rn)", "refs.get(&rn)", 1)}, "package read"),
        (profile, {"crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs": vcs_meta.replace("RefsAuthority::consumer_get(", "RefsAuthority::legacy_consumer_get(", 1)}, "VCS root read"),
        (profile, {"crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs": vcs_history.replace("RefsAuthority::consumer_list(refs_authority, refs, None)", "refs.list(None)", 1)}, "VCS history read"),
        (profile, {"crates/gc_effects/tests/sync_registry/cases_b.rs": sync_tests.replace("gpk_ref_export_fails_closed_without_authority_and_succeeds_with_it", "gpk_ref_export_bypasses_authority", 1)}, "GPK consumer control"),
    ])
    controls = 0
    for candidate, overrides, label in mutations:
        try:
            validate_all(root, candidate, schema, overrides, check_identity=True)
        except CheckError:
            controls += 1
        else:
            fail(f"mutation survived: {label}")
    if controls != 44:
        fail(f"negative control inventory drift: {controls}")
    return controls


def write_identities(path: Path, profile, root: Path) -> None:
    module_path = root / profile["sourceModule"]
    policy_module_path = root / profile["policySourceModule"]
    profile["sourceSha256"] = source_identity(profile["sourceModule"], module_path.read_bytes())
    profile["policySourceSha256"] = source_identity(
        profile["policySourceModule"], policy_module_path.read_bytes()
    )
    profile["contentIdentitySha256"] = canonical_identity(profile)
    path.write_text(json.dumps(profile, indent=2) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write-identities", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    profile_path = args.profile if args.profile.is_absolute() else root / args.profile
    schema_path = args.schema if args.schema.is_absolute() else root / args.schema
    try:
        profile = load_json(profile_path)
        schema = load_json(schema_path)
        if args.write_identities:
            validate_profile(profile, schema, check_identity=False)
            write_identities(profile_path, profile, root)
            profile = load_json(profile_path)
        validate_all(root, profile, schema)
        controls = self_test(root, profile, schema) if args.self_test else 0
    except CheckError as error:
        print(f"selfhost-refs-authority: {error}", file=sys.stderr)
        return 1
    suffix = f" negative_controls={controls}" if args.self_test else ""
    print(f"selfhost-refs-authority: ok content_identity={profile['contentIdentitySha256']}{suffix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
