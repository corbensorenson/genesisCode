#!/usr/bin/env python3
"""Independent custody verifier for gcpm workspace backend selection authority."""

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


SOURCE_MODULES = ["selfhost/pkg_workspace_env_select_authority_v1.gc"]
FIELDS = {
    "artifact", "auditDate", "binding", "contentIdentitySha256", "decisionInventory",
    "hostMechanisms", "hostOracle", "independentVerifier", "kind", "nonclaims",
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModules",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/pkg::workspace-env-select-authority",
    "decisionInventory": [
        "closed-runtime-backend-precedence",
        "unicode-trim-and-ascii-case-normalization",
        "canonical-profile-alias-normalization",
        "selected-source-attribution",
        "active-profile-normalization-and-compatibility",
        "request-bound-closed-selection-result",
    ],
    "hostMechanisms": [
        "bounded-workspace-toml-and-active-backend-observation",
        "non-backend-structural-workspace-admission",
        "artifact-only-bounded-authority-evaluation",
        "strict-request-bound-result-decoding",
        "environment-projection-and-materialization",
        "post-selection-task-resolution-and-dispatch",
    ],
    "hostOracle": {
        "productionRequired": False,
        "reachability": "none-proven",
        "removalTask": "R4.2.e",
    },
    "independentVerifier": "scripts/lib/selfhost_pkg_workspace_env_select_authority.py",
    "kind": "genesis/selfhost-pkg-workspace-env-select-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/pkg-workspace-env-select-authority-request-v0.1",
    "resultKind": "genesis/pkg-workspace-env-select-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_PKG_WORKSPACE_ENV_SELECT_AUTHORITY_v0.1.schema.json",
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
    "r4-2-e-closure",
    "release-qualification",
    "sh-c-closure",
    "task-resolution-authority",
    "wasi-workspace-env-support",
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
    module = text(root, SOURCE_MODULES[0], overrides)
    manifest = text(root, "selfhost/toolchain_manifest.gc", overrides)
    artifact = text(root, profile["artifact"], overrides)
    adapter = text(root, "crates/gc_cli_driver/src/pkg_workspace_env_select.rs", overrides)
    environment = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs", overrides)
    workspace_ops = text(root, "crates/gc_cli_driver/src/pkg_workspace_ops.rs", overrides)
    task_runner = text(root, "crates/gc_cli_driver/src/pkg_task_runner.rs", overrides)
    route = text(root, "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs", overrides)
    tests = text(root, "crates/gc_cli/tests/cli_pkg_workspace.rs", overrides)
    ledger = parse_json(
        text(root, "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json", overrides), "ledger"
    )

    require_markers(module, [
        "(def core/pkg::workspace-env-select-authority",
        profile["requestKind"], profile["resultKind"],
        "selfhost/policy::trim", "selfhost/effect-crypto::ascii-lower",
        "(def selfhost/pkg-workspace-env-select::choice",
        "(def selfhost/pkg-workspace-env-select::compatible?",
        '"profile-headless"', '"profile-gpu"', '"profile-gfx"', '"profile-backend"',
        "[:active :default :kind :override :profile :profile-backend :v]",
        "core/pkg/bad-workspace-env-selection",
    ], "GenesisCode workspace-env-select authority")
    for marker in (profile["binding"], *SOURCE_MODULES):
        if marker not in manifest:
            fail(f"toolchain manifest missing workspace-env-select marker: {marker}")
        if marker not in artifact:
            fail(f"published artifact missing workspace-env-select marker: {marker}")

    require_markers(adapter, [
        "pub(super) fn load_workspace(", 'parse::<toml::Table>()',
        'table.remove("runtime_backend")', "WorkspaceConfig::from_toml_str",
        "config.defaults.runtime_backend = default_runtime_backend.clone()",
        "profile.runtime_backend.clone_from(runtime_backend)",
        "pub(super) fn select_runtime_backend(", "crate::load_selfhost_toolchain",
        ".get(AUTHORITY_BINDING)", "decode_authorized(", "require_exact_fields(",
        'require_string(result, ":active", active_runtime_backend)',
        '":override" | ":profile" | ":default" | ":builtin"',
    ], "strict workspace-env-select adapter")
    if adapter.count("require_exact_fields(") != 3:
        fail("workspace-env-select exact-field check inventory drift")
    loader = adapter[adapter.index("pub(super) fn load_workspace("):adapter.index("fn take_runtime_backend(")]
    if "normalize_runtime_backend_profile" in loader or "runtime_backend_profile_is_compatible" in loader:
        fail("workspace observation loader contains backend semantic oracle")

    require_markers(environment, [
        "pkg_workspace_env_select::load_workspace(workspace_file, profile)",
        "pkg_workspace_env_select::select_runtime_backend(",
        "env_workspace.profile_runtime_backend.as_deref()",
        "env_workspace.default_runtime_backend.as_deref()",
        "let runtime_backend_compatible = selection.compatible",
        "std::fs::create_dir_all(&env_root)",
    ], "gcpm env production route")
    production = environment[
        environment.index("pub(crate) fn handle_env("):
        environment.index("struct RuntimeBackendContract")
    ]
    selector_at = production.find("pkg_workspace_env_select::select_runtime_backend(")
    materialize_at = production.find("std::fs::create_dir_all(&env_root)")
    if selector_at < 0 or materialize_at < 0 or selector_at >= materialize_at:
        fail("workspace-env selection no longer precedes materialization")
    for forbidden in ("resolve_env_runtime_backend_profile(", "normalize_runtime_backend_profile(",
                      "runtime_backend_profile_is_compatible("):
        if forbidden in production:
            fail(f"native workspace-env semantic fallback reachable: {forbidden}")

    require_markers(workspace_ops, [
        "pub(crate) fn prepare_workspace_for_run(",
        'pkg_workspace_env_select::load_workspace(workspace_file, "dev")',
        "pkg_workspace_env_select::select_runtime_backend(",
        "workspace.profile_runtime_backend.as_deref()",
        "workspace.default_runtime_backend.as_deref()",
        "if !selection.compatible",
        "Ok(workspace.config)",
    ], "gcpm run backend-admission route")
    run_admission = workspace_ops[
        workspace_ops.index("pub(crate) fn prepare_workspace_for_run("):
        workspace_ops.index("fn workspace_store_dir(")
    ]
    for forbidden in ("resolve_env_runtime_backend_profile(", "normalize_runtime_backend_profile(",
                      "runtime_backend_profile_is_compatible(", "WorkspaceConfig::load("):
        if forbidden in run_admission:
            fail(f"native gcpm-run backend semantic fallback reachable: {forbidden}")

    require_markers(task_runner, [
        "pub(crate) fn resolve_workspace_task(",
        "workspace: &WorkspaceConfig",
        "let task = workspace.tasks.get(task_name)",
    ], "post-selection workspace task mechanism")
    task_resolution = task_runner[
        task_runner.index("pub(crate) fn resolve_workspace_task("):
        task_runner.index("fn resolve_pkg_path(")
    ]
    if "WorkspaceConfig::load(" in task_resolution:
        fail("post-selection task resolution reloads workspace through native admission")

    require_markers(route, [
        "pkg_workspace_ops::handle_env(\n                cli,", "runtime_backend.as_deref()",
        "pkg_workspace_ops::prepare_workspace_for_run(cli, workspace_file)",
        "pkg_task_runner::resolve_workspace_task(workspace_file, &workspace, task)",
    ], "workspace-env CLI route")
    require_markers(tests, [
        "gcpm_env_runtime_backend_profile_contract_is_machine_readable",
        "gcpm_env_selection_override_masks_invalid_lower_precedence_values",
        "gcpm_env_selection_invalid_selected_backend_rejects_before_materialization",
        "gcpm_run_selection_masks_invalid_lower_precedence_backend",
        "gcpm_run_invalid_selected_backend_rejects_before_task_resolution",
        '"core/pkg/bad-workspace-env-selection"',
    ], "workspace-env integration evidence")

    rows = ledger.get("semanticDecisions")
    if not isinstance(rows, list):
        fail("ledger semanticDecisions missing")
    row = next((item for item in rows if item.get("id") == "SD-PACKAGE-WORKSPACE"), None)
    if not isinstance(row, dict):
        fail("ledger workspace decision missing")
    joined = json.dumps(row, sort_keys=True)
    require_markers(joined, [
        profile["kind"], profile["spec"], profile["independentVerifier"], *SOURCE_MODULES,
        "crates/gc_cli_driver/src/pkg_workspace_env_select.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops.rs",
        "crates/gc_cli_driver/src/pkg_task_runner.rs",
        "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs",
        "Artifact-loaded GenesisCode exclusively owns gcpm env and gcpm run backend-admission",
        "Workspace environment descriptor, projection, hashing, and materialization, general task-resolution, and manifest decisions remain host-authoritative",
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
        "crates/gc_cli_driver/src/pkg_workspace_env_select.rs",
        "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs",
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
        ("binding", "core/pkg::legacy-workspace-env-select"),
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

    source = SOURCE_MODULES[0]
    source_mutation(source, "(def core/pkg::workspace-env-select-authority", "(def core/pkg::legacy-env-select", "source")
    source_mutation(source, profile["requestKind"], "genesis/legacy-env-request", "request kind")
    source_mutation(source, "selfhost/policy::trim", "core/id", "trim normalization")
    source_mutation(source, "(def selfhost/pkg-workspace-env-select::choice", "(def selfhost/pkg-workspace-env-select::legacy-choice", "precedence")
    source_mutation(source, "(def selfhost/pkg-workspace-env-select::compatible?", "(def selfhost/pkg-workspace-env-select::always-compatible", "compatibility")
    source_mutation("selfhost/toolchain_manifest.gc", profile["binding"], "core/pkg::missing-env-select", "manifest")
    mutations.append((profile, {profile["artifact"]: sources[profile["artifact"]].replace(profile["binding"], "core/pkg::missing-env-select")}, "artifact"))
    adapter = "crates/gc_cli_driver/src/pkg_workspace_env_select.rs"
    source_mutation(adapter, "crate::load_selfhost_toolchain", "crate::load_legacy_toolchain", "artifact loader")
    source_mutation(adapter, "require_exact_fields(", "accept_open_fields(", "field closure")
    source_mutation(adapter, 'table.remove("runtime_backend")', 'table.get("runtime_backend")', "raw observation")
    source_mutation(adapter, "profile.runtime_backend.clone_from(runtime_backend)", "profile.runtime_backend = None", "raw restoration")
    environment = "crates/gc_cli_driver/src/pkg_workspace_ops_env.rs"
    source_mutation(environment, "pkg_workspace_env_select::select_runtime_backend(", "resolve_env_runtime_backend_profile(", "production selector")
    source_mutation(environment, "let runtime_backend_compatible = selection.compatible", "let runtime_backend_compatible = true", "compatibility transport")
    workspace_ops = "crates/gc_cli_driver/src/pkg_workspace_ops.rs"
    source_mutation(workspace_ops, "pkg_workspace_env_select::select_runtime_backend(", "resolve_env_runtime_backend_profile(", "run production selector")
    task_runner = "crates/gc_cli_driver/src/pkg_task_runner.rs"
    source_mutation(task_runner, "let task = workspace.tasks.get(task_name)", "let workspace = WorkspaceConfig::load(workspace_file).unwrap();\n    let task = workspace.tasks.get(task_name)", "workspace reload")
    route = "crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs"
    source_mutation(route, "pkg_workspace_ops::handle_env(\n                cli,", "pkg_workspace_ops::handle_env(\n                &Cli::default(),", "CLI custody")
    source_mutation(route, "pkg_workspace_ops::prepare_workspace_for_run(cli, workspace_file)", "pkg_workspace_ops::validate_workspace_runtime_backend_for_run(workspace_file)", "run authority custody")
    source_mutation(route, "pkg_task_runner::resolve_workspace_task(workspace_file, &workspace, task)", "pkg_task_runner::resolve_workspace_task(workspace_file, task)", "admitted workspace transport")
    tests = "crates/gc_cli/tests/cli_pkg_workspace.rs"
    source_mutation(tests, "gcpm_env_selection_override_masks_invalid_lower_precedence_values", "legacy_mask_test", "masking control")
    source_mutation(tests, "gcpm_env_selection_invalid_selected_backend_rejects_before_materialization", "legacy_reject_test", "rejection control")
    source_mutation(tests, "gcpm_run_selection_masks_invalid_lower_precedence_backend", "legacy_run_mask_test", "run masking control")
    source_mutation(tests, "gcpm_run_invalid_selected_backend_rejects_before_task_resolution", "legacy_run_reject_test", "run ordering control")
    ledger = "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json"
    source_mutation(ledger, profile["kind"], "native-workspace-env-select", "ledger authority")
    source_mutation(ledger, "Workspace environment descriptor, projection, hashing, and materialization", "Workspace environment", "ledger residual")

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
            "selfhost-pkg-workspace-env-select-authority: ok "
            f"profile={profile['contentIdentitySha256']} controls={controls}"
        )
    except CheckError as error:
        print(f"selfhost-pkg-workspace-env-select-authority: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
