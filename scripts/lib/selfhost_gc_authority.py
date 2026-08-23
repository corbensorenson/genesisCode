#!/usr/bin/env python3
"""Independent verifier for H2 GenesisCode artifact-GC authority."""

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
    "artifact", "auditDate", "binding", "contentIdentitySha256",
    "decisionInventory", "hostMechanisms", "hostOracle", "independentVerifier",
    "kind", "nonclaims", "productionEntrypoints", "requestKind", "resultKind",
    "runtimeEvidence", "schema", "sourceModule", "sourceSha256", "spec", "version",
}

DECISIONS = [
    "pins-document-admission-and-normalization",
    "pin-unpin-target-admission",
    "canonical-pins-byte-rendering",
    "reference-tombstone-and-pinned-reference-resolution",
    "root-selection-order-and-provenance",
    "artifact-edge-selection",
    "live-closure-edge-admission",
    "dead-set-selection",
    "reclaim-byte-accounting",
    "largest-dead-artifact-ranking",
    "quarantine-purge-selection",
]

HOST_MECHANISMS = [
    "artifact-only-authority-bootstrap-and-bounded-evaluation",
    "bounded-generic-toml-and-lock-model-observation",
    "bounded-reference-and-inventory-observation",
    "content-identity-verification-and-coreform-decoding",
    "exact-authority-edge-work-queue-execution",
    "store-pins-and-quarantine-locking",
    "system-time-and-file-age-observation",
    "atomic-exact-pins-write",
    "exact-authorized-rename-delete-and-purge",
]

NONCLAIMS = {
    "bootstrap-fixpoint", "gpk-sync-store-refs-or-package-authority",
    "h3-h4-closure", "r4-2-e-closure", "release-qualification", "sh-c-closure",
}

RUNTIME = {
    "allocationLimit": 320_000_000,
    "largestReportEntries": 25,
    "maxClosureObjects": 50_000,
    "maxItems": 65_536,
    "maxPayloadBytes": 8_388_608,
    "maxPinsBytes": 4_194_304,
    "stepLimit": 80_000_000,
}

CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/gc::authority",
    "decisionInventory": DECISIONS,
    "hostMechanisms": HOST_MECHANISMS,
    "hostOracle": {"removalTask": "R4.2.e", "required": False},
    "independentVerifier": "scripts/lib/selfhost_gc_authority.py",
    "kind": "genesis/selfhost-gc-authority-v0.1",
    "productionEntrypoints": ["genesis", "genesis_wasi"],
    "requestKind": "genesis/gc-authority-request-v0.1",
    "resultKind": "genesis/gc-authority-result-v0.1",
    "runtimeEvidence": RUNTIME,
    "schema": "docs/spec/SELFHOST_GC_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/gc_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_STORE_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}


def profile_identity(profile) -> str:
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


def validate(profile, schema, check_identity=True) -> None:
    if set(profile) != FIELDS:
        fail("profile field closure drift")
    if (
        schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("type") != "object"
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != FIELDS
        or set(schema.get("properties", {})) != FIELDS
    ):
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
    if check_identity and profile["contentIdentitySha256"] != profile_identity(profile):
        fail("profile content identity mismatch")


def read_text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    return (root / relative).read_text()


def require_all(text: str, markers, context: str) -> None:
    for marker in markers:
        if marker not in text:
            fail(f"{context} missing {marker!r}")


def static_check(root: Path, profile, overrides=None, artifact_path=None, check_artifact=True) -> None:
    overrides = overrides or {}
    source_relative = profile["sourceModule"]
    source_path = root / source_relative
    if source_path.is_symlink() or not source_path.is_file() or root not in source_path.resolve().parents:
        fail("GC authority source is missing, escaping, or symlinked")
    source = read_text(root, source_relative, overrides)
    if source_identity(source_relative, source.encode()) != profile["sourceSha256"]:
        fail("GC authority source identity mismatch")
    require_all(source, [
        f"(def {profile['binding']}", profile["requestKind"], profile["resultKind"],
        ":request-h (selfhost/hash::hash-term request)", "selfhost/gc-authority::pins-model",
        "selfhost/gc-authority::render-pins", "core/gc/reach::roots-plan",
        "core/vcs/reach::artifact-ref-plan", "selfhost/gc-authority::dead-plan",
        "selfhost/gc-authority::purge-plan", "(quote :roots)",
        "(quote :artifact-edges)", "(quote :dead-plan)", "(quote :pins-update)",
        "(quote :purge-plan)", "pinned reference is not present",
    ], "GenesisCode GC authority")
    if source.count("selfhost/store-authority::exact-map?") < 8:
        fail("GC authority does not close request and observation maps")
    if "core/effect::" in source or "core/host::" in source:
        fail("GC authority contains ambient effect or host access")

    manifest = read_text(root, "selfhost/toolchain_manifest.gc", overrides)
    if manifest.count(f'"{source_relative}"') != 1 or manifest.count(profile["binding"]) != 1:
        fail("GC authority manifest custody drift")

    bridge_path = "crates/gc_effects/src/gc_authority.rs"
    bridge = read_text(root, bridge_path, overrides)
    require_all(bridge, [
        f'const BINDING: &str = "{profile["binding"]}"',
        'op.starts_with("core/gc-low::")', "selfhost_authority_config()",
        "artifact GC requires the artifact-loaded GenesisCode authority",
        "Self::load(config)",
        f'const REQUEST_KIND: &str = "{profile["requestKind"]}"',
        f'const RESULT_KIND: &str = "{profile["resultKind"]}"',
        "const STEP_LIMIT: u64 = 80_000_000", "const ALLOC_LIMIT: u64 = 320_000_000",
        "const MAX_ITEMS: u64 = 65_536", "load_selfhost_coreform_toolchain_v1_with_mode",
        "context.reset_counters()", "exact_map(", "require_sorted_unique",
        "require_string(fields, \":request-h\", &hex32(request_hash))",
        "result field set mismatch",
        "result rejection code is outside closed inventory", "returned sealed ERROR",
    ], "Rust GC authority adapter")
    if "unwrap_or_default()" in bridge or "unwrap_or(true)" in bridge:
        fail("GC adapter contains success-capable result defaulting")

    runner = read_text(root, "crates/gc_effects/src/runner.rs", overrides)
    require_all(runner, [
        "let mut gc_authority = None", "GcAuthority::ensure", "gc_authority.as_mut()",
    ], "runner GC authority custody")

    cap_path = "crates/gc_effects/src/runner_cap_gc_gpk_low.rs"
    cap = read_text(root, cap_path, overrides)
    for op in ("plan", "run", "pin", "unpin", "purge"):
        require_all(cap, [
            f'"core/gc-low::{op}"',
            f'"core/gc-low::{op} requires the artifact-loaded GenesisCode GC authority"',
        ], f"GC {op} production route")
    require_all(cap, [
        ".roots(", "gc_closure_local(store, authority",
        ".dead_plan(", '.update_pins(":pin"',
        '.update_pins(":unpin"', ".purge_plan(",
        "let _pins_lock = gc_path_lock(&pins_path)?", "let _quarantine_lock",
        "gc_store_lock(store_dir)?", "gc_store_inventory(store)?",
        "store.verify_hex(hash)", "quarantine_store", "atomic_write_text(&pins_path, &plan.body)",
        "for hash in &dead_plan.dead", "for hash in purge",
        "authorized GC quarantine destination already exists",
    ], "GC production capability")
    for residual in (
        "gc_pins_load", "gc_pins_write", "GcPins::empty", "gc_store_dead_set",
        "gc_roots_plan_from_sources", "sync_closure_local",
    ):
        if residual in cap:
            fail(f"GC production route retains Rust semantic residual {residual!r}")
    if cap.index(".dead_plan(") > cap.index("for hash in &dead_plan.dead"):
        fail("GC mutation precedes authority dead plan")
    pin_lock = cap.index("let _pins_lock = gc_path_lock(&pins_path)?")
    if pin_lock > cap.index('.update_pins(":pin"'):
        fail("pin read/update is not serialized before authority evaluation")
    purge_lock = cap.index("let _quarantine_lock")
    if purge_lock > cap.index(".purge_plan("):
        fail("purge inventory/decision is not serialized")

    ops_path = "crates/gc_effects/src/runner_gc_ops.rs"
    ops = read_text(root, ops_path, overrides)
    require_all(ops, [
        "pub(super) fn gc_pins_document_at", "options.custom_flags(libc::O_NONBLOCK)",
        "let metadata = file", ".take(MAX_PINS_BYTES + 1)",
        "pins path must identify a regular file", "pub(super) fn gc_path_lock",
        "store.verify_hex(&hash)?", "quarantine_store.verify_hex(&hash)?",
        "pub(super) fn gc_closure_local", "if seen.len() > 50_000",
        ".artifact_edges(artifact, true, true, depth_left > 0)",
        "for next in edges.refs", "for parent in edges.parents",
        "gc_pins_reader_rejects_fifo_without_blocking",
    ], "bounded GC host mechanisms")
    for residual in (
        "pub(super) struct GcPins", "fn gc_pins_load", "fn gc_pins_write",
        "fn gc_store_dead_set", "fn gc_roots_plan_from_sources",
    ):
        if residual in ops:
            fail(f"GC host mechanism retains semantic oracle {residual!r}")

    dispatch = read_text(root, "crates/gc_effects/src/runner_capability_dispatch.rs", overrides)
    require_all(dispatch, [
        "gc_authority: Option<&mut GcAuthority>", "capability_gc_gpk_low(", "gc_authority,",
    ], "GC dispatcher")

    tests = read_text(root, "crates/gc_cli/tests/cli_gc.rs", overrides)
    require_all(tests, [
        "gc_plan_then_run_deletes_unreachable_artifacts",
        "gc_quarantine_and_purge_roundtrip", "gc_pin_prevents_deletion",
        "gc_unpin_allows_reclaim_after_run", "gc_pin_ref_keeps_target_even_with_no_refs_root_scan",
        "gc_keeps_tag_ref_commit_closure_and_prunes_unreachable",
        "gc_pin_rejects_malformed_existing_pins_without_overwrite",
        "gc_plan_accepts_tombstoned_refs_and_treats_their_objects_as_dead",
        "gc_plan_rejects_corrupt_named_inventory_without_mutation",
    ], "GC authority tests")

    ledger = json.loads(
        read_text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides),
        object_pairs_hook=unique_object,
    )
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-ARTIFACT-GC"]
    if len(rows) != 1:
        fail("SD-ARTIFACT-GC ledger row missing or duplicated")
    row = rows[0]
    if row.get("currentLevel") != "H2" or row.get("fallbackReachability") != "none-proven":
        fail("SD-ARTIFACT-GC H2 claim drift")
    for relative in (source_relative, profile["artifact"]):
        if relative not in row.get("productionAuthorityPaths", []):
            fail(f"SD-ARTIFACT-GC production authority omits {relative}")
    for relative in (profile["spec"], "policies/selfhost_gc_authority_v0.1.json"):
        if relative not in row.get("specAuthorityPaths", []):
            fail(f"SD-ARTIFACT-GC spec authority omits {relative}")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("SD-ARTIFACT-GC verifier custody drift")

    spec = read_text(root, profile["spec"], overrides)
    require_all(spec, [
        "normative H2 contract for `SD-ARTIFACT-GC`", "sole production semantic producer",
        "No Rust semantic fallback", "descriptor is a regular file",
        "repeat that proof immediately before mutation", "does not claim H3/H4",
    ], "GC authority specification")

    if check_artifact:
        artifact = artifact_path or (root / profile["artifact"])
        data = artifact.read_bytes()
        for marker in (source_relative, profile["binding"], profile["requestKind"], profile["resultKind"]):
            if marker.encode() not in data:
                fail(f"GC authority artifact marker missing: {marker}")


def self_test(root: Path, profile, schema) -> None:
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

    reject_profile(lambda value: value.__setitem__("binding", "core/gc::legacy"))
    reject_profile(lambda value: value["decisionInventory"].pop())
    reject_profile(lambda value: value["hostMechanisms"].append("native-dead-planner"))
    reject_profile(lambda value: value["hostOracle"].__setitem__("required", True))
    reject_profile(lambda value: value["nonclaims"].remove("h3-h4-closure"))
    reject_profile(lambda value: value["runtimeEvidence"].__setitem__("maxPinsBytes", 0))
    reject_profile(lambda value: value.__setitem__("kind", "wrong"))
    reject_profile(lambda value: value.__setitem__("contentIdentitySha256", "0" * 64))

    bad_schema = copy.deepcopy(schema)
    bad_schema["additionalProperties"] = True
    try:
        validate(profile, bad_schema)
    except CheckError:
        controls += 1
    else:
        fail("open schema mutation was accepted")

    mutations = [
        ("selfhost/toolchain_manifest.gc", lambda text: text.replace(profile["binding"], "core/gc::legacy", 1)),
        (profile["sourceModule"], lambda text: text.replace("(quote :dead-plan)", "(quote :legacy-plan)", 1)),
        ("crates/gc_effects/src/gc_authority.rs", lambda text: text.replace("result field set mismatch", "open result", 1)),
        ("crates/gc_effects/src/runner.rs", lambda text: text.replace("GcAuthority::ensure", "GcAuthority::legacy", 1)),
        ("crates/gc_effects/src/runner_cap_gc_gpk_low.rs", lambda text: text.replace(".dead_plan(", ".gc_store_dead_set(", 1)),
        ("crates/gc_effects/src/runner_gc_ops.rs", lambda text: text.replace("options.custom_flags(libc::O_NONBLOCK);", "", 1)),
        ("crates/gc_cli/tests/cli_gc.rs", lambda text: text.replace("gc_pin_rejects_malformed_existing_pins_without_overwrite", "missing_negative", 1)),
        ("docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", lambda text: text.replace('"id": "SD-ARTIFACT-GC",', '"id": "SD-ARTIFACT-GC",\n      "fallbackReachability": "reachable",', 1)),
    ]
    for relative, mutate in mutations:
        overrides = {relative: mutate((root / relative).read_text())}
        try:
            static_check(root, profile, overrides, check_artifact=False)
        except (CheckError, json.JSONDecodeError):
            controls += 1
        else:
            fail(f"static mutation was accepted: {relative}")

    if controls != 17:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-gc-authority-self-test: ok (negative_controls={controls})")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        profile = load_json(root / args.profile)
        schema = load_json(root / args.schema)
        validate(profile, schema)
        artifact = args.artifact
        if artifact is not None and not artifact.is_absolute():
            artifact = root / artifact
        static_check(root, profile, artifact_path=artifact)
        if args.self_test:
            self_test(root, profile, schema)
        print(
            "selfhost-gc-authority: ok "
            f"(decisions={len(DECISIONS)} host_oracle=none level=H2)"
        )
        return 0
    except (CheckError, OSError, json.JSONDecodeError) as error:
        print(f"selfhost-gc-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
