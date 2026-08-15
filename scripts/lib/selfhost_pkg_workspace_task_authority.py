#!/usr/bin/env python3
"""Independent custody verifier for gcpm workspace-task authority."""

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
    "selfhost/pkg_workspace_task_core_v1.gc",
    "selfhost/pkg_workspace_task_authority_v1.gc",
]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::workspace-task-authority",
    "decisionInventory": [
        "backend-admission-before-task-interpretation",
        "exact-requested-task-lookup",
        "closed-command-normalization-and-aliases",
        "primary-input-selection-and-package-default",
        "action-specific-option-grammar",
        "active-executable-engine-membership",
        "contract-hash-normalization-and-validation",
        "request-bound-closed-canonical-action",
    ],
    "hostMechanisms": [
        "bounded-workspace-toml-and-build-profile-observation",
        "non-backend-structural-workspace-admission",
        "artifact-only-bounded-authority-evaluation",
        "strict-request-bound-action-decoding",
        "workspace-relative-path-joining",
        "contract-file-read-and-hash-verification",
        "typed-action-dispatch",
    ],
    "hostOracle": {
        "productionRequired": False,
        "reachability": "test-or-parity-only",
        "removalTask": "R4.2.e",
    },
    "independentVerifier": "scripts/lib/selfhost_pkg_workspace_task_authority.py",
    "kind": "genesis/selfhost-pkg-workspace-task-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/pkg-workspace-task-authority-request-v0.1",
    "resultKind": "genesis/pkg-workspace-task-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_WORKSPACE_TASK_AUTHORITY_v0.1.schema.json",
    "sourceModules": SOURCE_MODULES,
    "spec": "docs/spec/SELFHOST_PKG_WORKSPACE_NEW_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint",
    "environment-descriptor-projection-hash-or-materialization-authority",
    "filesystem-or-path-policy-authority",
    "generic-toml-or-path-authority",
    "h2-workspace-closure",
    "manifest-authority",
    "package-command-implementation-authority",
    "r4-2-e-closure",
    "release-qualification",
    "sh-c-closure",
    "wasi-workspace-task-support",
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


def validate_sources(root: Path, profile, overrides=None) -> None:
    overrides = overrides or {}
    core = text(root, SOURCE_MODULES[0], overrides)
    authority = text(root, SOURCE_MODULES[1], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_cli_driver/src/pkg_workspace_task.rs", overrides)
    workspace_ops = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops.rs", overrides)
    native = text(root, "crates/gc_cli_driver/src/pkg_task_runner.rs", overrides)
    route = text(root, "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs", overrides)
    tests = text(root, "crates/gc_cli/tests/cli_pkg_workspace.rs", overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(core, [
        "(def selfhost/pkg-workspace-task::tasks?", "(def selfhost/pkg-workspace-task::find",
        "(def selfhost/pkg-workspace-task::normalize-action", '"build"', '"lint"', '"bench"',
        "(def selfhost/pkg-workspace-task::parse-args", '"--contract-h"', '"--engine"',
        "(def selfhost/pkg-workspace-task::resolve", ":stage1-pipeline", ":emit-wasm",
    ], "GenesisCode workspace-task core")
    require_markers(authority, [
        "(def core/pkg::workspace-task-authority", profile["requestKind"], profile["resultKind"],
        "core/pkg::workspace-env-select-authority", "selfhost/pkg-workspace-task::resolve",
        "[:active :default :engines :kind :profile :profile-backend :task :tasks :v]",
        '"core/pkg/bad-workspace-env-selection"', '"core/pkg/bad-workspace-task"',
    ], "GenesisCode workspace-task authority")
    selection_at = authority.find("core/pkg::workspace-env-select-authority")
    resolution_at = authority.find("selfhost/pkg-workspace-task::resolve", selection_at)
    if selection_at < 0 or resolution_at < 0 or selection_at >= resolution_at:
        fail("backend admission no longer precedes task interpretation")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in manifest:
            fail(f"toolchain manifest missing workspace-task marker: {marker}")
        if marker not in artifact:
            fail(f"published artifact missing workspace-task marker: {marker}")

    require_markers(adapter, [
        "pub(crate) fn resolve(", "fn task_request(", "fn task_observation(",
        "TASK_LIMIT: usize = 256", "TASK_ARG_LIMIT: usize = 64",
        "crate::load_selfhost_toolchain", ".get(AUTHORITY_BINDING)",
        "decode_authorized(", "decode_action(", "require_exact_fields(",
        'require_string(action, ":task", requested_task)',
        'require_bool(result, ":compatible", true)', "task_engine_inventory()",
    ], "strict workspace-task adapter")
    if adapter.count("require_exact_fields(") != 4:
        fail("workspace-task exact-field check inventory drift")
    for forbidden in (
        "workspace.tasks.get(task_name)", "task.cmd.trim().to_ascii_lowercase()",
        'match cmd.as_str()', "parse_run_like_args(", "parse_eval_args(",
    ):
        if forbidden in adapter:
            fail(f"native task semantic oracle reachable in adapter: {forbidden}")

    require_markers(workspace_ops, [
        "pub(crate) fn resolve_workspace_task_for_run(",
        'pkg_workspace_env_select::load_workspace(workspace_file, "dev")',
        "crate::pkg_workspace_task::resolve(", "workspace.default_runtime_backend.as_deref()",
        "workspace.profile_runtime_backend.as_deref()",
    ], "workspace-task production custody")
    require_markers(native, [
        "#[cfg(any(test, feature = \"parity-harness\"))]",
        "pub(crate) fn resolve_workspace_task_parity(",
        "pub(crate) fn verify_contract_task_file_hash(",
    ], "retained native task oracle")
    require_markers(route, [
        "pkg_workspace_ops::resolve_workspace_task_for_run(cli, workspace_file, task)",
        "pkg_task_runner::verify_contract_task_file_hash",
    ], "workspace-task CLI route")
    require_markers(tests, [
        "gcpm_run_executes_workspace_task_without_shell_glue",
        "gcpm_run_invalid_selected_backend_rejects_before_task_resolution",
        "gcpm_run_selfhost_authority_rejects_unsupported_task_command",
        "gcpm_run_selfhost_authority_rejects_ignored_package_arguments",
        "gcpm_run_selfhost_authority_rejects_unavailable_engine_before_file_access",
    ], "workspace-task integration evidence")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not isinstance(row, dict):
        fail("ledger workspace decision missing")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        profile["kind"], profile["spec"], profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_cli_driver/src/pkg_workspace_task.rs",
        "GenesisCode exclusively owns gcpm run task lookup",
        "Workspace-relative path joining, contract-file hashing, and typed action dispatch remain host mechanisms",
    ], "workspace-task ownership ledger")


def validate_all(root: Path, profile, schema, overrides=None) -> None:
    overrides = overrides or {}
    validate_profile(profile, schema)
    if source_identity(root, overrides) != profile["sourceSha256"]:
        fail("profile source identity mismatch")
    validate_sources(root, profile, overrides)


def self_test(root: Path, profile, schema) -> int:
    paths = SOURCE_MODULES + [
        "selfhost/toolchain_manifest.gc", profile["artifact"],
        "crates/gc_cli_driver/src/pkg_workspace_task.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops.rs",
        "crates/gc_cli_driver/src/pkg_task_runner.rs",
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
        ("binding", "core/pkg::legacy-workspace-task"),
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
    source_mutation(core, "(def selfhost/pkg-workspace-task::find", "(def selfhost/pkg-workspace-task::legacy-find", "task lookup")
    source_mutation(core, "(def selfhost/pkg-workspace-task::normalize-action", "(def selfhost/pkg-workspace-task::legacy-action", "command normalization")
    source_mutation(core, '"build"', '"legacy-build"', "command alias")
    source_mutation(core, "(def selfhost/pkg-workspace-task::parse-args", "(def selfhost/pkg-workspace-task::legacy-args", "argument grammar")
    source_mutation(core, '"--engine"', '"--legacy-engine"', "engine option")
    authority = SOURCE_MODULES[1]
    source_mutation(authority, "(def core/pkg::workspace-task-authority", "(def core/pkg::legacy-task-authority", "source binding")
    source_mutation(authority, profile["requestKind"], "genesis/legacy-task-request", "request kind")
    source_mutation(authority, "core/pkg::workspace-env-select-authority", "core/pkg::legacy-env-select", "backend composition")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-task", "manifest binding")
    mutations.append((profile, {
        profile["artifact"]: sources[profile["artifact"]].replace(
            profile["binding"], "core/pkg::missing-task"
        )
    }, "artifact binding"))
    adapter = "crates/gc_cli_driver/src/pkg_workspace_task.rs"
    source_mutation(adapter, "crate::load_selfhost_toolchain", "crate::load_legacy_toolchain", "artifact loader")
    source_mutation(adapter, "require_exact_fields(", "accept_open_fields(", "field closure")
    source_mutation(adapter, 'require_string(action, ":task", requested_task)', 'required_string(action, ":task")', "request binding")
    source_mutation(adapter, "TASK_LIMIT: usize = 256", "TASK_LIMIT: usize = usize::MAX", "task bound")
    workspace_ops = "crates/gc_cli_driver/src/pkg_workspace_ops.rs"
    source_mutation(workspace_ops, "crate::pkg_workspace_task::resolve(", "crate::pkg_task_runner::resolve_workspace_task_parity(", "production custody")
    native = "crates/gc_cli_driver/src/pkg_task_runner.rs"
    source_mutation(native, "pub(crate) fn resolve_workspace_task_parity(", "pub(crate) fn resolve_workspace_task(", "native reachability")
    route = "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs"
    source_mutation(route, "pkg_workspace_ops::resolve_workspace_task_for_run(cli, workspace_file, task)", "pkg_task_runner::resolve_workspace_task(workspace_file, task)", "route custody")
    tests = "crates/gc_cli/tests/cli_pkg_workspace.rs"
    source_mutation(tests, "gcpm_run_selfhost_authority_rejects_unsupported_task_command", "legacy_command_test", "command control")
    source_mutation(tests, "gcpm_run_selfhost_authority_rejects_ignored_package_arguments", "legacy_args_test", "argument control")
    source_mutation(tests, "gcpm_run_selfhost_authority_rejects_unavailable_engine_before_file_access", "legacy_engine_test", "engine control")
    ledger = "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json"
    source_mutation(ledger, profile["kind"], "native-workspace-task", "ledger authority")
    source_mutation(ledger, "Workspace-relative path joining, contract-file hashing, and typed action dispatch remain host mechanisms", "Workspace task behavior remains host-authoritative", "ledger residual")

    controls = 0
    for changed_profile, overrides, name in mutations:
        try:
            validate_all(root, changed_profile, schema, overrides)
        except CheckError:
            controls += 1
        else:
            fail(f"negative control survived: {name}")
    if controls != 28:
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
            "selfhost-pkg-workspace-task-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
    except CheckError as error:
        print(f"selfhost-pkg-workspace-task-authority: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
