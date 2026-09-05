#!/usr/bin/env python3
"""Independent custody verifier for the partial self-hosted commit authority."""

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
    "productionEntrypoints", "requestKind", "resultKind", "schema", "sourceModule",
    "sourceSha256", "spec", "version",
}
CONSTANTS = {
    "artifact": "selfhost/toolchain.gc",
    "binding": "core/commit::authority",
    "decisionInventory": [
        "native-commit-object-construction", "native-commit-object-field-closure",
        "native-commit-object-identity-admission", "native-commit-object-inspection-admission",
        "native-commit-author-metadata-construction", "vcs-history-commit-object-admission",
        "gpk-and-sync-closure-commit-admission",
        "package-commit-object-admission",
        "request-bound-result-verdict",
    ],
    "hostMechanisms": [
        "artifact-only-authority-bootstrap-and-bounded-evaluation", "cli-argument-transport",
        "ref-and-store-mechanisms", "patch-application-mechanism",
        "package-resolution-and-bridge-mechanisms",
        "artifact-hash-contradiction-check", "diagnostic-rendering",
    ],
    "hostOracle": {"parityOnly": True, "productionRequired": False, "removalTask": "R4.2.e"},
    "independentVerifier": "scripts/lib/selfhost_commit_authority.py",
    "kind": "genesis/selfhost-commit-authority-v0.1",
    "productionEntrypoints": ["genesis"],
    "requestKind": "genesis/commit-authority-request-v0.1",
    "resultKind": "genesis/commit-authority-result-v0.1",
    "schema": "docs/spec/SELFHOST_COMMIT_AUTHORITY_v0.1.schema.json",
    "sourceModule": "selfhost/commit_authority_v1.gc",
    "spec": "docs/spec/SELFHOST_COMMIT_AUTHORITY_v0.1.md",
    "version": "0.1.0",
}
NONCLAIMS = {
    "bootstrap-fixpoint", "h2-sd-canon-identity", "h2-sd-commit",
    "registry-commit-authority", "r4-2-e-closure",
    "release-qualification", "sh-c-closure", "wasi-commit-cli-authority",
}


def canonical_identity(profile) -> str:
    value = copy.deepcopy(profile)
    value.pop("contentIdentitySha256", None)
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


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


def read_text(root: Path, relative: str, overrides) -> str:
    return overrides.get(relative, (root / relative).read_text())


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
    source = read_text(root, source_relative, overrides)
    if source_identity(source_relative, source.encode()) != profile["sourceSha256"]:
        fail("authority source identity mismatch")
    require_all(source, [
        f"(def {profile['binding']}", profile["requestKind"], profile["resultKind"],
        "(quote :make)", "(quote :validate)", "selfhost/commit::commit?",
        "selfhost/store-authority::exact-map? request", "selfhost/store-authority::hash?",
        "selfhost/commit::allowed-fields-loop?", "constructed commit failed canonical validation",
        ":request-h (selfhost/hash::hash-term request)", "commit must use the closed canonical v1 schema",
    ], "GenesisCode commit authority")
    if "core/effect::" in source or "core/host::" in source:
        fail("commit authority contains an ambient effect or host operation")

    manifest_path = "selfhost/toolchain_manifest.gc"
    manifest = read_text(root, manifest_path, overrides)
    if manifest.count(f'"{source_relative}"') != 1 or manifest.count(profile["binding"]) != 1:
        fail("toolchain manifest custody drift")

    bridge_path = "crates/gc_effects/src/commit_authority.rs"
    bridge = read_text(root, bridge_path, overrides)
    require_all(bridge, [
        f'const BINDING: &str = "{profile["binding"]}"',
        f'const REQUEST_KIND: &str = "{profile["requestKind"]}"',
        f'const RESULT_KIND: &str = "{profile["resultKind"]}"',
        "pub struct CommitAuthority", "load_selfhost_coreform_toolchain_v1_with_mode(",
        "hex32(hash_term(&request))", "decode_result(value, &request_hash, expected_artifact)",
        "value.to_plain_term()", "result field set mismatch", "successful result artifact must be a map",
        "validation result substituted the submitted artifact",
        "strict_decoder_rejects_open_unbound_and_substituted_results",
        "strict_decoder_accepts_runtime_map_results",
        "pub(crate) fn validate_typed_commit(", "if !is_typed_commit(artifact)",
        "pub(crate) fn validate_expected_commit(", "pub(crate) fn validate_with_binding(",
        "obligation_name_terms",
        "typed_commit_classifier_is_exact_and_does_not_capture_other_objects",
    ], "Rust commit authority bridge")
    for default in ("unwrap_or_default()", "unwrap_or(true)", "unwrap_or(Term::Map"):
        if default in bridge:
            fail(f"commit authority bridge contains success-capable default {default!r}")

    adapter_path = "crates/gc_cli_driver/src/commit_authority.rs"
    adapter = read_text(root, adapter_path, overrides)
    require_all(adapter, [
        "resolve_selfhost_toolchain_bootstrap(cli)?", "gc_effects::CommitAuthority::load(",
        "load(cli)?.make(payload)", ".validate(artifact)",
        "CommitAuthorityError::Rejected", "CommitAuthorityError::Protocol",
    ], "native commit authority adapter")

    cmd_path = "crates/gc_cli_driver/src/cmd_commit.rs"
    cmd = read_text(root, cmd_path, overrides)
    require_all(cmd, [
        "commit_authority::make(", 'commit_authority::validate(cli, artifact, "commit/show")',
        'commit_authority::validate(cli, artifact, "commit/new base ref")',
        "commit_make_payload(", "mk_store_put_program(&artifact)",
    ], "native commit route")
    for residual in (
        "fn build_commit_artifact", "gc_vcs::Commit::from_term", "invalid --target-id: empty value",
        "invalid --message: empty value", "to_ascii_lowercase()",
    ):
        if residual in cmd:
            fail(f"native commit route retains host semantic residual {residual!r}")
    if cmd.index("commit_authority::make(") > cmd.index("mk_store_put_program(&artifact)"):
        fail("commit artifact is stored before authority construction")

    selfhost_consumer_path = "selfhost/cli_reachability_rules_v1.gc"
    selfhost_consumer = read_text(root, selfhost_consumer_path, overrides)
    require_all(selfhost_consumer, [
        "(def core/cli::vcs-validate-commit", "(core/commit::authority request)",
        "selfhost/store-authority::exact-map? result", "selfhost/hash::hash-term request",
        "commit authority substituted the submitted artifact",
    ], "self-hosted VCS history commit admission")

    dispatch_path = "crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs"
    dispatch = read_text(root, dispatch_path, overrides)
    history_path = "crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs"
    history = read_text(root, history_path, overrides)
    require_all(dispatch, [
        "let mut commit_authority = load_commit_authority(policy)?;",
        "vcs_load_commit(store, &mut commit_authority", "CommitAuthority::load_config(config)",
    ], "low-level VCS history commit route")
    if dispatch.count("let mut commit_authority = load_commit_authority(policy)?;") != 3:
        fail("low-level VCS history must load commit authority for log, blame, and why")
    require_all(history, [
        "commit_authority: &mut CommitAuthority", ".validate_commit(t.clone())",
    ], "VCS history traversal adapter")
    if "gc_vcs::Commit::from_term" in dispatch or "gc_vcs::Commit::from_term" in history:
        fail("VCS history retains native commit acceptance")

    gpk_route_path = "crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs"
    gpk_route = read_text(root, gpk_route_path, overrides)
    gpk_closure_path = "crates/gc_effects/src/runner_gc_ops.rs"
    gpk_closure = read_text(root, gpk_closure_path, overrides)
    sync_pull_path = "crates/gc_effects/src/runner_remote_ops/sync_closure_parallel.rs"
    sync_pull = read_text(root, sync_pull_path, overrides)
    sync_route_path = "crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs"
    sync_route = read_text(root, sync_route_path, overrides)
    require_all(gpk_route, [
        "CommitAuthority::validate_typed_commit(", "root_commit_admitted: root_commit.is_some()",
        '"core/gpk/bad-commit"', "&mut commit_authority",
    ], "GPK root commit admission")
    require_all(gpk_closure, [
        "CommitAuthority::validate_typed_commit(policy, commit_authority, &t)",
        '"core/gpk/bad-commit"',
    ], "GPK closure commit admission")
    require_all(sync_pull, [
        "CommitAuthority::validate_typed_commit(policy, commit_authority, &t)",
        '"core/sync/bad-commit"',
    ], "sync pull closure commit admission")
    require_all(sync_route, [
        "sync_closure_local(", "CommitAuthority::validate_typed_commit(policy, commit_authority, &t)",
        '"core/sync/bad-commit"', "let mut commit_authority = None;",
    ], "sync push closure commit admission")
    for path, route in (
        (gpk_route_path, gpk_route), (gpk_closure_path, gpk_closure),
        (sync_pull_path, sync_pull), (sync_route_path, sync_route),
    ):
        if "gc_vcs::Commit::from_term" in route:
            fail(f"{path} retains native commit acceptance")

    pkg_resolution_path = "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs"
    pkg_resolution = read_text(root, pkg_resolution_path, overrides)
    pkg_lock_validation_path = (
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs"
    )
    pkg_lock_validation = read_text(root, pkg_lock_validation_path, overrides)
    pkg_dispatch_path = "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs"
    pkg_dispatch = read_text(root, pkg_dispatch_path, overrides)
    pkg_workflow_path = (
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs"
    )
    pkg_workflow = read_text(root, pkg_workflow_path, overrides)
    pkg_install_path = (
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs"
    )
    pkg_install = read_text(root, pkg_install_path, overrides)
    pkg_verify_observation_path = (
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/"
        "install_verify/verify_observation.rs"
    )
    pkg_verify_observation = read_text(root, pkg_verify_observation_path, overrides)
    pkg_lock_read_path = "crates/gc_effects/src/pkg_lock_read_authority.rs"
    pkg_lock_read = read_text(root, pkg_lock_read_path, overrides)
    pkg_bridge_path = "crates/gc_effects/src/pkg_bridge_authority.rs"
    pkg_bridge = read_text(root, pkg_bridge_path, overrides)
    require_all(pkg_resolution, [
        "commit_authority: &mut Option<CommitAuthority>",
        "CommitAuthority::validate_expected_commit(policy, commit_authority, &t)",
    ], "package selector commit admission")
    if pkg_resolution.count(
            "CommitAuthority::validate_expected_commit(policy, commit_authority, &t)") != 3:
        fail("commit, ref, and semver package selectors must share commit authority")
    require_all(pkg_lock_validation, [
        "CommitAuthority::validate_expected_commit(policy, commit_authority, &commit_term)",
        "obligation_name_terms()",
    ], "package strict closure and provenance commit admission")
    require_all(pkg_dispatch, [
        "let mut commit_authority = None;", "&mut commit_authority",
    ], "package operation authority lifetime")
    require_all(pkg_workflow, [
        "pub(super) fn commit_observations(", "obligation_name_terms()",
        "CommitAuthority::validate_expected_commit(policy, commit_authority, &term)",
        "Result<Vec<Term>, Value>",
    ], "package workflow commit admission")
    require_all(pkg_install, [
        "CommitAuthority::validate_expected_commit(policy, commit_authority, &commit_term)",
        "observe_verify_commit_closure(", "commit_observations(",
    ], "package install commit admission")
    require_all(pkg_verify_observation, [
        "CommitAuthority::validate_expected_commit(policy, commit_authority, &commit_term)",
        "PkgVerifyClosureStatus::BadCommit",
    ], "package verify commit admission")
    require_all(pkg_lock_read, [
        '.get("core/commit::authority")',
        "commit_authority: Value",
    ], "package bridge commit binding custody")
    require_all(pkg_bridge, [
        "CommitAuthority::validate_with_binding(",
        "&self.commit_authority", "gc_vcs::commit_signing_hash(&commit.term)",
        "verify_commit_attestation(",
    ], "package bridge final commit admission")
    if pkg_bridge.count("CommitAuthority::validate_with_binding(") != 2:
        fail("package bridge production and positive control must both use commit authority")
    for path, route in (
        (pkg_resolution_path, pkg_resolution),
        (pkg_lock_validation_path, pkg_lock_validation),
        (pkg_workflow_path, pkg_workflow),
        (pkg_install_path, pkg_install),
        (pkg_verify_observation_path, pkg_verify_observation),
        (pkg_bridge_path, pkg_bridge),
    ):
        if "gc_vcs::Commit::from_term" in route:
            fail(f"{path} retains native package commit acceptance")

    effects_source_root = root / "crates/gc_effects/src"
    for source_path in effects_source_root.rglob("*.rs"):
        relative = source_path.relative_to(root).as_posix()
        if "gc_vcs::Commit::from_term" in read_text(root, relative, overrides):
            fail(f"{relative} reintroduces native commit acceptance")

    tests_path = "crates/gc_cli/tests/cli_commit.rs"
    tests = read_text(root, tests_path, overrides)
    require_all(tests, [
        "commit_new_and_show_roundtrip_with_ref_base_and_patch_file",
        "exercise self-hosted construction", "commit_show_rejects_open_commit_objects",
        'contains("core/vcs/bad-commit")',
    ], "native commit authority tests")
    vcs_tests_path = "crates/gc_cli/tests/cli_vcs_engine.rs"
    vcs_tests = read_text(root, vcs_tests_path, overrides)
    require_all(vcs_tests, [
        "vcs_log_rejects_open_commit_before_history_projection",
        '.stdout(predicate::str::contains("core/vcs/bad-commit"))',
        '.args(["--selfhost-artifact", artifact.to_str().unwrap()])',
    ], "VCS history commit authority tests")
    sync_tests_a_path = "crates/gc_effects/tests/sync_registry/cases_a.rs"
    sync_tests_a = read_text(root, sync_tests_a_path, overrides)
    sync_tests_b_path = "crates/gc_effects/tests/sync_registry/cases_b.rs"
    sync_tests_b = read_text(root, sync_tests_b_path, overrides)
    require_all(sync_tests_a, [
        "sync_push_rejects_open_typed_commit_before_remote_upload",
        "sync_pull_rejects_open_typed_commit_before_local_ref_update",
        '"core/sync/bad-commit"', "assert!(!reg.has(&commit_hash))",
        'assert_eq!(refs.get("refs/heads/open").unwrap(), None)',
    ], "sync commit authority negative controls")
    require_all(sync_tests_b, [
        "gpk_export_rejects_open_typed_commit_before_bundle_write",
        '"core/gpk/bad-commit"', "assert!(!output.exists())",
    ], "GPK commit authority negative control")
    pkg_tests_path = "crates/gc_cli/tests/cli_pkg_lock.rs"
    pkg_tests = read_text(root, pkg_tests_path, overrides)
    require_all(pkg_tests, [
        "pkg_lifecycle_rejects_open_typed_commit_without_lock_mutation",
        'for operation in ["lock", "update"]',
        'for operation in ["install", "verify"]',
        'contains("core/pkg/bad-commit")',
        "assert_eq!(fs::read(&lock_path).unwrap(), unresolved_lock)",
        "assert_eq!(fs::read(&lock_path).unwrap(), locked_bytes)",
        "gcpm_lock_and_install_emit_workspace_and_dependency_provenance",
        '.args(["verify"])',
    ], "package commit authority lifecycle controls")

    ledger = load_json(root / "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    rows = [row for row in ledger.get("semanticDecisions", []) if row.get("id") == "SD-COMMIT"]
    if len(rows) != 1:
        fail("SD-COMMIT ledger row missing or duplicated")
    row = rows[0]
    limitations = " ".join(row.get("limitations", []))
    if (row.get("currentLevel") != "H0" or source_relative not in row.get("producingImplementationPaths", [])
            or source_relative not in row.get("productionAuthorityPaths", [])
            or bridge_path not in row.get("productionAuthorityPaths", [])
            or profile["spec"] not in row.get("specAuthorityPaths", [])
            or profile["independentVerifier"] not in row.get("verifierPaths", [])
            or "registry commit" not in limitations
            or "package commit" not in limitations
            or gpk_route_path not in row.get("productionAuthorityPaths", [])
            or gpk_closure_path not in row.get("productionAuthorityPaths", [])
            or sync_pull_path not in row.get("productionAuthorityPaths", [])
            or sync_route_path not in row.get("productionAuthorityPaths", [])
            or pkg_resolution_path not in row.get("productionAuthorityPaths", [])
            or pkg_lock_validation_path not in row.get("productionAuthorityPaths", [])
            or pkg_workflow_path not in row.get("productionAuthorityPaths", [])
            or pkg_install_path not in row.get("productionAuthorityPaths", [])
            or pkg_verify_observation_path not in row.get("productionAuthorityPaths", [])
            or pkg_bridge_path not in row.get("productionAuthorityPaths", [])
            or pkg_tests_path not in row.get("testPaths", [])):
        fail("SD-COMMIT partial H0 custody drift")

    spec = read_text(root, profile["spec"], overrides)
    require_all(spec, [
        "This slice remains H0", "sole producer of canonical v1 commit construction",
        "Registry paths", "package resolution", "GPK export and sync push/pull closure traversal",
        "substitute a different artifact",
        "does not close `SD-COMMIT`", "permanent source/route mutations",
    ], "commit authority specification")

    if check_artifact:
        artifact = artifact_path or (root / profile["artifact"])
        data = artifact.read_bytes()
        if source_relative.encode() not in data or profile["binding"].encode() not in data:
            fail("commit authority source or binding absent from admitted artifact")


def mutation_controls(root: Path, profile) -> int:
    names = [
        profile["sourceModule"], "selfhost/toolchain_manifest.gc",
        "crates/gc_effects/src/commit_authority.rs",
        "crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs",
        "crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs",
        "crates/gc_effects/src/runner_gc_ops.rs",
        "crates/gc_effects/src/runner_remote_ops/sync_closure_parallel.rs",
        "crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs",
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs",
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/verify_observation.rs",
        "crates/gc_effects/src/pkg_lock_read_authority.rs",
        "crates/gc_effects/src/pkg_bridge_authority.rs",
        "crates/gc_cli_driver/src/commit_authority.rs", "crates/gc_cli_driver/src/cmd_commit.rs",
        "selfhost/cli_reachability_rules_v1.gc", "crates/gc_cli/tests/cli_commit.rs",
        "crates/gc_cli/tests/cli_vcs_engine.rs", "crates/gc_effects/tests/sync_registry/cases_a.rs",
        "crates/gc_effects/tests/sync_registry/cases_b.rs",
        "crates/gc_cli/tests/cli_pkg_lock.rs",
    ]
    paths = {name: (root / name).read_text() for name in names}
    source = paths[profile["sourceModule"]]
    manifest = paths["selfhost/toolchain_manifest.gc"]
    bridge = paths["crates/gc_effects/src/commit_authority.rs"]
    adapter = paths["crates/gc_cli_driver/src/commit_authority.rs"]
    cmd = paths["crates/gc_cli_driver/src/cmd_commit.rs"]
    dispatch = paths["crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs"]
    history = paths["crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs"]
    gpk_route = paths["crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs"]
    gpk_closure = paths["crates/gc_effects/src/runner_gc_ops.rs"]
    sync_pull = paths["crates/gc_effects/src/runner_remote_ops/sync_closure_parallel.rs"]
    sync_route = paths["crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs"]
    pkg_resolution = paths["crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs"]
    pkg_lock_validation = paths[
        "crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs"]
    pkg_dispatch = paths["crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs"]
    pkg_workflow = paths[
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs"]
    pkg_install = paths[
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs"]
    pkg_verify_observation = paths[
        "crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/verify_observation.rs"]
    pkg_lock_read = paths["crates/gc_effects/src/pkg_lock_read_authority.rs"]
    pkg_bridge = paths["crates/gc_effects/src/pkg_bridge_authority.rs"]
    selfhost_consumer = paths["selfhost/cli_reachability_rules_v1.gc"]
    tests = paths["crates/gc_cli/tests/cli_commit.rs"]
    vcs_tests = paths["crates/gc_cli/tests/cli_vcs_engine.rs"]
    sync_tests_a = paths["crates/gc_effects/tests/sync_registry/cases_a.rs"]
    sync_tests_b = paths["crates/gc_effects/tests/sync_registry/cases_b.rs"]
    pkg_tests = paths["crates/gc_cli/tests/cli_pkg_lock.rs"]
    mutations = [
        ({profile["sourceModule"]: source.replace("(quote :make)", "(quote :removed)", 1)}, "make operation"),
        ({profile["sourceModule"]: source.replace("(quote :validate)", "(quote :removed)", 1)}, "validate operation"),
        ({profile["sourceModule"]: source.replace("selfhost/store-authority::hash?", "selfhost/store-authority::str?")}, "identity admission"),
        ({profile["sourceModule"]: source.replace("selfhost/commit::allowed-fields-loop?", "selfhost/commit::removed-fields-loop?")}, "field closure"),
        ({profile["sourceModule"]: source.replace(":request-h (selfhost/hash::hash-term request)", ":request-h nil", 1)}, "request binding"),
        ({"selfhost/toolchain_manifest.gc": manifest.replace(f'    "{profile["sourceModule"]}"\n', "", 1)}, "module custody"),
        ({"selfhost/toolchain_manifest.gc": manifest.replace(f"    {profile['binding']}\n", "", 1)}, "binding custody"),
        ({"crates/gc_effects/src/commit_authority.rs": bridge.replace("value.to_plain_term()", "value.as_data().cloned()", 1)}, "runtime collection decoder"),
        ({"crates/gc_effects/src/commit_authority.rs": bridge.replace("result field set mismatch", "removed field closure", 1)}, "result closure"),
        ({"crates/gc_effects/src/commit_authority.rs": bridge.replace("validation result substituted the submitted artifact", "validation accepted substitution", 1)}, "artifact substitution"),
        ({"crates/gc_effects/src/commit_authority.rs": bridge.replace("if !is_typed_commit(artifact)", "if false")}, "typed commit classifier route"),
        ({"crates/gc_cli_driver/src/commit_authority.rs": adapter.replace("load(cli)?.make(payload)", "removed_authority_make(payload)", 1)}, "shared native adapter"),
        ({"crates/gc_cli_driver/src/cmd_commit.rs": cmd.replace("commit_authority::make(", "removed_authority::make(", 1)}, "construction route"),
        ({"crates/gc_cli_driver/src/cmd_commit.rs": cmd.replace('commit_authority::validate(cli, artifact, "commit/show")', "Ok(artifact)", 1)}, "inspection route"),
        ({"crates/gc_cli_driver/src/cmd_commit.rs": cmd.replace("Ok((base.to_string(), Vec::new()))", "Ok((base.to_ascii_lowercase(), Vec::new()))", 1)}, "host normalization"),
        ({"selfhost/cli_reachability_rules_v1.gc": selfhost_consumer.replace("(core/commit::authority request)", "commit", 1)}, "self-hosted VCS admission route"),
        ({"crates/gc_effects/src/runner_cap_vcs_low/dispatch_meta.rs": dispatch.replace("let mut commit_authority = load_commit_authority(policy)?;", "let mut commit_authority = removed_authority(policy)?;")}, "low-level VCS authority load"),
        ({"crates/gc_effects/src/runner_vcs_pkg_helpers/vcs_history.rs": history.replace(".validate_commit(t.clone())", ".removed_validate(t.clone())", 1)}, "VCS traversal validation"),
        ({"crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs": gpk_route.replace("CommitAuthority::validate_typed_commit(", "CommitAuthority::removed_typed_commit(", 1)}, "GPK root admission"),
        ({"crates/gc_effects/src/runner_gc_ops.rs": gpk_closure.replace("CommitAuthority::validate_typed_commit(policy, commit_authority, &t)", "CommitAuthority::removed_typed_commit(policy, commit_authority, &t)", 1)}, "GPK closure admission"),
        ({"crates/gc_effects/src/runner_remote_ops/sync_closure_parallel.rs": sync_pull.replace("CommitAuthority::validate_typed_commit(policy, commit_authority, &t)", "CommitAuthority::removed_typed_commit(policy, commit_authority, &t)", 1)}, "sync pull admission"),
        ({"crates/gc_effects/src/runner_remote_ops/sync_capabilities.rs": sync_route.replace("CommitAuthority::validate_typed_commit(policy, commit_authority, &t)", "CommitAuthority::removed_typed_commit(policy, commit_authority, &t)", 1)}, "sync push admission"),
        ({"crates/gc_effects/src/commit_authority.rs": bridge.replace("pub(crate) fn validate_expected_commit(", "pub(crate) fn removed_expected_commit(", 1)}, "expected commit adapter"),
        ({"crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs": pkg_resolution.replace("CommitAuthority::validate_expected_commit(policy, commit_authority, &t)", "CommitAuthority::removed_expected_commit(policy, commit_authority, &t)", 1)}, "package selector admission"),
        ({"crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs": pkg_lock_validation.replace("CommitAuthority::validate_expected_commit(policy, commit_authority, &commit_term)", "CommitAuthority::removed_expected_commit(policy, commit_authority, &commit_term)", 1)}, "package closure admission"),
        ({"crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs": pkg_dispatch.replace("let mut commit_authority = None;", "let mut removed_authority = None;", 1)}, "package operation authority lifetime"),
        ({"crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs": pkg_dispatch + "\n// gc_vcs::Commit::from_term\n"}, "gc_effects native parser exclusion"),
        ({"crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/workflow.rs": pkg_workflow.replace("CommitAuthority::validate_expected_commit(policy, commit_authority, &term)", "CommitAuthority::removed_expected_commit(policy, commit_authority, &term)", 1)}, "package workflow admission"),
        ({"crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs": pkg_install.replace("CommitAuthority::validate_expected_commit(policy, commit_authority, &commit_term)", "CommitAuthority::removed_expected_commit(policy, commit_authority, &commit_term)", 1)}, "package install admission"),
        ({"crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify/verify_observation.rs": pkg_verify_observation.replace("CommitAuthority::validate_expected_commit(policy, commit_authority, &commit_term)", "CommitAuthority::removed_expected_commit(policy, commit_authority, &commit_term)", 1)}, "package verify admission"),
        ({"crates/gc_effects/src/pkg_lock_read_authority.rs": pkg_lock_read.replace('.get("core/commit::authority")', '.get("removed/commit::authority")', 1)}, "package bridge binding custody"),
        ({"crates/gc_effects/src/pkg_bridge_authority.rs": pkg_bridge.replace("CommitAuthority::validate_with_binding(", "CommitAuthority::removed_validate_with_binding(", 1)}, "package bridge final admission"),
        ({"crates/gc_cli/tests/cli_commit.rs": tests.replace("commit_show_rejects_open_commit_objects", "removed_open_commit_control", 1)}, "negative control"),
        ({"crates/gc_cli/tests/cli_vcs_engine.rs": vcs_tests.replace("vcs_log_rejects_open_commit_before_history_projection", "removed_vcs_open_commit_control", 1)}, "VCS negative control"),
        ({"crates/gc_effects/tests/sync_registry/cases_a.rs": sync_tests_a.replace("sync_push_rejects_open_typed_commit_before_remote_upload", "removed_sync_push_open_commit_control", 1)}, "sync push negative control"),
        ({"crates/gc_effects/tests/sync_registry/cases_a.rs": sync_tests_a.replace("sync_pull_rejects_open_typed_commit_before_local_ref_update", "removed_sync_pull_open_commit_control", 1)}, "sync pull negative control"),
        ({"crates/gc_effects/tests/sync_registry/cases_b.rs": sync_tests_b.replace("gpk_export_rejects_open_typed_commit_before_bundle_write", "removed_gpk_open_commit_control", 1)}, "GPK negative control"),
        ({"crates/gc_cli/tests/cli_pkg_lock.rs": pkg_tests.replace("pkg_lifecycle_rejects_open_typed_commit_without_lock_mutation", "removed_pkg_open_commit_control", 1)}, "package lifecycle negative control"),
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
        "kind": profile["kind"], "mutationControls": controls, "ok": True,
        "sourceSha256": profile["sourceSha256"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CheckError as error:
        print(f"selfhost-commit-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
