#!/usr/bin/env python3
"""Independent R4.2.b typecheck-authority verifier.

This verifier uses only the Python standard library and production CLI JSON. It
does not import or execute gc_types or any GenesisCode implementation crate.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time
from typing import Any


class VerificationError(RuntimeError):
    pass


def canonical_identity(profile: dict[str, Any]) -> str:
    payload = copy.deepcopy(profile)
    payload.pop("contentIdentitySha256", None)
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise VerificationError(f"cannot read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise VerificationError(f"JSON root must be an object: {path}")
    return value


def validate_profile(profile: dict[str, Any], schema: dict[str, Any]) -> None:
    required = schema.get("required")
    properties = schema.get("properties")
    if not isinstance(required, list) or not isinstance(properties, dict):
        raise VerificationError("authority schema must declare closed required properties")
    expected = set(properties)
    actual = set(profile)
    if actual != expected or set(required) != expected:
        raise VerificationError(
            f"profile root is not closed: missing={sorted(expected - actual)} "
            f"extra={sorted(actual - expected)}"
        )
    constants = {
        key: rule["const"]
        for key, rule in properties.items()
        if isinstance(rule, dict) and "const" in rule
    }
    for key, expected_value in constants.items():
        if profile.get(key) != expected_value:
            raise VerificationError(f"profile {key} does not match schema constant")
    identity = profile.get("contentIdentitySha256")
    if identity != canonical_identity(profile):
        raise VerificationError("profile contentIdentitySha256 mismatch")

    decisions = profile.get("decisionInventory")
    modules = profile.get("sourceModules")
    nonclaims = profile.get("nonclaims")
    if not isinstance(decisions, list) or len(decisions) < 18:
        raise VerificationError("decision inventory is incomplete")
    if len(decisions) != len(set(decisions)) or decisions != sorted(decisions):
        raise VerificationError("decision inventory must be unique and sorted")
    if not isinstance(modules, list) or len(modules) < 20:
        raise VerificationError("source module inventory is incomplete")
    if len(modules) != len(set(modules)):
        raise VerificationError("source module inventory contains duplicates")
    if not isinstance(nonclaims, list) or len(nonclaims) < 5:
        raise VerificationError("nonclaim inventory is incomplete")
    oracle = profile.get("compatibilityOracle")
    if not isinstance(oracle, dict) or set(oracle) != {
        "crate",
        "feature",
        "package",
        "sunsetReviewDate",
    }:
        raise VerificationError("compatibility oracle contract is not closed")
    if {key: oracle[key] for key in ("crate", "feature", "package")} != {
        "crate": "gc_types",
        "feature": "parity-oracle",
        "package": "gc_cli_driver_parity",
    }:
        raise VerificationError("compatibility oracle custody differs from the schema")
    runtime = profile.get("runtimeEvidence")
    if not isinstance(runtime, dict) or set(runtime) != {
        "allocationLimit",
        "classification",
        "coldDefinition",
        "corpusFixtureCount",
        "corpusGlob",
        "lowAllocationControl",
        "lowStepControl",
        "optimizationOwner",
        "performanceBudget",
        "performanceClaim",
        "stepLimit",
        "unrelatedCommand",
        "warmDefinition",
    }:
        raise VerificationError("runtime evidence contract is not closed")
    expected_runtime = {
        "allocationLimit": 50_000_000,
        "classification": "E0-observation",
        "coldDefinition": "first-fresh-process-after-build",
        "corpusFixtureCount": 26,
        "corpusGlob": "tests/spec/pkg_*/package.toml",
        "lowAllocationControl": 1,
        "lowStepControl": 1,
        "optimizationOwner": "R2.3",
        "performanceBudget": "PB-6",
        "performanceClaim": "none",
        "stepLimit": 50_000_000,
        "unrelatedCommand": "--help",
        "warmDefinition": "immediate-fresh-process-repeat-with-page-cache-retained",
    }
    if runtime != expected_runtime:
        raise VerificationError("runtime evidence contract differs from the schema")
    if runtime["stepLimit"] <= runtime["lowStepControl"]:
        raise VerificationError("step negative control must be lower than the observation limit")
    if runtime["allocationLimit"] <= runtime["lowAllocationControl"]:
        raise VerificationError(
            "allocation negative control must be lower than the observation limit"
        )


def manifest_typecheck_modules(root: Path) -> list[str]:
    text = (root / "selfhost/toolchain_manifest.gc").read_text()
    modules = re.findall(r'"(selfhost/typecheck_[a-z0-9_]+_v1\.gc)"', text)
    if len(modules) != len(set(modules)):
        raise VerificationError("toolchain manifest contains duplicate typecheck modules")
    if "core/cli::typecheck-package" not in text:
        raise VerificationError("toolchain manifest omits core/cli::typecheck-package")
    return modules


def manifest_all_modules(root: Path) -> list[str]:
    text = (root / "selfhost/toolchain_manifest.gc").read_text()
    module_section = text.split(":module-paths [", 1)
    if len(module_section) != 2:
        raise VerificationError("toolchain manifest omits :module-paths")
    module_section = module_section[1].split("]", 1)[0]
    modules = re.findall(r'"(selfhost/[^"\n]+\.gc)"', module_section)
    if not modules or len(modules) != len(set(modules)):
        raise VerificationError("toolchain manifest module closure is empty or duplicated")
    return modules


def verify_source_closure(root: Path, profile: dict[str, Any]) -> dict[str, int]:
    declared = profile["sourceModules"]
    discovered = manifest_typecheck_modules(root)
    if declared != discovered:
        raise VerificationError(
            "typecheck source inventory differs from toolchain manifest: "
            f"declared-only={sorted(set(declared) - set(discovered))} "
            f"manifest-only={sorted(set(discovered) - set(declared))}"
        )
    maximum = int(profile["limits"]["maxSourceLines"])
    lines: dict[str, int] = {}
    for relative in declared:
        path = (root / relative).resolve()
        if root.resolve() not in path.parents or path.is_symlink() or not path.is_file():
            raise VerificationError(f"invalid or escaping typecheck source: {relative}")
        count = len(path.read_text().splitlines())
        lines[relative] = count
        if count > maximum:
            raise VerificationError(
                f"typecheck source exceeds {maximum}-line budget: {relative}={count}"
            )
    return lines


def cargo_tree(root: Path, package: str) -> str:
    result = subprocess.run(
        [
            "cargo",
            "tree",
            "-p",
            package,
            "--edges",
            "normal",
            "--locked",
            "--offline",
        ],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise VerificationError(f"cargo tree failed for {package}: {result.stderr.strip()}")
    return result.stdout


def cargo_feature_members(manifest: str, feature: str) -> list[str]:
    match = re.search(
        rf"(?m)^{re.escape(feature)}\s*=\s*\[([^\n]*)\]\s*$", manifest
    )
    if match is None:
        return []
    return re.findall(r'"([^"\n]+)"', match.group(1))


def cargo_metadata(root: Path) -> dict[str, Any]:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--locked",
            "--offline",
        ],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise VerificationError(f"cargo metadata failed: {result.stderr.strip()}")
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise VerificationError(f"cargo metadata emitted invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise VerificationError("cargo metadata root is not an object")
    return value


def verify_oracle_isolation(root: Path) -> None:
    obligations = (root / "crates/gc_obligations/Cargo.toml").read_text()
    driver = (root / "crates/gc_cli_driver/Cargo.toml").read_text()
    parity = (root / "crates/gc_cli_driver_parity/Cargo.toml").read_text()
    authority = (
        root / "crates/gc_obligations/src/obligations/typecheck_authority.rs"
    ).read_text()
    if cargo_feature_members(obligations, "parity-oracle") != ["dep:gc_types"]:
        raise VerificationError("gc_types is not isolated behind parity-oracle")
    if 'gc_types = { path = "../gc_types", optional = true }' not in obligations:
        raise VerificationError("gc_types must be optional in gc_obligations")
    if "gc_obligations/parity-oracle" not in cargo_feature_members(
        driver, "parity-harness"
    ):
        raise VerificationError("driver parity feature does not enable the isolated oracle")
    if 'features = ["parity-oracle"]' not in parity:
        raise VerificationError("dedicated parity package does not bind the oracle feature")
    if '#[cfg(feature = "parity-oracle")]' not in authority:
        raise VerificationError("Rust checker branch lacks compile-time parity custody")
    if "Rust type/effect oracle is not compiled into production" not in authority:
        raise VerificationError("ordinary build lacks a typed oracle-rejection path")

    for package in ("gc_obligations", "gc_cli_driver"):
        if "gc_types v" in cargo_tree(root, package):
            raise VerificationError(f"normal {package} dependency graph reaches gc_types")
    if "gc_types v" not in cargo_tree(root, "gc_cli_driver_parity"):
        raise VerificationError("dedicated parity graph does not contain gc_types")
    packages = {
        item["name"]: item for item in cargo_metadata(root).get("packages", [])
    }
    for package, production, parity_bin in (
        ("gc_cli", "genesis", "genesis_parity"),
        ("gc_wasi_cli", "genesis_wasi", "genesis_wasi_parity"),
    ):
        outer = packages.get(package, {})
        if outer.get("features", {}).get("parity-harness") != [
            "dep:gc_cli_driver_parity"
        ]:
            raise VerificationError(f"{package} parity feature custody drift")
        parity_dependency = next(
            (
                item
                for item in outer.get("dependencies", [])
                if item.get("name") == "gc_cli_driver_parity"
            ),
            {},
        )
        if not parity_dependency.get("optional"):
            raise VerificationError(f"{package} parity driver is not optional")
        bins = {
            item.get("name"): item
            for item in outer.get("targets", [])
            if "bin" in item.get("kind", [])
        }
        if bins.get(parity_bin, {}).get("required-features") != ["parity-harness"]:
            raise VerificationError(f"{package} parity binary is not feature-gated")
        if bins.get(production, {}).get("required-features"):
            raise VerificationError(
                f"{package} production binary unexpectedly requires a feature"
            )
        outer_tree = cargo_tree(root, package)
        if "gc_cli_driver_parity v" in outer_tree or "gc_types v" in outer_tree:
            raise VerificationError(
                f"production {package} dependency graph reaches the parity oracle"
            )


def verify_report_custody(root: Path) -> None:
    authority = (
        root / "crates/gc_obligations/src/obligations/typecheck_authority.rs"
    ).read_text()
    decoder = (
        root / "crates/gc_obligations/src/obligations/typecheck_authority_decode.rs"
    ).read_text()
    abi = (root / "crates/gc_cli_driver/src/pkg_abi.rs").read_text()
    obligation = (root / "crates/gc_obligations/src/obligation_exec.rs").read_text()
    tests = (root / "crates/gc_obligations/src/tests/typecheck_authority.rs").read_text()
    required_authority = [
        "genesis/typecheck-request-v0.1",
        "core/cli::typecheck-package",
        "decode_typecheck_report",
        'Term::symbol(":modules")',
    ]
    for token in required_authority:
        if token not in authority:
            raise VerificationError(f"authority boundary missing token: {token}")
    required_decoder = [
        "module count mismatch",
        "module {index} path mismatch",
        "duplicate module path",
        "export inventory mismatch",
        "declared type mismatch",
        ":active disagrees",
        "identity presence disagrees",
        "report :ok disagrees with module/profile reports",
    ]
    for token in required_decoder:
        if token not in decoder:
            raise VerificationError(f"report decoder missing fail-closed control: {token}")
    for token in (
        "typecheck report missing declared export type",
        "typecheck report missing declared export effects",
        "typecheck_modules_with_authority",
    ):
        if token not in abi:
            raise VerificationError(f"ABI consumer does not require authoritative {token}")
    if "gc_types::infer_effects" in obligation:
        raise VerificationError("determinism obligation restored direct Rust effect inference")
    for token in (
        "active_profile_report_matches_rust_oracle",
        "active_profile_failures_match_rust_oracle",
        "binds_exact_module_order_count_and_paths",
        "binds_declared_export_inventory_and_types",
    ):
        if token not in tests:
            raise VerificationError(f"authority mutation coverage missing: {token}")


def timed_command(
    command: list[str], root: Path, artifact: Path
) -> tuple[subprocess.CompletedProcess[str], float, int]:
    env = os.environ.copy()
    env["GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT"] = str(artifact)
    env["LC_ALL"] = "C"
    time_binary = Path("/usr/bin/time")
    if not time_binary.is_file():
        raise VerificationError("/usr/bin/time is required for peak-RSS evidence")
    if platform.system() == "Darwin":
        timed = [str(time_binary), "-l", *command]
        rss_pattern = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.MULTILINE)
        rss_multiplier = 1
    elif platform.system() == "Linux":
        timed = [str(time_binary), "-v", *command]
        rss_pattern = re.compile(
            r"^\s*Maximum resident set size \(kbytes\):\s*(\d+)\s*$", re.MULTILINE
        )
        rss_multiplier = 1024
    else:
        raise VerificationError(f"unsupported peak-RSS host: {platform.system()}")
    started = time.monotonic()
    result = subprocess.run(
        timed,
        cwd=root,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    elapsed_ms = (time.monotonic() - started) * 1000.0
    rss_match = rss_pattern.search(result.stderr)
    if rss_match is None:
        raise VerificationError("cannot parse peak RSS from /usr/bin/time output")
    return result, elapsed_ms, int(rss_match.group(1)) * rss_multiplier


def run_json(
    binary: Path,
    root: Path,
    package: Path,
    step_limit: int,
    allocation_limit: int,
) -> tuple[int, dict[str, Any], float, int]:
    result, elapsed_ms, peak_rss_bytes = timed_command(
        [
            str(binary),
            "--step-limit",
            str(step_limit),
            "--max-alloc-units",
            str(allocation_limit),
            "--json",
            "typecheck",
            "--pkg",
            str(package),
        ],
        root,
        root / "selfhost/toolchain.gc",
    )
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise VerificationError(
            f"{binary.name} emitted invalid JSON: {error}; stderr={result.stderr.strip()}"
        ) from error
    if not isinstance(value, dict):
        raise VerificationError(f"{binary.name} JSON root is not an object")
    return result.returncode, value, elapsed_ms, peak_rss_bytes


def observe_unrelated(binary: Path, root: Path) -> dict[str, Any]:
    missing = root / ".genesis/authority-negative-control/missing-toolchain.gc"
    if missing.exists():
        raise VerificationError("unrelated-command missing-artifact control unexpectedly exists")
    result, elapsed_ms, peak_rss_bytes = timed_command(
        [str(binary), "--help"], root, missing
    )
    if result.returncode != 0 or "Usage:" not in result.stdout:
        raise VerificationError(f"{binary.name} --help unexpectedly loaded or failed")
    return {
        "artifactLoaded": False,
        "command": "--help",
        "elapsedMs": round(elapsed_ms, 3),
        "entrypoint": binary.name,
        "exit": result.returncode,
        "peakRssBytes": peak_rss_bytes,
    }


def verify_runtime(
    root: Path, native: Path, wasi: Path, profile: dict[str, Any]
) -> dict[str, list[dict[str, Any]]]:
    if not (root / "selfhost/toolchain.gc").is_file():
        raise VerificationError("selfhost toolchain artifact is missing")
    runtime = profile["runtimeEvidence"]
    step_limit = int(runtime["stepLimit"])
    allocation_limit = int(runtime["allocationLimit"])
    valid_package = root / "tests/spec/pkg_basic/package.toml"
    invalid_package = root / "tests/spec/pkg_fail_typecheck/package.toml"
    observations: list[dict[str, Any]] = []
    reports: dict[tuple[str, str], dict[str, Any]] = {}
    for label, binary in (("native", native), ("wasi", wasi)):
        for thermal in ("cold", "warm"):
            code, value, elapsed_ms, peak_rss_bytes = run_json(
                binary, root, valid_package, step_limit, allocation_limit
            )
            if (
                code != 0
                or value.get("kind") != "genesis/typecheck-v0.2"
                or value.get("ok") is not True
            ):
                raise VerificationError(f"valid/{label}/{thermal} unexpectedly failed")
            reports[(label, thermal)] = value
            observations.append(
                {
                    "entrypoint": label,
                    "fixture": "valid",
                    "phase": thermal,
                    "elapsedMs": round(elapsed_ms, 3),
                    "exit": code,
                    "peakRssBytes": peak_rss_bytes,
                }
            )
        code, value, elapsed_ms, peak_rss_bytes = run_json(
            binary, root, invalid_package, step_limit, allocation_limit
        )
        if (
            code == 0
            or value.get("kind") != "genesis/typecheck-v0.2"
            or value.get("ok") is not False
        ):
            raise VerificationError(f"invalid/{label} negative control passed")
        reports[(label, "invalid")] = value
        observations.append(
            {
                "entrypoint": label,
                "fixture": "invalid",
                "phase": "negative-control",
                "elapsedMs": round(elapsed_ms, 3),
                "exit": code,
                "peakRssBytes": peak_rss_bytes,
            }
        )

    for phase in ("cold", "warm", "invalid"):
        n_value = reports[("native", phase)]
        w_value = reports[("wasi", phase)]
        for label, value in (("native", n_value), ("wasi", w_value)):
            if value.get("kind") != "genesis/typecheck-v0.2":
                raise VerificationError(f"{phase}/{label} report kind mismatch")
        n_data = n_value.get("data")
        w_data = w_value.get("data")
        if not isinstance(n_data, dict) or not isinstance(w_data, dict):
            raise VerificationError(f"{phase} report data is missing")
        for field in ("report_coreform", "diagnostics"):
            if n_data.get(field) != w_data.get(field):
                raise VerificationError(f"{phase} native/WASI {field} divergence")

    resource_controls: list[dict[str, Any]] = []
    controls = (
        (
            "step",
            "step limit exceeded",
            int(runtime["lowStepControl"]),
            allocation_limit,
        ),
        (
            "allocation",
            "allocation-units",
            step_limit,
            int(runtime["lowAllocationControl"]),
        ),
    )
    for dimension, failure_token, controlled_steps, controlled_allocations in controls:
        for label, binary in (("native", native), ("wasi", wasi)):
            code, value, _, _ = run_json(
                binary, root, valid_package, controlled_steps, controlled_allocations
            )
            if code == 0 or value.get("ok") is not False:
                raise VerificationError(f"{label} ignored low {dimension} limit")
            encoded = json.dumps(value, sort_keys=True)
            if failure_token not in encoded:
                raise VerificationError(f"{label} did not identify the {dimension} limit")
            resource_controls.append(
                {
                    "dimension": dimension,
                    "entrypoint": label,
                    "exit": code,
                    "failureToken": failure_token,
                    "rejected": True,
                }
            )

    unrelated = [observe_unrelated(binary, root) for binary in (native, wasi)]
    return {
        "checker": observations,
        "resourceControls": resource_controls,
        "unrelatedCommand": unrelated,
    }


def expect_rejected(profile: dict[str, Any], schema: dict[str, Any]) -> None:
    try:
        validate_profile(profile, schema)
    except VerificationError:
        return
    raise VerificationError("profile mutation was accepted")


def self_test(profile: dict[str, Any], schema: dict[str, Any]) -> int:
    mutations: list[dict[str, Any]] = []
    extra = copy.deepcopy(profile)
    extra["forged"] = True
    mutations.append(extra)
    wrong_binding = copy.deepcopy(profile)
    wrong_binding["binding"] = "core/cli::rust-typecheck"
    mutations.append(wrong_binding)
    duplicate_module = copy.deepcopy(profile)
    duplicate_module["sourceModules"].append(duplicate_module["sourceModules"][0])
    mutations.append(duplicate_module)
    missing_decisions = copy.deepcopy(profile)
    missing_decisions["decisionInventory"] = []
    mutations.append(missing_decisions)
    wrong_entrypoints = copy.deepcopy(profile)
    wrong_entrypoints["productionEntrypoints"] = ["genesis_parity"]
    mutations.append(wrong_entrypoints)
    forged_identity = copy.deepcopy(profile)
    forged_identity["contentIdentitySha256"] = "f" * 64
    mutations.append(forged_identity)
    open_oracle = copy.deepcopy(profile)
    open_oracle["compatibilityOracle"]["package"] = "gc_cli_driver"
    mutations.append(open_oracle)
    forged_performance_claim = copy.deepcopy(profile)
    forged_performance_claim["runtimeEvidence"]["performanceClaim"] = "PB-6-pass"
    mutations.append(forged_performance_claim)
    for mutation in mutations:
        expect_rejected(mutation, schema)
    return len(mutations)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--schema", type=Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--runtime", action="store_true")
    parser.add_argument("--genesis-bin", type=Path)
    parser.add_argument("--genesis-wasi-bin", type=Path)
    args = parser.parse_args()

    root = args.root.resolve()
    profile = load_json((root / args.profile).resolve() if not args.profile.is_absolute() else args.profile)
    schema = load_json((root / args.schema).resolve() if not args.schema.is_absolute() else args.schema)
    validate_profile(profile, schema)
    line_counts = verify_source_closure(root, profile)
    verify_oracle_isolation(root)
    verify_report_custody(root)
    mutation_count = self_test(profile, schema) if args.self_test else 0
    observations: dict[str, list[dict[str, Any]]] = {
        "checker": [],
        "resourceControls": [],
        "unrelatedCommand": [],
    }
    if args.runtime:
        if args.genesis_bin is None or args.genesis_wasi_bin is None:
            raise VerificationError("runtime verification requires both production binaries")
        observations = verify_runtime(root, args.genesis_bin, args.genesis_wasi_bin, profile)

    artifact = root / profile["artifact"]
    all_modules = manifest_all_modules(root)
    corpus_fixtures = sorted(root.glob(profile["runtimeEvidence"]["corpusGlob"]))
    if len(corpus_fixtures) != profile["runtimeEvidence"]["corpusFixtureCount"]:
        raise VerificationError("typecheck corpus fixture count differs from the closed profile")
    output = {
        "artifactBytes": artifact.stat().st_size if artifact.is_file() else None,
        "artifactSha256": (
            hashlib.sha256(artifact.read_bytes()).hexdigest() if artifact.is_file() else None
        ),
        "componentClosure": {
            "distributionEnvelope": "combined-v0.1",
            "performanceOwner": profile["runtimeEvidence"]["optimizationOwner"],
            "separatelyLoadable": False,
            "toolchainModules": len(all_modules),
            "typecheckModules": len(line_counts),
        },
        "contentIdentitySha256": profile["contentIdentitySha256"],
        "corpusFixtureCount": len(corpus_fixtures),
        "decisionCount": len(profile["decisionInventory"]),
        "kind": "genesis/selfhost-typecheck-authority-check-v0.1",
        "maxSourceLines": max(line_counts.values()),
        "moduleCount": len(line_counts),
        "mutationControls": mutation_count,
        "observations": observations,
        "performanceDisposition": {
            "budget": profile["runtimeEvidence"]["performanceBudget"],
            "classification": profile["runtimeEvidence"]["classification"],
            "claim": profile["runtimeEvidence"]["performanceClaim"],
            "owner": profile["runtimeEvidence"]["optimizationOwner"],
        },
        "resourceBounds": {
            "maxAllocUnits": profile["runtimeEvidence"]["allocationLimit"],
            "stepLimit": profile["runtimeEvidence"]["stepLimit"],
        },
        "ok": True,
    }
    print(json.dumps(output, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except VerificationError as error:
        print(f"selfhost-typecheck-authority: {error}", file=sys.stderr)
        raise SystemExit(1)
