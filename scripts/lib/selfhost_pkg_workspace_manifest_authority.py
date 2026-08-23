#!/usr/bin/env python3
"""Independent custody verifier for structural workspace-manifest authority."""

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


SOURCE_MODULES = [
    "selfhost/pkg_workspace_manifest_core_v1.gc",
    "selfhost/pkg_workspace_manifest_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::workspace-manifest-authority",
    "decisionInventory": [
        "exact-workspace-version-and-root-admission",
        "member-field-and-duplicate-admission",
        "defaults-profile-and-task-type-normalization",
        "runtime-backend-normalization-across-defaults-and-profiles",
        "selected-profile-presence-and-projection",
        "bounded-closed-workspace-config-construction",
        "request-and-source-bound-result-verdict",
    ],
    "hostMechanisms": [
        "bounded-file-read-and-utf8-validation",
        "generic-toml-syntax-decoding-and-neutral-term-transport",
        "artifact-only-bounded-authority-evaluation",
        "strict-request-and-source-bound-result-decoding",
        "typed-workspace-structure-materialization",
        "workspace-relative-path-joining-and-command-dispatch",
    ],
    "hostOracle": {
        "productionRequired": False,
        "reachability": "test-or-parity-only",
        "removalTask": "R4.2.e",
    },
    "independentVerifier": "scripts/lib/selfhost_pkg_workspace_manifest_authority.py",
    "kind": "genesis/selfhost-pkg-workspace-manifest-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/pkg-workspace-manifest-authority-request-v0.1",
    "resultKind": "genesis/pkg-workspace-manifest-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_WORKSPACE_MANIFEST_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_WORKSPACE_NEW_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint",
    "cross-root-crash-atomic-transaction-or-recovery",
    "filesystem-or-path-policy-authority",
    "generic-toml-syntax-codec",
    "h2-workspace-closure",
    "package-command-implementation-authority",
    "r4-2-e-closure",
    "release-qualification",
    "sh-c-closure",
    "wasi-workspace-command-support",
    "workspace-rendering-authority",
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
    if check_identity and canonical_identity(profile) != profile["contentIdentitySha256"]:
        fail("profile content identity mismatch")


def require_markers(subject: str, markers, label: str) -> None:
    for marker in markers:
        if marker not in subject:
            fail(f"{label} missing marker: {marker}")


def require_pattern(subject: str, pattern: str, label: str) -> None:
    if re.search(pattern, subject) is None:
        fail(f"{label} missing structural pattern")


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    core = text(root, SOURCE_MODULES[0], overrides)
    authority = text(root, SOURCE_MODULES[1], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_cli_driver/src/pkg_workspace_manifest_authority.rs", overrides)
    legacy = text(root, "crates/gc_cli_driver/src/pkg_workspace_env_select.rs", overrides)
    workspace_ops = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops.rs", overrides)
    env_ops = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs", overrides)
    tests = text(root, "crates/gc_cli/tests/cli_pkg_workspace.rs", overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(core, [
        "(def selfhost/pkg-workspace-manifest::members-loop",
        '"duplicate workspace member name"', '"duplicate workspace member path"',
        "(def selfhost/pkg-workspace-manifest::defaults",
        "(def selfhost/pkg-workspace-manifest::profile",
        "(def selfhost/pkg-workspace-manifest::tasks",
        "(def selfhost/pkg-workspace-manifest::normalize",
        "workspace version must be exactly 1", "workspace has too many profiles",
        "selfhost/pkg-workspace-env-select::normalize",
    ], "GenesisCode workspace-manifest core")
    require_markers(authority, [
        "(def core/pkg::workspace-manifest-authority", profile["requestKind"],
        profile["resultKind"],
        "[:document :kind :require-profile :selected-profile :source-h :v]",
        '"core/pkg/bad-workspace-manifest"',
        "selfhost/pkg-lock-ops::lower-hex64?",
    ], "GenesisCode workspace-manifest authority")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in manifest:
            fail(f"toolchain manifest missing workspace-manifest marker: {marker}")
        if marker not in artifact:
            fail(f"published artifact missing workspace-manifest marker: {marker}")

    require_markers(adapter, [
        "pub(super) fn load(", "SOURCE_LIMIT: usize = 16 * 1024 * 1024",
        "COLLECTION_LIMIT: usize = 4096", "toml::from_str::<toml::Value>",
        "fn toml_to_term(", "crate::load_selfhost_toolchain",
        ".get(AUTHORITY_BINDING)", "decode(value, &request_hash, &source_hash",
        'require_string(result, ":source-h", source_hash)',
        "fn decode_members(", "fn decode_profiles(", "fn decode_tasks(",
        "require_exact_fields(",
    ], "strict workspace-manifest adapter")
    for forbidden in (
        "WorkspaceConfig::from_toml_str", "normalize_runtime_backend_profile(",
        "runtime_backend_profile_is_compatible(", ".trim().is_empty()",
    ):
        if forbidden in adapter:
            fail(f"native workspace semantic oracle reachable in adapter: {forbidden}")
    if adapter.count("require_exact_fields(") < 7:
        fail("workspace-manifest exact-field check inventory drift")

    require_markers(legacy, [
        '#[cfg(any(test, feature = "parity-harness"))]',
        "pub(super) fn load_workspace(", "WorkspaceConfig::from_toml_str",
    ], "retained workspace parser oracle")
    loader_at = legacy.index("pub(super) fn load_workspace(")
    loader_prefix = legacy[max(0, loader_at - 220):loader_at]
    if '#[cfg(any(test, feature = "parity-harness"))]' not in loader_prefix:
        fail("retained workspace parser is not directly test/parity guarded")
    require_markers(workspace_ops, [
        "pkg_workspace_manifest_authority::load(cli, workspace_file, \"dev\", false)",
        "crate::pkg_workspace_task::resolve(",
    ], "workspace-task manifest custody")
    require_pattern(
        env_ops,
        r"super::pkg_workspace_manifest_authority::load\(\s*cli\s*,\s*workspace_file\s*,\s*profile\s*,\s*true\s*,?\s*\)\?",
        "workspace-env manifest custody",
    )
    require_markers(env_ops, [
        ".selected_profile", "workspace manifest authority omitted required profile",
    ], "workspace-env manifest custody")
    require_markers(tests, [
        "gcpm_env_materializes_deterministic_profile_record",
        "gcpm_run_executes_workspace_task_without_shell_glue",
        "gcpm_env_manifest_authority_rejects_duplicate_members_before_file_access",
        "gcpm_run_manifest_authority_rejects_invalid_task_before_dispatch",
    ], "workspace-manifest integration evidence")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not isinstance(row, dict):
        fail("ledger workspace decision missing")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        profile["kind"], profile["spec"], profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_cli_driver/src/pkg_workspace_manifest_authority.rs",
        "GenesisCode exclusively owns structural workspace-manifest admission",
        "Generic TOML syntax decoding remains a bounded host mechanism",
    ], "workspace-manifest ownership ledger")


def validate_all(root: Path, profile, schema, overrides=None) -> None:
    overrides = overrides or {}
    validate_profile(profile, schema)
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("profile source identity mismatch")
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_cli_driver/src/pkg_workspace_manifest_authority.rs",
        "crates/gc_cli_driver/src/pkg_workspace_env_select.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs",
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
        ("binding", "core/pkg::legacy-workspace-manifest"),
        ("decisionInventory", profile["decisionInventory"][:-1]),
        ("hostMechanisms", profile["hostMechanisms"][:-1]),
        ("hostOracle", {**profile["hostOracle"], "reachability": "production"}),
        ("nonclaims", profile["nonclaims"][:-1]),
        ("sourceSha256", "f" * 64),
    ):
        profile_mutation(name, value)

    def source_mutation(path, old, new, name):
        if old not in sources[path]:
            fail(f"self-test marker absent for {name}")
        mutations.append((profile, {path: sources[path].replace(old, new, 1)}, name))

    core = SOURCE_MODULES[0]
    source_mutation(core, "(def selfhost/pkg-workspace-manifest::members-loop", "(def selfhost/pkg-workspace-manifest::legacy-members", "member admission")
    source_mutation(core, '"duplicate workspace member name"', '"allow duplicate workspace member name"', "member duplicate")
    source_mutation(core, "(def selfhost/pkg-workspace-manifest::defaults", "(def selfhost/pkg-workspace-manifest::legacy-defaults", "defaults")
    source_mutation(core, "(def selfhost/pkg-workspace-manifest::profile", "(def selfhost/pkg-workspace-manifest::legacy-profile", "profiles")
    source_mutation(core, "(def selfhost/pkg-workspace-manifest::tasks", "(def selfhost/pkg-workspace-manifest::legacy-tasks", "tasks")
    source_mutation(core, "workspace version must be exactly 1", "workspace version ignored", "version")
    authority = SOURCE_MODULES[1]
    source_mutation(authority, "(def core/pkg::workspace-manifest-authority", "(def core/pkg::legacy-manifest-authority", "source binding")
    source_mutation(authority, profile["requestKind"], "genesis/legacy-manifest-request", "request kind")
    source_mutation(authority, "selfhost/pkg-lock-ops::lower-hex64?", "selfhost/pkg-lock-read::str?", "source binding")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-manifest", "manifest binding")
    mutations.append((profile, {
        profile["artifact"]: sources[profile["artifact"]].replace(
            profile["binding"], "core/pkg::missing-manifest"
        )
    }, "artifact binding"))
    adapter = "crates/gc_cli_driver/src/pkg_workspace_manifest_authority.rs"
    source_mutation(adapter, "crate::load_selfhost_toolchain", "crate::load_legacy_toolchain", "artifact loader")
    source_mutation(adapter, "toml::from_str::<toml::Value>", "WorkspaceConfig::from_toml_str", "neutral TOML transport")
    source_mutation(adapter, "require_exact_fields(", "accept_open_fields(", "field closure")
    source_mutation(adapter, 'require_string(result, ":source-h", source_hash)', 'required_string(result, ":source-h")', "source binding")
    source_mutation(adapter, "COLLECTION_LIMIT: usize = 4096", "COLLECTION_LIMIT: usize = usize::MAX", "collection bound")
    legacy = "crates/gc_cli_driver/src/pkg_workspace_env_select.rs"
    source_mutation(
        legacy,
        '#[cfg(any(test, feature = "parity-harness"))]\n#[allow(dead_code)] // Retained only as the explicit pre-authority compatibility oracle.\npub(super) fn load_workspace',
        '#[allow(dead_code)]\npub(super) fn load_workspace',
        "legacy reachability",
    )
    workspace_ops = "crates/gc_cli_driver/src/pkg_workspace_ops.rs"
    source_mutation(workspace_ops, "pkg_workspace_manifest_authority::load(cli, workspace_file, \"dev\", false)", "pkg_workspace_env_select::load_workspace(workspace_file, \"dev\")", "run custody")
    env_ops = "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs"
    source_mutation(env_ops, "super::pkg_workspace_manifest_authority::load(", "super::pkg_workspace_env_select::load_workspace(", "env custody")
    source_mutation(env_ops, "profile, true)", "profile, false)", "env required-profile custody")
    tests = "crates/gc_cli/tests/cli_pkg_workspace.rs"
    source_mutation(tests, "gcpm_env_manifest_authority_rejects_duplicate_members_before_file_access", "legacy_duplicate_test", "duplicate control")
    source_mutation(tests, "gcpm_run_manifest_authority_rejects_invalid_task_before_dispatch", "legacy_task_test", "task control")
    ledger = "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json"
    source_mutation(ledger, profile["kind"], "native-workspace-manifest", "ledger authority")
    source_mutation(ledger, "Generic TOML syntax decoding remains a bounded host mechanism", "Workspace manifest decisions remain host-authoritative", "ledger residual")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 30:
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
            "selfhost-pkg-workspace-manifest-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
    except CheckError as error:
        print(f"selfhost-pkg-workspace-manifest-authority: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
