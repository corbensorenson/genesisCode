#!/usr/bin/env python3
"""Independent custody verifier for the self-hosted internal package lock model."""

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
    "productionOperations", "requestKind", "resultKind", "schema", "sourceModule",
    "sourceSha256", "spec", "version",
}
PACKAGE_OPERATIONS = [
    "core/pkg-low::info", "core/pkg-low::lock", "core/pkg-low::update",
    "core/pkg-low::install", "core/pkg-low::verify",
]
GC_OPERATIONS = ["core/gc-low::plan", "core/gc-low::run"]
CLI_OPERATIONS = ["genesis gcpm remove"]
OPERATIONS = PACKAGE_OPERATIONS + GC_OPERATIONS + CLI_OPERATIONS
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::lock-model-authority",
    "decisionInventory": [
        "supported-lock-version-admission", "workspace-and-policy-normalization",
        "requirement-update-policy-normalization",
        "requirement-resolution-strategy-normalization",
        "tag-policy-selector-compatibility", "locked-resolution-metadata-retention",
        "closed-internal-lock-model", "request-bound-result-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation",
        "capability-policy-and-sandbox-path-enforcement",
        "bounded-file-read-and-utf8-validation",
        "generic-toml-decoding-and-term-transport",
        "strict-result-contradiction-checking-and-typed-reification",
        "graph-semver-registry-store-and-persistence-mechanisms",
    ],
    "hostOracle": {"parityOnly": False, "productionRequired": True, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_lock_model_authority.py",
    "kind": "genesis/selfhost-pkg-lock-model-authority-v0.1",
    "productionOperations": OPERATIONS,
    "requestKind": "genesis/pkg-lock-model-authority-request-v0.1",
    "resultKind": "genesis/pkg-lock-model-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_LOCK_MODEL_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/pkg_lock_model_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_PKG_LOCK_MODEL_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "all-lock-consumer-authority", "bootstrap-fixpoint",
    "graph-resolution-mechanism-authority", "h2-package-resolution", "r4-2-e-closure",
    "registry-authority", "release-qualification", "selfhost-toml-codec", "sh-c-closure",
    "workspace-authority",
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


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = source_text(root, profile["sourceModule"], overrides)
    shared = source_text(root, "selfhost/pkg_lock_read_authority_v1.gc", overrides)
    manifest = source_text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = source_text(root, profile["artifact"], overrides)
    adapter = source_text(root, "crates/gc_effects/src/pkg_lock_model_authority.rs", overrides)
    resolution = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", overrides)
    install = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", overrides)
    parent = source_text(root, "crates/gc_effects/src/runner_cap_pkg_low.rs", overrides)
    classifier = source_text(root, "crates/gc_effects/src/pkg_lock_read_authority.rs", overrides)
    gc_dispatch = source_text(root, "crates/gc_effects/src/runner_cap_gc_gpk_low.rs", overrides)
    gc_sources = source_text(root, "crates/gc_effects/src/runner_gc_ops.rs", overrides)
    cli_adapter = source_text(root, "crates/gc_cli_driver/src/pkg_lock_model_authority.rs", overrides)
    cli_remove = source_text(root, "crates/gc_cli_driver/src/pkg_workspace_remove.rs", overrides)
    runner = source_text(root, "crates/gc_effects/src/runner.rs", overrides)
    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")

    for marker in (
        "(def core/pkg::lock-model-authority", profile["requestKind"], profile["resultKind"],
        "normalize-model-requirement", "normalize-model-locked", "normalize-model-document",
        "[:document :kind :op :v]", "selfhost/hash::hash-term request",
    ):
        if marker not in module:
            fail(f"GenesisCode lock model authority missing marker: {marker}")
    for marker in ("(def selfhost/pkg-lock-read::exact-map?", "(def selfhost/pkg-lock-read::map-has-key-loop?"):
        if marker not in shared:
            fail(f"shared lock normalization module missing marker: {marker}")
    if f'    "{profile["sourceModule"]}"' not in manifest or profile["binding"] not in manifest:
        fail("toolchain manifest does not custody lock model module and binding")
    if f':path "{profile["sourceModule"]}"' not in artifact or profile["binding"] not in artifact:
        fail("published artifact does not contain lock model module and binding")

    for marker in (
        "pub(crate) enum PkgLockModelDecision", "pub(crate) fn read_model_toml",
        "fn decode_model_result", "fn decode_model(", ":environment-fingerprint", ":source-selector",
        "MODEL_REQUEST_KIND", "MODEL_RESULT_KIND",
    ):
        if marker not in adapter:
            fail(f"Rust lock model adapter missing marker: {marker}")
    for marker in (
        "const MAX_LOCK_BYTES: u64 = 4 * 1024 * 1024", "read_bounded_lock",
        "bounded_lock_reader_rejects_oversized_input",
    ):
        if marker not in parent:
            fail(f"bounded lock transport missing marker: {marker}")
    for operation in PACKAGE_OPERATIONS + GC_OPERATIONS:
        if operation not in classifier:
            fail(f"runner lazy authority set missing {operation}")
    if 'matches!(op, "core/gc-low::plan" | "core/gc-low::run")' not in classifier:
        fail("GC plan/run are not a closed lazy authority classifier")
    for operation in PACKAGE_OPERATIONS:
        if f'"{operation}" =>' not in resolution:
            fail(f"resolution dispatcher missing {operation}")
    if resolution.count("load_lock_model(") < 4 or install.count("load_lock_model(") != 2:
        fail("not every selected route uses the lock model loader")
    if "authority.read_model_toml(&bytes)" not in resolution:
        fail("production loader does not invoke GenesisCode lock model authority")
    fallback = '#[cfg(any(test, feature = "parity-oracle"))]'
    if fallback not in resolution:
        fail("typed parser fallback is not compile-time parity-only")
    if "gc_pkg::GenesisLock::load" in resolution.split(fallback, 1)[0]:
        fail("production resolution route retains typed Rust lock parser")
    if "gc_pkg::GenesisLock::load" in install:
        fail("install or verify retains typed Rust lock parser")
    if "selfhost package lock model authority is unavailable" not in resolution:
        fail("production missing-authority path does not fail closed")
    if "PkgLockReadAuthority::required_for_request(&req.op, &req.payload)" not in runner:
        fail("runner does not use the closed lock authority operation set")

    for marker in (
        'const AUTHORITY_BINDING: &str = "core/pkg::lock-model-authority"',
        "const SOURCE_LIMIT: usize = 4 * 1024 * 1024", "pub(crate) fn read_bounded(",
        ".take((SOURCE_LIMIT as u64) + 1)", "authorize_bytes(",
        ".get(AUTHORITY_BINDING)", "decode(value, &request_hash)", "validate_model(&model)",
    ):
        if marker not in cli_adapter:
            fail(f"CLI lock model adapter missing marker: {marker}")
    if cli_remove.count("pkg_lock_model_authority::authorize_bytes(") != 2:
        fail("gcpm remove does not authorize both input and emitted lock models")
    production_remove = cli_remove.split("pub(super) fn handle_remove(", 1)[1].split(
        "fn decode_plan(", 1
    )[0]
    if "pkg_lock_model_authority::read_bounded(lock_path)" not in production_remove:
        fail("gcpm remove does not cap the user lock before allocation growth")
    if "std::fs::read(lock_path)" in production_remove:
        fail("gcpm remove retains an unbounded user-controlled lock read")
    if "GenesisLock::" in production_remove or "candidate != plan.model" not in production_remove:
        fail("gcpm remove retains native model authority or omits exact post-write comparison")

    for marker in (
        "pkg_lock_read_authority: Option<&mut PkgLockReadAuthority>",
        "gc_build_sources(", "gc_roots_plan_from_sources(",
    ):
        if marker not in gc_dispatch:
            fail(f"GC dispatch missing lock authority marker: {marker}")
    if gc_dispatch.count("gc_build_sources(") != 2:
        fail("GC plan and run do not each build authority-backed root sources")
    for marker in (
        "lock_authority: Option<&mut PkgLockReadAuthority>",
        "sandbox_path_allow_missing(base_dir, lock_s, false)",
        "runner_cap_pkg_low::read_bounded_lock(&lock_path)",
        "lock_authority.read_model_toml(&bytes)",
        "core/gc/lock-authority-unavailable",
        "GC lock roots require the artifact-loaded GenesisCode lock model authority",
        "gc_lock_authority_fails_closed_before_store_mutation_when_missing",
        "gc_lock_path_failure_is_sealed_before_store_mutation",
    ):
        if marker not in gc_sources:
            fail(f"GC lock-root source route missing marker: {marker}")
    if "gc_pkg::GenesisLock::load" in gc_sources:
        fail("production GC lock-root route retains typed Rust lock parser")

    plan_route = gc_dispatch.split('"core/gc-low::plan" => {', 1)[1].split(
        '"core/gc-low::run" => {', 1
    )[0]
    run_route = gc_dispatch.split('"core/gc-low::run" => {', 1)[1].split(
        '"core/gc-low::pin" => {', 1
    )[0]
    for name, route in (("plan", plan_route), ("run", run_route)):
        source_index = route.find("gc_build_sources(")
        roots_index = route.find("gc_roots_plan_from_sources(")
        dead_index = route.find("gc_store_dead_set(")
        if not (0 <= source_index < roots_index < dead_index):
            fail(f"GC {name} does not consume authority-backed roots before dead-set planning")

    row = next((item for item in ledger.get("semanticDecisions", [])
                if item.get("id") == "SD-PACKAGE-RESOLUTION"), None)
    if not row or row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-RESOLUTION must remain truthful H0")
    if set(row.get("commandSelectors", [])) != {"pkg/*", "gc/*"}:
        fail("semantic ledger does not bind GC lock consumers to package resolution")
    for path in (
        profile["sourceModule"], "crates/gc_effects/src/pkg_lock_model_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        "crates/gc_effects/src/runner_cap_gc_gpk_low.rs",
        "crates/gc_effects/src/runner_gc_ops.rs",
    ):
        if path not in row.get("productionAuthorityPaths", []):
            fail(f"semantic ledger missing production authority path: {path}")
    if profile["spec"] not in row.get("specAuthorityPaths", []):
        fail("semantic ledger missing lock model specification")
    if profile["independentVerifier"] not in row.get("verifierPaths", []):
        fail("semantic ledger missing lock model verifier")
    limitations = "\n".join(row.get("limitations", [])).lower()
    if ("internal" not in limitations or "toml" not in limitations or "h0" not in limitations
            or "core/gc-low::{plan,run}" not in limitations):
        fail("semantic ledger does not disclose the partial internal authority and TOML oracle")
    workspace_row = next((item for item in ledger.get("semanticDecisions", [])
                          if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not workspace_row or workspace_row.get("currentLevel") != "H0":
        fail("SD-PACKAGE-WORKSPACE must remain truthful H0")
    for path in (profile["sourceModule"], "crates/gc_cli_driver/src/pkg_lock_model_authority.rs"):
        if path not in workspace_row.get("productionAuthorityPaths", []):
            fail(f"workspace ledger missing lock model authority path: {path}")
    if profile["spec"] not in workspace_row.get("specAuthorityPaths", []):
        fail("workspace ledger missing lock model specification")
    if profile["independentVerifier"] not in workspace_row.get("verifierPaths", []):
        fail("workspace ledger missing lock model verifier")
    if source_identity(profile["sourceModule"], module.encode()) != profile["sourceSha256"]:
        fail("lock model authority source identity mismatch")


def validate_all(root, profile, schema, overrides=None, check_identity=True) -> None:
    validate_profile(profile, schema, check_identity)
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = [
        profile["sourceModule"], "selfhost/pkg_lock_read_authority_v1.gc",
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_effects/src/pkg_lock_model_authority.rs",
        "crates/gc_effects/src/pkg_lock_read_authority.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        "crates/gc_effects/src/runner_cap_pkg_low.rs",
        "crates/gc_effects/src/runner_cap_gc_gpk_low.rs",
        "crates/gc_effects/src/runner_gc_ops.rs", "crates/gc_effects/src/runner.rs",
        "crates/gc_cli_driver/src/pkg_lock_model_authority.rs",
        "crates/gc_cli_driver/src/pkg_workspace_remove.rs",
    ]
    sources = {path: source_text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-lock-model"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("productionOperations", profile["productionOperations"][:-1]),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        profile_mutation(name, value)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    source_mutation(profile["sourceModule"], "[:document :kind :op :v]", "[:kind :op :v :wrong]", "request-closure")
    source_mutation(profile["sourceModule"], "normalize-model-locked", "legacy-model-locked", "source")
    source_mutation("selfhost/pkg_lock_read_authority_v1.gc", "(def selfhost/pkg-lock-read::exact-map?", "(def selfhost/pkg-lock-read::count-only?", "shared-closure")
    source_mutation("selfhost/toolchain_manifest.gc", profile["sourceModule"], "selfhost/missing.gc", "manifest")
    source_mutation(profile["artifact"], f':path "{profile["sourceModule"]}"', ':path "selfhost/missing.gc"', "artifact")
    source_mutation("crates/gc_effects/src/pkg_lock_model_authority.rs", "fn decode_model_result", "fn legacy_decode", "decoder")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low.rs", "MAX_LOCK_BYTES", "UNBOUNDED_LOCK_BYTES", "bound")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs", "authority.read_model_toml(&bytes)", "legacy_read(&bytes)", "authority-route")
    source_mutation("crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs", "load_lock_model(", "legacy_lock_model(", "install-route")
    source_mutation("crates/gc_effects/src/pkg_lock_read_authority.rs", '"core/pkg-low::verify"', '"core/pkg-low::legacy-verify"', "lazy-route-set")
    source_mutation("crates/gc_effects/src/pkg_lock_read_authority.rs", '"core/gc-low::run"', '"core/gc-low::legacy-run"', "gc-lazy-route-set")
    source_mutation("crates/gc_effects/src/runner_cap_gc_gpk_low.rs", "pkg_lock_read_authority: Option<&mut PkgLockReadAuthority>", "legacy_lock_authority: Option<&mut PkgLockReadAuthority>", "gc-authority-dispatch")
    source_mutation("crates/gc_effects/src/runner_gc_ops.rs", "lock_authority.read_model_toml(&bytes)", "gc_pkg::GenesisLock::load(&lock_path)", "gc-authority-route")
    source_mutation("crates/gc_effects/src/runner_gc_ops.rs", "gc_lock_authority_fails_closed_before_store_mutation_when_missing", "gc_missing_authority_is_ignored", "gc-fail-closed-control")
    source_mutation("crates/gc_effects/src/runner_gc_ops.rs", "sandbox_path_allow_missing(base_dir, lock_s, false)", "base_dir.join(lock_s)", "gc-lock-path-admission")
    source_mutation("crates/gc_effects/src/runner.rs", "PkgLockReadAuthority::required_for_request(&req.op, &req.payload)", "req.op.starts_with(\"core/pkg-low::\")", "lazy-route-use")
    source_mutation("crates/gc_cli_driver/src/pkg_lock_model_authority.rs", ".get(AUTHORITY_BINDING)", ".get(\"native-model\")", "cli-model-route")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_remove.rs", "pkg_lock_model_authority::read_bounded(lock_path)", "std::fs::read(lock_path)", "cli-bounded-read")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_remove.rs", "candidate != plan.model", "candidate == plan.model", "cli-post-write-check")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides, check_identity=True)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 25:
        fail(f"negative control inventory drift: {controls}")
    print(f"selfhost-pkg-lock-model-authority: self-test ok (negative_controls={controls})")
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
            "selfhost-pkg-lock-model-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
        return 0
    except CheckError as error:
        print(f"selfhost-pkg-lock-model-authority: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
