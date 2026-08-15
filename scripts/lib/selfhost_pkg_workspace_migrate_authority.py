#!/usr/bin/env python3
"""Independent custody verifier for package workspace-migrate authority."""

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


def parse_json(raw: str, name: str):
    try:
        value = json.loads(raw, object_pairs_hook=unique_object)
    except json.JSONDecodeError as error:
        fail(f"cannot parse {name}: {error}")
    if not isinstance(value, dict):
        fail(f"{name} root must be object")
    return value


def load_json(path: Path):
    try:
        return parse_json(path.read_text(), str(path))
    except OSError as error:
        fail(f"cannot read {path}: {error}")


SOURCE_MODULES = ["selfhost/pkg_workspace_migrate_authority_v1.gc"]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::workspace-migrate-authority",
    "decisionInventory": [
        "optional-workspace-name-default-selection",
        "bounded-dependency-admission-and-snapshot-hash-filtering",
        "canonical-workspace-member-defaults-and-task-rendering",
        "canonical-lock-model-construction-and-lock-writer-composition",
        "request-bound-migration-report-and-body-identity",
    ],
    "hostMechanisms": [
        "bounded-manifest-parse-and-path-observation",
        "artifact-only-bounded-authority-evaluation",
        "strict-request-bound-result-and-cross-document-decoding",
        "two-destination-regular-file-ancestor-and-symlink-preflight",
        "exact-byte-temporary-file-persistence-and-atomic-rename",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_pkg_workspace_migrate_authority.py",
    "kind": "genesis/selfhost-pkg-workspace-migrate-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/pkg-workspace-migrate-authority-request-v0.1",
    "resultKind": "genesis/pkg-workspace-migrate-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_WORKSPACE_MIGRATE_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_WORKSPACE_NEW_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "crash-atomic-multi-file-commit-or-recovery",
    "filesystem-or-path-policy-authority", "generic-toml-or-path-authority",
    "h2-workspace-closure", "r4-2-e-closure", "release-qualification",
    "sh-c-closure", "wasi-workspace-migrate-support",
    "workspace-environment-task-resolution-or-manifest-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def text(root: Path, relative: str, overrides) -> str:
    if relative in overrides:
        return overrides[relative]
    try:
        return (root / relative).read_text()
    except OSError as error:
        fail(f"cannot read {relative}: {error}")


def source_identity(root: Path, overrides) -> str:
    digest = hashlib.sha256()
    for relative in SOURCE_MODULES:
        digest.update(relative.encode())
        digest.update(b"\0")
        digest.update(text(root, relative, overrides).encode())
        digest.update(b"\0")
    return digest.hexdigest()


def validate_profile(profile, schema, check_identity=True) -> None:
    if set(profile) != FIELDS:
        fail("profile field closure drift")
    if (schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
            or schema.get("type") != "object"
            or schema.get("additionalProperties") is not False
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


def require_markers(subject: str, markers, label: str) -> None:
    for marker in markers:
        if marker not in subject:
            fail(f"{label} missing marker: {marker}")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    module = text(root, SOURCE_MODULES[0], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_cli_driver/src/pkg_workspace_migrate.rs", overrides)
    custody = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops.rs", overrides)
    route = text(root, "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs", overrides)
    tests = text(root, "crates/gc_cli/tests/cli_pkg_workspace.rs", overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(module, [
        "(def core/pkg::workspace-migrate-authority", profile["requestKind"],
        profile["resultKind"], "selfhost/pkg-workspace-migrate::requirements-loop",
        "selfhost/pkg-resolution-plan::hash64?", "selfhost/pkg-workspace-migrate::workspace-body",
        "selfhost/pkg-workspace-migrate::lock-model", "core/pkg/bad-workspace-migrate",
        ":workspace-h (selfhost/pkg-scaffold::hash-body body)",
    ], "GenesisCode workspace-migrate authority")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in manifest:
            fail(f"toolchain manifest missing workspace-migrate marker: {marker}")
        if marker not in artifact:
            fail(f"published artifact missing workspace-migrate marker: {marker}")

    require_markers(adapter, [
        "const AUTHORITY_BINDING", "const LOCK_WRITE_BINDING", ".get(AUTHORITY_BINDING)",
        ".get(LOCK_WRITE_BINDING)", "decode_authorized(", "decode_lock_write(",
        "require_exact_fields(", "GenesisLock::from_toml_str",
        "workspace-migrate lock contradicts request", "preflight_paths(lock_path, workspace_path)",
        "file_type().is_symlink()", "atomic_write_text(lock_path, &lock_bytes)",
    ], "strict workspace-migrate adapter")
    production = adapter[adapter.index("pub(super) fn handle_migrate("):adapter.index("fn migration_request(")]
    decode_at = production.find("decode_authorized(")
    writer_at = production.find(".get(LOCK_WRITE_BINDING)")
    writer_decode_at = production.find("decode_lock_write(")
    cross_check_at = production.find("validate_documents(")
    preflight_at = production.find("preflight_paths(lock_path, workspace_path)")
    write_at = production.find("atomic_write_text(lock_path, &lock_bytes)")
    positions = [decode_at, writer_at, writer_decode_at, cross_check_at, preflight_at, write_at]
    if min(positions) < 0 or positions != sorted(positions):
        fail("workspace-migrate authority/preflight/write causal order drift")
    if "to_toml_canonical" in production or "handle_migrate_parity" in production:
        fail("native workspace-migrate fallback reachable in production")
    require_markers(custody, [
        "#[cfg(any(test, feature = \"parity-harness\"))]", "fn handle_migrate_parity(",
        "pkg_workspace_migrate::handle_migrate(",
    ], "retained workspace-migrate oracle")
    require_markers(route, ["pkg_workspace_ops::handle_migrate(", "cli,"], "migrate route")
    require_markers(tests, [
        "gcpm_migrate_creates_workspace_and_lock_from_package_manifest",
        "gcpm_migrate_defaults_name_and_filters_unusable_dependency_hashes",
        "gcpm_migrate_rejects_empty_workspace_without_writes",
        "gcpm_migrate_preflights_all_destinations_before_writing",
    ], "workspace-migrate integration evidence")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not isinstance(row, dict):
        fail("ledger workspace decision missing")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        profile["kind"], profile["spec"], profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_cli_driver/src/pkg_workspace_migrate.rs",
        "Workspace environment descriptor, projection, hashing, and materialization and manifest decisions remain host-authoritative",
    ], "workspace ownership ledger")


def validate_all(root: Path, profile, schema, overrides=None) -> None:
    overrides = overrides or {}
    validate_profile(profile, schema)
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("profile source identity mismatch")
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_cli_driver/src/pkg_workspace_migrate.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops.rs",
        "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs",
        "crates/gc_cli/tests/cli_pkg_workspace.rs",
        "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json",
    ]
    sources = {path: text(root, path, {}) for path in paths}
    mutations = []

    def profile_mutation(name, value):
        changed = copy.deepcopy(profile)
        changed[name] = value
        changed["contentIdentitySha256"] = canonical_identity(changed)
        mutations.append((changed, {}, name))

    for name, value in (
        ("binding", "core/pkg::legacy-workspace-migrate"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("hostOracle", {"parityOnly": False, "productionRequired": True, "removalTask": "R4.2.e"}),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        profile_mutation(name, value)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    source_mutation(SOURCE_MODULES[0], "(def core/pkg::workspace-migrate-authority", "(def core/pkg::legacy-migrate", "source")
    source_mutation(SOURCE_MODULES[0], "selfhost/pkg-resolution-plan::hash64?", "core/str::len", "hash filter")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-migrate", "manifest")
    mutations.append((profile, {profile["artifact"]: sources[profile["artifact"]].replace(profile["binding"], "core/pkg::missing-migrate")}, "artifact"))
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_migrate.rs", ".get(AUTHORITY_BINDING)", ".get(\"native\")", "loader")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_migrate.rs", ".get(LOCK_WRITE_BINDING)", ".get(\"native-writer\")", "writer route")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_migrate.rs", "workspace-migrate lock contradicts request", "legacy lock accepted", "cross-check")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_migrate.rs", "preflight_paths(lock_path, workspace_path)?", "Ok(())?", "preflight")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_migrate.rs", "file_type().is_symlink()", "file_type().is_file()", "symlink")
    source_mutation("crates/gc_cli_driver/src/pkg_workspace_ops.rs", "fn handle_migrate_parity(", "fn handle_migrate(", "parity")
    source_mutation("crates/gc_cli/tests/cli_pkg_workspace.rs", "gcpm_migrate_preflights_all_destinations_before_writing", "legacy_preflight_test", "integration")
    source_mutation("docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", profile["kind"], "native-workspace-migrate", "ledger")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 18:
        fail(f"negative control inventory drift: {controls}")
    return controls


def write_identities(path: Path, profile, root: Path) -> None:
    profile["sourceSha256"] = source_identity(root, {})
    profile["contentIdentitySha256"] = canonical_identity(profile)
    path.write_text(json.dumps(profile, indent=2) + "\n")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--write-identities", action="store_true")
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        profile = load_json(args.profile)
        schema = load_json(args.schema)
        if args.write_identities:
            write_identities(args.profile, profile, root)
            profile = load_json(args.profile)
        validate_all(root, profile, schema)
        controls = self_test(root, profile, schema) if args.self_test else 0
        print(
            "selfhost-pkg-workspace-migrate-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
    except CheckError as error:
        print(f"selfhost-pkg-workspace-migrate-authority: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
