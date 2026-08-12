#!/usr/bin/env python3
"""Independent verifier for the partial R4.2.d obligation authority."""

import argparse
import copy
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
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
    "hostFacts",
    "independentVerifier",
    "kind",
    "migratedObligations",
    "nonclaims",
    "productionEntrypoints",
    "residualObligations",
    "resultKind",
    "runtimeEvidence",
    "schema",
    "sourceModules",
    "sourceSetSha256",
    "spec",
    "version",
}

MIGRATED = [
    "core/obligation::ai-style",
    "core/obligation::budgets",
    "core/obligation::capabilities-declared",
    "core/obligation::concurrency-replay",
    "core/obligation::coverage",
    "core/obligation::coverage-decision",
    "core/obligation::coverage-mcdc",
    "core/obligation::determinism",
    "core/obligation::gfx-api-stability",
    "core/obligation::lint",
    "core/obligation::property-tests",
    "core/obligation::replayable-tests",
    "core/obligation::stage1-validation",
    "core/obligation::translation-validation",
    "core/obligation::typecheck",
    "core/obligation::typecheck-strict",
    "core/obligation::unit-tests",
]
RESIDUAL = [
    "core/obligation::gfx-frame-budgets",
    "core/obligation::gfx-golden-images",
    "core/obligation::preflight",
]

SOURCE_MODULES = [
    "selfhost/obligation_authority_core_v1.gc",
    "selfhost/obligation_authority_typecheck_v1.gc",
    "selfhost/obligation_authority_determinism_v1.gc",
    "selfhost/obligation_authority_lint_v1.gc",
    "selfhost/obligation_authority_ai_style_v1.gc",
    "selfhost/obligation_authority_replay_v1.gc",
    "selfhost/obligation_authority_property_v1.gc",
    "selfhost/obligation_authority_stage_v1.gc",
    "selfhost/obligation_authority_coverage_v1.gc",
    "selfhost/obligation_authority_translation_v1.gc",
    "selfhost/obligation_authority_gfx_api_v1.gc",
    "selfhost/obligation_authority_v1.gc",
]


def source_set_identity(root: Path, modules) -> str:
    digest = hashlib.sha256()
    digest.update(b"genesis/selfhost-obligation-authority-source-set-v0.1\0")
    for relative in modules:
        encoded_path = relative.encode()
        source = (root / relative).read_bytes()
        digest.update(len(encoded_path).to_bytes(8, "big"))
        digest.update(encoded_path)
        digest.update(len(source).to_bytes(8, "big"))
        digest.update(source)
    return digest.hexdigest()


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
        "binding": "core/cli::obligation-authority",
        "hostFacts": [
            "actual-value-hash",
            "canonical-module-forms",
            "coverage-decision-count",
            "coverage-decision-sample",
            "coverage-export-hit-count",
            "coverage-missing-effect-log-test",
            "coverage-per-test-instrumentation",
            "coverage-statement-site-hit-count",
            "coverage-test-count",
            "effect-entry-count",
            "effect-log-artifact",
            "effect-log-byte-count",
            "effect-log-entry-await-edge",
            "effect-log-entry-operation",
            "effect-log-entry-position",
            "effect-log-entry-schedule-step",
            "effect-log-entry-task-id",
            "effect-program-status",
            "expected-value-hash",
            "gfx-api-configured-export-symbol",
            "gfx-api-configured-surface-hash",
            "gfx-api-definition-expression-hash",
            "gfx-api-export-symbol",
            "module-path",
            "observed-effect-operation",
            "optimizer-original-evaluation-error",
            "optimizer-original-module-hash",
            "optimizer-original-value-hash",
            "optimizer-stat-counter",
            "optimizer-transformed-evaluation-error",
            "optimizer-transformed-module-hash",
            "optimizer-transformed-value-hash",
            "optimized-test-value-hash",
            "property-body-callable-status",
            "property-cases-value",
            "property-entry-shape",
            "property-execution-result",
            "property-suite-state",
            "replay-value-hash",
            "sealed-error-status",
            "stage2-mechanism-error",
            "stage2-module-hash",
            "stage2-original-value-hash",
            "stage2-result-equality",
            "stage2-status",
            "stage2-value-kind",
            "stage2-wasm-byte-count",
            "stage2-wasm-hash",
            "stage2-wasm-value-hash",
            "step-count",
            "test-identity",
        ],
        "independentVerifier": "scripts/lib/selfhost_obligation_authority.py",
        "kind": "genesis/selfhost-obligation-authority-v0.1",
        "migratedObligations": MIGRATED,
        "productionEntrypoints": ["genesis", "genesis_wasi"],
        "residualObligations": RESIDUAL,
        "resultKind": "genesis/obligation-authority-result-v0.2",
        "runtimeEvidence": {
            "allocationLimit": 10_000_000,
            "stepLimit": 5_000_000,
            "timeoutSeconds": 60,
        },
        "schema": "docs/spec/SELFHOST_OBLIGATION_AUTHORITY_v0.1.schema.json",
        "sourceModules": SOURCE_MODULES,
        "spec": "docs/spec/SELFHOST_OBLIGATION_AUTHORITY_v0.1.md",
        "version": "0.1.0",
    }
    for key, expected in constants.items():
        if profile.get(key) != expected:
            fail(f"profile {key} drift")
    expected_nonclaims = {
        "bootstrap-fixpoint",
        "effect-policy-authority",
        "evidence-verification-authority",
        "r4-2-d-closure",
        "release-qualification",
        "effect-replay-execution-authority",
        "sd-obligation-h2",
        "signing-authority",
    }
    if set(profile.get("nonclaims", [])) != expected_nonclaims:
        fail("nonclaim inventory drift")
    for key in ("contentIdentitySha256", "sourceSetSha256"):
        if not re.fullmatch(r"[0-9a-f]{64}", str(profile.get(key, ""))):
            fail(f"invalid {key}")
    if check_identity and profile["contentIdentitySha256"] != identity(profile):
        fail("profile content identity mismatch")


def cargo_tree(root: Path, package: str) -> str:
    result = subprocess.run(
        ["cargo", "tree", "-p", package, "-e", "features", "--locked", "--offline"],
        cwd=root,
        text=True,
        capture_output=True,
    )
    if result.returncode:
        fail(f"cargo tree failed for {package}: {result.stderr.strip()}")
    return result.stdout


def validate_bridge(bridge: str) -> None:
    required = [
        'env.get("core/cli::obligation-authority")',
        "evaluate_obligation_with_authority(",
        "unit_test_observations(",
        "budget_observations(",
        "capability_inputs(",
        "typecheck_inputs(",
        "validate_unit_report(",
        "validate_budget_report(",
        "validate_capabilities_report(",
        "validate_determinism_report(",
        "validate_lint_report(",
        "validate_ai_style_report(",
        "validate_replay_report(",
        "property_authority_context(",
        "property_authority_plan(",
        "property_authority_finalize(",
        "decode_property_plan_result(",
        "evaluate_stage1_obligation_with_authority(",
        "decode_stage1_result(",
        "stage1_inputs(",
        "evaluate_coverage_obligation_with_authority(",
        "decode_coverage_result(",
        "evaluate_translation_obligation_with_authority(",
        "decode_translation_result(",
        "translation_inputs(",
        "replay_observations(",
        "run_replay_authority(",
        "decode_artifact_transport(",
        "expected_lint_patch(",
        "validate_typecheck_obligation_report(",
        "strict_typecheck_meta_for_validation(",
        "ObligationAuthorityOperation::Determinism",
        "ObligationAuthorityOperation::Lint",
        "ObligationAuthorityOperation::AiStyle",
        "ObligationAuthorityOperation::ReplayableTests",
        "ObligationAuthorityOperation::ConcurrencyReplay",
        "ObligationAuthorityOperation::PropertyTests",
        "ObligationAuthorityOperation::TranslationValidation",
        "ObligationAuthorityOperation::GfxApiStability",
        "ObligationAuthorityOperation::TypecheckStrict",
        'Term::symbol(":request-h")',
        "let request_hash = hash_term(&request);",
        "obligation_authority_rejects_result_bound_to_another_request",
        "if frontend_is_rust(frontend)",
        "resolved_authority_frontend = default_coreform_frontend();",
        "rust_frontend_selection_does_not_replace_selfhost_obligation_authority",
        "lint_and_ai_style_authorities_decide_and_persist_closed_artifacts",
        "lint_authority_rejects_side_artifact_and_final_report_tampering",
        "replay_authorities_decide_from_closed_host_observations",
        "replay_authority_rejects_open_observations_and_contradictory_reports",
        "property_authority_plans_exact_seeds_and_rejects_seed_tampering",
        "stage1_authority_aggregates_failures_and_rejects_report_tampering",
        "stage1_eval_observation_obeys_caller_step_limit",
        "coverage_authority_decides_profiles_and_rejects_open_observations",
        "coverage_decoder_rejects_request_and_report_tampering",
        "translation_authority_decides_complete_divergent_and_no_test_observations",
        "translation_authority_rejects_open_substituted_and_tampered_results",
        "gfx_api_authority_decides_surface_export_and_definition_facts",
        "gfx_api_authority_rejects_open_substituted_and_tampered_results",
        "evaluate_gfx_api_obligation_with_authority(",
        "decode_gfx_api_result(",
    ]
    for token in required:
        if token not in bridge:
            fail(f"missing obligation authority boundary: {token}")


def static_check(root: Path, profile):
    sources = []
    for relative in profile["sourceModules"]:
        source_path = root / relative
        if source_path.is_symlink() or not source_path.is_file() or root not in source_path.resolve().parents:
            fail("obligation authority source is missing, escaping, or symlinked")
        if len(source_path.read_text().splitlines()) > 700:
            fail("obligation authority source exceeds 700-line decomposition ceiling")
        sources.append(source_path.read_text())
    source_hash = source_set_identity(root, profile["sourceModules"])
    if source_hash != profile["sourceSetSha256"]:
        fail("obligation authority source-set identity mismatch")
    combined_sources = "\n".join(sources)
    if "core/cli::typecheck-package" not in combined_sources:
        fail("self-host typecheck obligation route is absent")
    if "selfhost/obligation::determinism" not in combined_sources:
        fail("self-host determinism obligation route is absent")
    if "selfhost/obligation::lint" not in combined_sources:
        fail("self-host lint obligation route is absent")
    if "selfhost/obligation::ai-style" not in combined_sources:
        fail("self-host AI-style obligation route is absent")
    if "selfhost/obligation::replay-authority" not in combined_sources:
        fail("self-host replay obligation route is absent")
    if "selfhost/obligation::property-authority" not in combined_sources:
        fail("self-host property obligation route is absent")
    if "selfhost/obligation::stage1-validation" not in combined_sources:
        fail("self-host stage1 obligation route is absent")
    if "selfhost/obligation::coverage" not in combined_sources:
        fail("self-host coverage obligation route is absent")
    if "selfhost/obligation::translation-validation" not in combined_sources:
        fail("self-host translation-validation obligation route is absent")
    if "selfhost/obligation::gfx-api-stability" not in combined_sources:
        fail("self-host gfx API obligation route is absent")
    manifest = (root / "selfhost/toolchain_manifest.gc").read_text()
    positions = []
    for relative in profile["sourceModules"]:
        if manifest.count(f'"{relative}"') != 1:
            fail("obligation authority source manifest custody drift")
        positions.append(manifest.index(f'"{relative}"'))
    if positions != sorted(positions):
        fail("obligation authority source manifest order drift")
    if manifest.count(profile["binding"]) != 1:
        fail("obligation authority binding manifest custody drift")

    bridge = (root / "crates/gc_obligations/src/obligation_authority.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_caps.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_lint.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_replay.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_property.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_property_finalize.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_stage.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_coverage.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_translation.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_translation_finalize.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_gfx_api.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_gfx_api.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_translation.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_exec_coverage.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_exec_coverage_finalize.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_exec_replay.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_exec_tests.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_tests.rs").read_text()
    validate_bridge(bridge)
    types_api = (root / "crates/gc_obligations/src/obligations/types_api.rs").read_text()
    if types_api.count("obligation_unit_tests(&store, &manifest, &test_runs, &frontend, limits)") != 1:
        fail("unit-test production authority call-site drift")
    if types_api.count("obligation_budgets(&store, &manifest, &test_runs, &frontend, limits)") != 1:
        fail("budget production authority call-site drift")
    if types_api.count("obligation_caps_declared(") != 1:
        fail("capabilities-declared production authority call-site drift")
    if types_api.count(
        "obligation_typecheck(&store, &manifest, &modules, &frontend, limits, false)"
    ) != 1:
        fail("typecheck production authority call-site drift")
    if types_api.count(
        "obligation_typecheck(&store, &manifest, &modules, &frontend, limits, true)"
    ) != 1:
        fail("strict typecheck production authority call-site drift")
    if types_api.count(
        "obligation_determinism(&store, &manifest, &modules, &test_runs, &frontend, limits)"
    ) != 1:
        fail("determinism production authority call-site drift")
    if types_api.count("obligation_lint(&store, &manifest, &modules, &frontend, limits)") != 1:
        fail("lint production authority call-site drift")
    if types_api.count("obligation_ai_style(&store, &manifest, &modules, &frontend, limits)") != 1:
        fail("AI-style production authority call-site drift")
    if types_api.count("replay_observations(") != 1:
        fail("replay host-observation collection call-site drift")
    if types_api.count("run_replay_authority(") != 2:
        fail("replay authority production dispatch drift")
    if types_api.count(
        "obligation_property_tests(&store, &pkg_dir, &manifest, &modules, &frontend, limits)"
    ) != 1:
        fail("property-test authority production dispatch drift")
    if types_api.count(
        "obligation_stage1_validation(&store, &manifest, &modules, &frontend, limits)"
    ) != 1:
        fail("stage1 authority production dispatch drift")
    if types_api.count("obligation_coverage(CoverageRunArgs {") != 3:
        fail("coverage authority production dispatch drift")
    if types_api.count("obligation_translation_validation(") != 1:
        fail("translation authority production dispatch drift")
    if types_api.count(
        "obligation_gfx_api_stability(&store, &manifest, &modules, &frontend, limits)"
    ) != 1:
        fail("gfx API authority production dispatch drift")
    unit_host = (root / "crates/gc_obligations/src/obligation_exec.rs").read_text()
    budget_host = (root / "crates/gc_obligations/src/obligation_exec_budgets.rs").read_text()
    test_host = (root / "crates/gc_obligations/src/obligations/test_exec.rs").read_text()
    stage_host = (root / "crates/gc_obligations/src/obligation_stage.rs").read_text()
    translation_host = (root / "crates/gc_obligations/src/obligation_translation.rs").read_text()
    coverage_execution = (root / "crates/gc_obligations/src/obligation_exec_coverage.rs").read_text()
    coverage_host = coverage_execution + (
        root / "crates/gc_obligations/src/obligation_exec_coverage_finalize.rs"
    ).read_text()
    lint_host = (root / "crates/gc_obligations/src/obligation_lint.rs").read_text()
    replay_host = (root / "crates/gc_obligations/src/obligation_exec_replay.rs").read_text()
    property_host = (root / "crates/gc_obligations/src/obligation_exec_tests.rs").read_text()
    gfx_api_host = (root / "crates/gc_obligations/src/obligation_gfx_api.rs").read_text()
    gfx_residual_host = (root / "crates/gc_obligations/src/obligation_gfx.rs").read_text()
    forbidden = [
        "t.steps >",
        "t.effect_entries >",
        "t.effect_log_bytes >",
        '" exceeded max_steps_per_test: "',
        "fv_hash == expected_hash",
        "tr.ok",
        "declares :caps [] but has inferred effects",
        "performed effects but module declares :caps []",
        "lint_autofix_patch_for_module",
        "strict_warning_codes",
        'env.get("core/editor/lint::lint-module")',
        "expected effect program for replayability",
        "expected effect program for concurrency replay",
        "concurrency log mismatch",
        "concurrency log missing",
        "replay mismatch for",
    ]
    combined = unit_host + budget_host + test_host + stage_host + translation_host + lint_host + replay_host
    for token in forbidden:
        if token in combined:
            fail(f"reachable host obligation decision restored: {token}")
    if unit_host.count("ObligationAuthorityOperation::UnitTests") != 1:
        fail("unit-test authority dispatch drift")
    if budget_host.count("ObligationAuthorityOperation::Budgets") != 1:
        fail("budget authority dispatch drift")
    if unit_host.count("ObligationAuthorityOperation::CapabilitiesDeclared") != 1:
        fail("capabilities-declared authority dispatch drift")
    if len(re.findall(r"ObligationAuthorityOperation::Typecheck(?!Strict)", unit_host)) != 1:
        fail("typecheck authority dispatch drift")
    if unit_host.count("ObligationAuthorityOperation::TypecheckStrict") != 1:
        fail("strict typecheck authority dispatch drift")
    if unit_host.count("ObligationAuthorityOperation::Determinism") != 1:
        fail("determinism authority dispatch drift")
    if lint_host.count("ObligationAuthorityOperation::Lint") != 1:
        fail("lint authority dispatch drift")
    if lint_host.count("ObligationAuthorityOperation::AiStyle") != 1:
        fail("AI-style authority dispatch drift")
    if replay_host.count("gc_effects::replay_with_store(") != 1:
        fail("replay execution host-fact collection drift")
    if "ReplayEntryObservation" not in replay_host or "ReplayObservation" not in replay_host:
        fail("closed replay observation transport drift")
    property_execution = property_host.split("pub(super) fn obligation_property_tests(", 1)[1].split(
        "pub(super) fn is_callable_value", 1
    )[0]
    for token in ('"genesis/property-tests-v0.2"', "seed_for_case(", "parse_property_entry("):
        if token in property_execution:
            fail(f"reachable host property policy restored: {token}")
    stage1_execution = stage_host.split("pub(super) fn obligation_stage1_validation(", 1)[1].split(
        "pub(super) struct PackageEval", 1
    )[0]
    for token in (
        "gc_opt::stage1_pipeline(",
        ".gate_report",
        "pure value hash mismatch after stage1 transform",
        "original module is not gate-valid:",
        "transformed module is not gate-valid:",
    ):
        if token in stage1_execution:
            fail(f"reachable host stage1 policy restored: {token}")
    if stage1_execution.count("evaluate_stage1_obligation_with_authority(") != 1:
        fail("stage1 authority invocation drift")
    if coverage_host.count("evaluate_coverage_obligation_with_authority(") != 1:
        fail("coverage authority invocation drift")
    if translation_host.count("evaluate_translation_obligation_with_authority(") != 2:
        fail("translation authority invocation drift")
    if "store.put_term(" in translation_host:
        fail("reachable host translation report persistence restored")
    for token in (
        '"genesis/translation-validation-v0.2"',
        '"hash mismatch for ',
        '"stage2 wasm result differs from kernel result"',
        '"stage2 wasm value hash mismatch"',
    ):
        if token in translation_host:
            fail(f"reachable host translation policy restored: {token}")
    if "store.put_term(&report)" in coverage_host:
        fail("reachable host coverage report persistence restored")
    for token in (
        "export not covered:",
        "statement-site coverage missing",
        "decision coverage missing branch outcomes",
        "mcdc coverage missing condition independence",
    ):
        if token in coverage_execution:
            fail(f"reachable coverage execution policy restored: {token}")
    obligation_typecheck = unit_host.split("pub(super) fn obligation_typecheck(", 1)[1].split(
        "pub(super) fn typecheck_report_with_frontend(", 1
    )[0]
    if "typecheck_report_with_frontend(" in obligation_typecheck:
        fail("reachable host strict typecheck obligation decision restored")
    obligation_determinism = unit_host.split("pub(super) fn obligation_determinism(", 1)[1].split(
        "pub(super) fn obligation_caps_declared(", 1
    )[0]
    if (
        "typecheck_report_with_frontend(" in obligation_determinism
        or "meta_caps(" in obligation_determinism
        or "suite_to_module(" in obligation_determinism
    ):
        fail("reachable host determinism obligation decision restored")
    if (
        '"core/obligation::capabilities-declared-report"' in unit_host
        or "did not declare it in :caps" in unit_host
    ):
        fail("reachable host capability-declaration decision restored")
    if "ObligationAuthorityOperation::UnitTests" in translation_host:
        fail("translation execution restored a host-selected unit-test verdict")
    if gfx_api_host.count("evaluate_gfx_api_obligation_with_authority(") != 1:
        fail("gfx API authority invocation drift")
    for token in (
        "api-stability-analysis",
        "api-stability-report",
        '"genesis/gfx-api-stability-v0.2"',
        '"gfx API surface hash mismatch"',
        '"missing exported gfx symbols"',
        '"unexpected exported gfx symbols"',
        '"no tracked gfx API exports found"',
    ):
        if token in gfx_api_host or token in gfx_residual_host:
            fail(f"reachable host gfx API policy restored: {token}")
    if "store.put_term(" in gfx_api_host:
        fail("reachable host gfx API report persistence restored")
    for package in ("gc_cli", "gc_wasi_cli"):
        tree = cargo_tree(root, package)
        if 'gc_obligations feature "parity-oracle"' in tree:
            fail(f"{package} production graph activates obligation parity oracle")
    return {"migrated": len(MIGRATED), "residual": len(RESIDUAL), "sourceSetSha256": source_hash}


def run_case(binary: Path, artifact: Path, root: Path, fixture: str, profile):
    limits = profile["runtimeEvidence"]
    with tempfile.TemporaryDirectory(prefix="genesis-obligation-runtime-") as temp:
        fixture_copy = Path(temp) / Path(fixture).name
        shutil.copytree(root / fixture, fixture_copy)
        if fixture == "tests/spec/pkg_gpu_parallel_obligations":
            bridge = fixture_copy / "host_bridge.sh"
            bridge.write_text(
                "#!/bin/sh\n"
                "IFS= read -r request_len || exit 2\n"
                'dd bs=1 count="$request_len" of=/dev/null 2>/dev/null || exit 2\n'
                "resp='{:ok true :id \"gpu-bridge-0\" :data b\"\\x01\\x02\\x03\\x04\" :written 4}'\n"
                'printf \'%s\\n%s\' "${#resp}" "$resp"\n'
            )
            bridge.chmod(0o755)
            with (fixture_copy / "caps.toml").open("a") as policy:
                for operation in (
                    "gfx/gpu::create-buffer",
                    "gfx/gpu::write-buffer",
                    "gfx/gpu::read-buffer",
                    "gfx/gpu::destroy-resource",
                ):
                    policy.write(
                        f'\n[op."{operation}"]\n'
                        'base_dir = "."\n'
                        'bridge_cmd = "host_bridge.sh"\n'
                    )
        result = subprocess.run(
            [
                str(binary),
                "test",
                "--pkg",
                str(fixture_copy / "package.toml"),
                "--selfhost-artifact",
                str(artifact),
                "--step-limit",
                str(limits["stepLimit"]),
                "--max-alloc-units",
                str(limits["allocationLimit"]),
                "--json",
            ],
            cwd=(fixture_copy if fixture == "tests/spec/pkg_gpu_parallel_obligations" else root),
            text=True,
            capture_output=True,
            timeout=limits["timeoutSeconds"],
            env={**os.environ, "GENESIS_OBLIGATION_CACHE_DISABLE": "1"},
        )
    try:
        envelope = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        fail(f"invalid JSON from {binary.name}/{fixture}: {error}: {result.stderr.strip()}")
    facts = []
    for item in envelope.get("data", {}).get("obligations", []):
        if item.get("name") in MIGRATED:
            facts.append((item.get("name"), item.get("ok"), item.get("errors")))
    return result.returncode, envelope.get("ok"), facts


def runtime_check(root: Path, profile, binaries, artifact_override=None):
    artifact = (
        artifact_override.resolve()
        if artifact_override is not None
        else (root / profile["artifact"]).resolve()
    )
    if not artifact.is_file():
        fail(f"runtime self-host artifact is not a file: {artifact}")
    fixtures = [
        (
            "tests/spec/pkg_basic",
            0,
            True,
            [
                "core/obligation::unit-tests",
                "core/obligation::determinism",
                "core/obligation::capabilities-declared",
                "core/obligation::replayable-tests",
                "core/obligation::typecheck",
                "core/obligation::stage1-validation",
                "core/obligation::translation-validation",
            ],
        ),
        ("tests/spec/pkg_fail_unit", 30, False, ["core/obligation::unit-tests"]),
        (
            "tests/spec/pkg_fail_budgets",
            30,
            False,
            ["core/obligation::unit-tests", "core/obligation::budgets"],
        ),
        (
            "tests/spec/pkg_fail_caps_declared",
            30,
            False,
            [
                "core/obligation::unit-tests",
                "core/obligation::capabilities-declared",
            ],
        ),
        (
            "tests/spec/pkg_fail_determinism",
            30,
            False,
            ["core/obligation::unit-tests", "core/obligation::determinism"],
        ),
        (
            "tests/spec/pkg_fail_typecheck",
            30,
            False,
            ["core/obligation::typecheck"],
        ),
        (
            "tests/spec/pkg_typecheck_strict",
            0,
            True,
            ["core/obligation::typecheck-strict"],
        ),
        (
            "tests/spec/pkg_fail_typecheck_strict",
            30,
            False,
            ["core/obligation::typecheck-strict"],
        ),
        ("tests/spec/pkg_lint", 0, True, ["core/obligation::lint"]),
        ("tests/spec/pkg_fail_lint", 30, False, ["core/obligation::lint"]),
        ("tests/spec/pkg_lint_autofix", 0, True, ["core/obligation::lint"]),
        ("tests/spec/pkg_ai_style", 0, True, ["core/obligation::ai-style"]),
        (
            "tests/spec/pkg_fail_ai_style",
            30,
            False,
            ["core/obligation::ai-style"],
        ),
        (
            "tests/spec/pkg_property_tests",
            0,
            True,
            ["core/obligation::property-tests"],
        ),
        (
            "tests/spec/pkg_fail_property_tests",
            30,
            False,
            ["core/obligation::property-tests"],
        ),
        (
            "tests/spec/pkg_gpu_parallel_obligations",
            0,
            True,
            [
                "core/obligation::unit-tests",
                "core/obligation::capabilities-declared",
                "core/obligation::replayable-tests",
                "core/obligation::concurrency-replay",
            ],
        ),
        (
            "tests/obligation/coverage_profiles",
            0,
            True,
            [
                "core/obligation::unit-tests",
                "core/obligation::coverage",
                "core/obligation::coverage-decision",
                "core/obligation::coverage-mcdc",
            ],
        ),
        (
            "tests/spec/pkg_fail_coverage",
            30,
            False,
            ["core/obligation::unit-tests", "core/obligation::coverage"],
        ),
        (
            "tests/spec/pkg_gfx_obligations",
            0,
            True,
            ["core/obligation::gfx-api-stability"],
        ),
        (
            "tests/spec/pkg_fail_gfx_api",
            30,
            False,
            ["core/obligation::gfx-api-stability"],
        ),
    ]
    all_observations = []
    for binary in binaries:
        binary = binary.resolve()
        if not binary.is_file() or not os.access(binary, os.X_OK):
            fail(f"runtime entrypoint is not executable: {binary}")
        observations = []
        for fixture, expected_exit, expected_ok, expected_names in fixtures:
            observed = run_case(binary, artifact, root, fixture, profile)
            if observed[0] != expected_exit or observed[1] is not expected_ok:
                fail(f"{binary.name}/{fixture} disposition drift: {observed[:2]}")
            if [fact[0] for fact in observed[2]] != expected_names:
                fail(f"{binary.name}/{fixture} migrated obligation inventory drift: {observed[2]}")
            observations.append(observed)
        all_observations.append(observations)
    if any(observations != all_observations[0] for observations in all_observations[1:]):
        fail("native/WASI obligation authority observations differ")
    return all_observations[0]


def self_test(root: Path, profile, schema):
    mutations = []
    for label, mutate in [
        ("binding", lambda p: p.__setitem__("binding", "core/cli::wrong")),
        ("migrated", lambda p: p.__setitem__("migratedObligations", MIGRATED[:1])),
        ("residual", lambda p: p.__setitem__("residualObligations", RESIDUAL[:-1])),
        ("host-facts", lambda p: p.__setitem__("hostFacts", p["hostFacts"][:-1])),
        ("source", lambda p: p.__setitem__("sourceSetSha256", "0" * 64)),
        ("source-inventory", lambda p: p.__setitem__("sourceModules", p["sourceModules"][:-1])),
        ("nonclaim", lambda p: p.__setitem__("nonclaims", p["nonclaims"][:-1])),
    ]:
        candidate = copy.deepcopy(profile)
        mutate(candidate)
        candidate["contentIdentitySha256"] = identity(candidate)
        try:
            validate(candidate, schema)
            if label == "source":
                static_check(root, candidate)
        except CheckError:
            mutations.append(label)
        else:
            fail(f"mutation was not rejected: {label}")
    bridge = (root / "crates/gc_obligations/src/obligation_authority.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_caps.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_lint.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_property.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_property_finalize.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_stage.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_coverage.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_translation.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_translation_finalize.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_gfx_api.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_gfx_api.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_translation.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_exec_coverage.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_exec_coverage_finalize.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_exec_tests.rs").read_text()
    bridge += (root / "crates/gc_obligations/src/obligation_authority_tests.rs").read_text()
    redirected = "resolved_authority_frontend = default_coreform_frontend();"
    try:
        validate_bridge(bridge.replace(redirected, "", 1))
    except CheckError:
        mutations.append("rust-frontend-redirection")
    else:
        fail("mutation was not rejected: rust-frontend-redirection")
    request_binding = "let request_hash = hash_term(&request);"
    try:
        validate_bridge(bridge.replace(request_binding, "", 1))
    except CheckError:
        mutations.append("request-result-binding")
    else:
        fail("mutation was not rejected: request-result-binding")
    typecheck_route = "validate_typecheck_obligation_report("
    try:
        validate_bridge(bridge.replace(typecheck_route, ""))
    except CheckError:
        mutations.append("typecheck-route")
    else:
        fail("mutation was not rejected: typecheck-route")
    determinism_route = "validate_determinism_report("
    try:
        validate_bridge(bridge.replace(determinism_route, ""))
    except CheckError:
        mutations.append("determinism-route")
    else:
        fail("mutation was not rejected: determinism-route")
    for label, route in [
        ("lint-route", "validate_lint_report("),
        ("ai-style-route", "validate_ai_style_report("),
        ("artifact-transport", "decode_artifact_transport("),
        ("lint-patch-reconstruction", "expected_lint_patch("),
        ("replay-route", "validate_replay_report("),
        ("replay-host-facts", "replay_observations("),
        ("property-plan-route", "decode_property_plan_result("),
        ("property-host-facts", "property_authority_context("),
        ("stage1-route", "decode_stage1_result("),
        ("stage1-host-facts", "stage1_inputs("),
        ("coverage-route", "decode_coverage_result("),
        ("coverage-host-facts", "evaluate_coverage_obligation_with_authority("),
        ("translation-route", "decode_translation_result("),
        ("translation-host-facts", "translation_inputs("),
        ("gfx-api-route", "decode_gfx_api_result("),
        ("gfx-api-host-facts", "evaluate_gfx_api_obligation_with_authority("),
    ]:
        try:
            validate_bridge(bridge.replace(route, ""))
        except CheckError:
            mutations.append(label)
        else:
            fail(f"mutation was not rejected: {label}")
    return mutations


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--refresh-identity", action="store_true")
    parser.add_argument("--runtime", action="store_true")
    parser.add_argument("--binary", action="append", type=Path, default=[])
    parser.add_argument("--artifact", type=Path)
    args = parser.parse_args(argv)
    root = args.root.resolve()
    profile_path = args.profile if args.profile.is_absolute() else root / args.profile
    schema_path = args.schema if args.schema.is_absolute() else root / args.schema
    profile = load_json(profile_path)
    schema = load_json(schema_path)
    if args.refresh_identity:
        profile["sourceSetSha256"] = source_set_identity(root, profile["sourceModules"])
        profile["contentIdentitySha256"] = identity(profile)
        profile_path.write_text(json.dumps(profile, indent=2, sort_keys=True) + "\n")
    validate(profile, schema)
    report = {"static": static_check(root, profile)}
    if args.self_test:
        report["mutationsRejected"] = self_test(root, profile, schema)
    if args.runtime:
        if not args.binary:
            fail("--runtime requires at least one --binary")
        report["runtime"] = runtime_check(root, profile, args.binary, args.artifact)
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except CheckError as error:
        print(f"selfhost-obligation-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
