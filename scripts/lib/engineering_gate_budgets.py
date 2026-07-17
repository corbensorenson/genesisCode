#!/usr/bin/env python3
"""Validate and enforce the GB-1 through GB-8 engineering budget authority."""

from __future__ import annotations

import argparse
import copy
import datetime as dt
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Dict, Iterable, List, Mapping, Optional, Sequence, Set, Tuple

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/lib"))
from toml_compat import tomllib

POLICY = ROOT / "policies/engineering_gate_budgets_v0.1.json"
SCHEMA = ROOT / "docs/spec/ENGINEERING_GATE_BUDGETS_v0.1.schema.json"
MANIFEST = ROOT / "genesis.gates.json"

TOP_FIELDS = {"kind", "version", "roadmapTask", "budgets", "panicAssurance", "profileSubjects", "releaseFullOnlyGates", "semanticCrateWaivers", "staticReclassifications"}
BUDGET_IDS = [f"GB-{index}" for index in range(1, 9)]
EXPECTED = {
    "GB-1": {"subject": "static-gates", "maxWarmDurationMs": 15000, "maxAdditionalDiskBytes": 67108864, "network": "deny", "compilation": False},
    "GB-2": {"subject": "changed-file-gate", "maxWarmDurationMs": 120000, "maxAdditionalDiskBytes": 1073741824, "network": "deny"},
    "GB-3": {"subject": "prepush-standard", "maxWarmDurationMs": 480000, "maxAdditionalDiskBytes": 3221225472},
    "GB-4": {"subject": "release-full", "maxReferenceDurationMs": 2700000, "maxArtifactBytes": 21474836480},
    "GB-5": {"subject": "development-footprint", "maxNormalBuildBytes": 8589934592, "maxPostCleanGeneratedBytes": 2147483648, "cleanupProfile": "dev-clean"},
    "GB-6": {"subject": "prebuilt-evidence-verification", "maxOfflineDurationMs": 300000, "network": "deny", "compilerInvocation": False},
    "GB-7": {"subject": "source-concentration", "maxProductionRustFileLines": 1000, "maxSemanticCrateLines": 20000, "enforcementMilestone": "M3"},
    "GB-8": {"subject": "fresh-clone-prerequisites", "prerequisiteManifest": "genesis.prerequisites.json", "minimumPython": "3.9", "undeclaredPythonModules": 0},
}
STDLIB = {
    "__future__", "argparse", "ast", "base64", "binascii", "collections", "contextlib", "copy",
    "dataclasses", "datetime", "decimal", "errno", "fcntl", "fnmatch", "fractions", "functools", "gzip",
    "hashlib", "html", "http", "io", "json", "math", "msvcrt", "os", "pathlib", "platform", "random", "re",
    "shlex", "shutil", "signal", "socket", "stat",
    "secrets", "statistics", "string", "subprocess", "sys", "tarfile", "tempfile", "threading", "time", "tomllib",
    "types", "typing", "urllib", "xml",
}


class BudgetError(ValueError):
    pass


def unique_pairs(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise BudgetError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs)
    except (OSError, json.JSONDecodeError) as exc:
        raise BudgetError(f"cannot load {path.relative_to(ROOT)}: {exc}") from exc


def canonical_path(raw: str, field: str, must_exist: bool = True) -> str:
    path = PurePosixPath(raw)
    if not raw or path.is_absolute() or path.as_posix() != raw or ".." in path.parts or "." in path.parts or "\\" in raw:
        raise BudgetError(f"{field} is not canonical repository-relative: {raw!r}")
    if must_exist and not (ROOT / raw).exists():
        raise BudgetError(f"{field} does not exist: {raw}")
    return raw


def require_policy(policy: Any) -> Mapping[str, Any]:
    if not isinstance(policy, dict) or set(policy) != TOP_FIELDS:
        raise BudgetError("engineering budget policy fields mismatch")
    if policy["kind"] != "genesis/engineering-gate-budget-policy-v0.1" or policy["version"] != "0.1" or policy["roadmapTask"] != "R0.4.f":
        raise BudgetError("engineering budget policy identity mismatch")
    if not isinstance(policy["budgets"], dict) or list(policy["budgets"]) != BUDGET_IDS:
        raise BudgetError("engineering budget inventory must be exactly GB-1 through GB-8 in order")
    for budget_id in BUDGET_IDS:
        if policy["budgets"][budget_id] != EXPECTED[budget_id]:
            raise BudgetError(f"{budget_id} normative values drift")
    for section in ("panicAssurance", "profileSubjects"):
        if not isinstance(policy[section], dict):
            raise BudgetError(f"{section} must be an object")
        for field, path in policy[section].items():
            canonical_path(path, f"{section}.{field}")
    release_only = policy["releaseFullOnlyGates"]
    if not isinstance(release_only, list) or not release_only or release_only != sorted(set(release_only)):
        raise BudgetError("release-full-only gates must be a non-empty sorted unique array")
    for path in release_only:
        if not isinstance(path, str):
            raise BudgetError("release-full-only gate must be a path string")
        canonical_path(path, "release-full-only gate")
    rows = policy["staticReclassifications"]
    if not isinstance(rows, list) or rows != sorted(rows, key=lambda row: row.get("entrypoint", "")):
        raise BudgetError("static reclassifications must be a sorted array")
    for row in rows:
        if not isinstance(row, dict) or set(row) != {"entrypoint", "kind", "profile"}:
            raise BudgetError("static reclassification fields mismatch")
        canonical_path(row["entrypoint"], "static reclassification entrypoint")
        if row["kind"] == "static" or row["profile"] not in {"prepush-standard", "release-full"}:
            raise BudgetError("static reclassification must move to a non-static governed profile")
    waivers = policy["semanticCrateWaivers"]
    if not isinstance(waivers, list) or waivers != sorted(waivers, key=lambda row: row.get("crate", "")):
        raise BudgetError("semantic crate waivers must be sorted")
    for row in waivers:
        if not isinstance(row, dict) or set(row) != {"crate", "owner", "rationale", "expiresAtMilestone"}:
            raise BudgetError("semantic crate waiver fields mismatch")
        canonical_path(row["crate"], "semantic crate waiver")
        if row["expiresAtMilestone"] != "M3" or not row["owner"] or len(row["rationale"]) < 20:
            raise BudgetError("semantic crate waiver is unbounded")
    return policy


def validate_schema() -> None:
    schema = load_json(SCHEMA)
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema" or schema.get("$id") != "https://genesiscode.dev/schemas/engineering-gate-budgets-v0.1.json" or schema.get("additionalProperties") is not False:
        raise BudgetError("engineering budget schema identity/closure drift")
    required = schema.get("required")
    if not isinstance(required, list) or set(required) != TOP_FIELDS:
        raise BudgetError("engineering budget schema required-field closure drift")


def gate_map() -> Dict[str, Mapping[str, Any]]:
    manifest = load_json(MANIFEST)
    gates = manifest.get("gates") if isinstance(manifest, dict) else None
    if not isinstance(gates, list):
        raise BudgetError("gate manifest has no gate inventory")
    return {gate["entrypoint"]: gate for gate in gates}


def check_gb1(policy: Mapping[str, Any], gates: Mapping[str, Mapping[str, Any]]) -> int:
    reclassified = {row["entrypoint"]: row for row in policy["staticReclassifications"]}
    for path, row in reclassified.items():
        gate = gates.get(path)
        if gate is None or gate["kind"] != row["kind"] or gate["profile"] != row["profile"]:
            raise BudgetError(f"measured static outlier remains misclassified: {path}")
    static = [gate for gate in gates.values() if gate["kind"] == "static"]
    if not static:
        raise BudgetError("gate manifest has no static gates")
    for gate in static:
        if gate["compilation"] is not False:
            raise BudgetError(f"GB-1 static gate compiles: {gate['entrypoint']}")
        if gate["expectedDurationSeconds"] > 15:
            raise BudgetError(f"GB-1 static gate exceeds 15-second envelope: {gate['entrypoint']}")
        if gate["diskBudgetMiB"] * 1048576 > EXPECTED["GB-1"]["maxAdditionalDiskBytes"]:
            raise BudgetError(f"GB-1 static gate exceeds disk envelope: {gate['entrypoint']}")
        if gate["network"] != {"mode": "deny", "declaredInputs": []}:
            raise BudgetError(f"GB-1 static gate permits network: {gate['entrypoint']}")
    telemetry = (ROOT / "scripts/lib/gate_telemetry.py").read_text(encoding="utf-8")
    for marker in ("expectedDurationSeconds", "diskBudgetMiB", "network-attempt", "resource budget exceeded"):
        if marker not in telemetry:
            raise BudgetError(f"per-gate resource enforcement marker missing: {marker}")
    return len(static)


def require_source_markers(path: str, markers: Sequence[str]) -> str:
    source = (ROOT / canonical_path(path, "profile subject")).read_text(encoding="utf-8")
    missing = [marker for marker in markers if marker not in source]
    if missing:
        raise BudgetError(f"{path} is missing budget enforcement markers: {missing}")
    return source


def profile_sections(runner_source: str) -> Tuple[str, str, str]:
    patterns = (
        r"(?ms)^COMMON_GATES=\(\n(.*?)^\)\n\nif \[\[ \"\$PROFILE\" == \"agent-inner-loop\" \]\]",
        r"(?ms)^  prepush-standard\)\n(.*?)(?=^  release-full\))",
        r"(?ms)^  release-full\)\n(.*?)(?=^  full-selfhost-cutover\))",
    )
    sections = []
    for pattern in patterns:
        match = re.search(pattern, runner_source)
        if match is None:
            raise BudgetError("profile runner case topology is not parseable")
        sections.append(match.group(1))
    return sections[0], sections[1], sections[2]


def check_profiles(policy: Mapping[str, Any], gates: Mapping[str, Mapping[str, Any]]) -> None:
    changed = policy["profileSubjects"]["changedFileScript"]
    require_source_markers(changed, [
        "GENESIS_TEST_CHANGED_BUDGET_MS:-120000",
        "GENESIS_TEST_CHANGED_FALLBACK_BUDGET_MS:-480000",
        "1073741824",
        "3221225472",
        'BUDGET_SUBJECT="changed-file-gate"',
        "CARGO_NET_OFFLINE",
    ])
    runner = policy["profileSubjects"]["profileRunner"]
    runner_source = require_source_markers(runner, ["GENESIS_HEALTH_PREPUSH_BUDGET_MS:-480000", "3221225472", "21474836480", "2700000", "CARGO_GATE_ENTRYPOINTS", 'gate["compilation"]'])
    common_section, prepush_section, release_section = profile_sections(runner_source)
    for entrypoint in policy["releaseFullOnlyGates"]:
        if entrypoint not in gates:
            raise BudgetError(f"release-full-only gate is absent from gate manifest: {entrypoint}")
        if entrypoint in common_section or entrypoint in prepush_section:
            raise BudgetError(f"release-full-only gate leaks into GB-3 scheduling: {entrypoint}")
        if entrypoint not in release_section:
            raise BudgetError(f"release-full-only gate is not scheduled by release-full: {entrypoint}")
    verifier = policy["profileSubjects"]["evidenceVerifierScript"]
    verifier_source = require_source_markers(verifier, ["prebuilt_evidence_verify.py"])
    helper = (ROOT / "scripts/lib/prebuilt_evidence_verify.py").read_text(encoding="utf-8")
    if re.search(r"\bcargo\b|\brustc\b", verifier_source + "\n" + helper):
        raise BudgetError("GB-6 prebuilt verifier path can invoke a compiler")
    for marker in ("deny network", "--net", "timeout=args.timeout_seconds", "default=300"):
        if marker not in helper:
            raise BudgetError(f"GB-6 verifier enforcement marker missing: {marker}")

    cleanup = load_json(ROOT / "policies/deterministic_cleanup_v0.1.json")
    profiles = {row["id"]: row for row in cleanup.get("profiles", [])}
    if profiles.get("dev-clean") != {"id": "dev-clean", "deleteClasses": ["rebuildable-output"]}:
        raise BudgetError("GB-5 dev-clean profile drift")
    classes = {row["id"]: row for row in cleanup.get("classes", [])}
    rebuildable = classes.get("rebuildable-output", {})
    required_roots = {".genesis/build", ".genesis/cache", ".genesis/tmp", "target", "node_modules"}
    if rebuildable.get("deletable") is not True or not required_roots <= set(rebuildable.get("roots", [])):
        raise BudgetError("GB-5 cleanup does not cover required rebuildable roots")


def production_rust_files() -> Iterable[Path]:
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        rel = path.relative_to(ROOT)
        parts = rel.parts
        if any(part in {"benches", "examples"} or "test" in part for part in parts):
            continue
        if "test" in path.stem:
            continue
        yield path


def check_source_concentration(policy: Mapping[str, Any]) -> Tuple[int, int]:
    source_policy = tomllib.loads((ROOT / "policies/source_decomposition_progress.toml").read_text(encoding="utf-8"))
    tracked = {row.get("module_path"): row for row in source_policy.get("tracked_over_budget_rows", []) if isinstance(row, dict)}
    review_date = dt.date.today()
    over_files = []
    crate_lines: Dict[str, int] = {}
    for path in production_rust_files():
        count = sum(1 for _ in path.open("r", encoding="utf-8"))
        rel = path.relative_to(ROOT).as_posix()
        crate = "/".join(rel.split("/")[:2])
        crate_lines[crate] = crate_lines.get(crate, 0) + count
        if count > EXPECTED["GB-7"]["maxProductionRustFileLines"]:
            over_files.append(rel)
            row = tracked.get(rel)
            if row is None or row.get("status") != "waived":
                raise BudgetError(f"GB-7 over-limit production file lacks a tracked waiver: {rel}")
            try:
                expiry = dt.date.fromisoformat(str(row.get("waiver_review_by", "")))
            except ValueError as exc:
                raise BudgetError(f"GB-7 waiver has invalid review date: {rel}") from exc
            if expiry < review_date:
                raise BudgetError(f"GB-7 waiver expired: {rel}")
            for field in ("waiver_owner", "waiver_scope", "waiver_rationale", "parity_gate"):
                if not isinstance(row.get(field), str) or not row[field].strip():
                    raise BudgetError(f"GB-7 waiver field is missing for {rel}: {field}")
    over_crates = {crate for crate, lines in crate_lines.items() if lines > EXPECTED["GB-7"]["maxSemanticCrateLines"]}
    waived_crates = {row["crate"] for row in policy["semanticCrateWaivers"]}
    if over_crates != waived_crates:
        raise BudgetError(f"GB-7 semantic crate waiver closure drift: observed={sorted(over_crates)} waived={sorted(waived_crates)}")
    return len(over_files), len(over_crates)


def undeclared_python_modules(
    sources: Iterable[Tuple[str, str]], repo_modules: Set[str]
) -> Dict[str, Set[str]]:
    import_re = re.compile(r"^\s*(?:import|from)\s+([A-Za-z_][A-Za-z0-9_]*)", re.MULTILINE)
    unknown: Dict[str, Set[str]] = {}
    for source_name, source in sources:
        for module in import_re.findall(source):
            if module not in STDLIB and module not in repo_modules:
                unknown.setdefault(module, set()).add(source_name)
    return unknown


def check_python_closure() -> int:
    repo_modules = {path.stem for path in (ROOT / "scripts/lib").glob("*.py")} | {"vendor"}
    sources: List[Tuple[str, str]] = []
    scanned = 0
    for base in (ROOT / "scripts", ROOT / "tools"):
        for path in sorted(base.rglob("*")):
            if not path.is_file() or path.suffix not in {".py", ".sh"}:
                continue
            scanned += 1
            sources.append(
                (path.relative_to(ROOT).as_posix(), path.read_text(encoding="utf-8"))
            )
    unknown = undeclared_python_modules(sources, repo_modules)
    if unknown:
        detail = "; ".join(f"{name}:{','.join(sorted(paths))}" for name, paths in sorted(unknown.items()))
        raise BudgetError("GB-8 undeclared Python modules: " + detail)
    prerequisites = load_json(ROOT / "genesis.prerequisites.json")
    python = next((tool for tool in prerequisites.get("tools", []) if tool.get("id") == "python"), None)
    if python is None or python.get("constraint", {}).get("minInclusive") != "3.9.0":
        raise BudgetError("GB-8 prerequisite manifest does not declare Python >=3.9.0")
    if not (ROOT / "scripts/lib/vendor/tomli/LICENSE").is_file():
        raise BudgetError("GB-8 vendored Python compatibility dependency lacks its license")
    return scanned


def bundle_digest() -> str:
    paths = [
        "policies/engineering_gate_budgets_v0.1.json",
        "docs/spec/ENGINEERING_GATE_BUDGETS_v0.1.schema.json",
        "scripts/lib/engineering_gate_budgets.py",
        "scripts/check_engineering_gate_contract.sh",
        "genesis.gates.json",
        "policies/gates_v0.1.json",
        "policies/gate_telemetry_v0.1.json",
        "scripts/lib/gate_telemetry.py",
        "scripts/lib/gate_telemetry.sh",
        "scripts/check_gate_resource_telemetry.sh",
        "scripts/test_changed_fast.sh",
        "scripts/render_upgrade_plan_health_report.sh",
        "policies/deterministic_cleanup_v0.1.json",
        "policies/source_decomposition_progress.toml",
        "genesis.prerequisites.json",
        "scripts/lib/panic_policy.py",
        "scripts/check_no_user_panics.sh",
        "scripts/check_no_user_panics_compiler.sh",
        "scripts/verify_prebuilt_evidence_bundle.sh",
        "scripts/lib/prebuilt_evidence_verify.py",
        "scripts/lib/toml_compat.py",
        "scripts/lib/vendor/tomli/LICENSE",
        "scripts/lib/vendor/tomli/__init__.py",
        "scripts/lib/vendor/tomli/_parser.py",
        "scripts/lib/vendor/tomli/_re.py",
        "scripts/lib/vendor/tomli/_types.py",
        "policies/check_update_boundary_v0.1.json",
        "docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json",
        "docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md",
        "scripts/check_green_front_door.sh",
        "scripts/check_test_execution_profile_matrix.sh",
        ".github/workflows/ci.yml",
        "docs/INDEX.md",
    ]
    digest = sha256()
    for rel in paths:
        data = (ROOT / rel).read_bytes()
        rel_bytes = rel.encode("utf-8")
        digest.update(len(rel_bytes).to_bytes(8, "big"))
        digest.update(rel_bytes)
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
    return digest.hexdigest()


def run_check(policy_doc: Optional[Any] = None) -> Dict[str, Any]:
    policy = require_policy(load_json(POLICY) if policy_doc is None else policy_doc)
    validate_schema()
    gates = gate_map()
    static_count = check_gb1(policy, gates)
    check_profiles(policy, gates)
    file_waivers, crate_waivers = check_source_concentration(policy)
    python_files = check_python_closure()
    return {"static": static_count, "fileWaivers": file_waivers, "crateWaivers": crate_waivers, "pythonFiles": python_files}


def self_test() -> int:
    policy = load_json(POLICY)
    controls = 0
    mutations = []
    candidate = copy.deepcopy(policy); candidate["budgets"]["GB-1"]["maxWarmDurationMs"] += 1; mutations.append(candidate)
    candidate = copy.deepcopy(policy); candidate["budgets"]["GB-2"]["network"] = "allow"; mutations.append(candidate)
    candidate = copy.deepcopy(policy); candidate["semanticCrateWaivers"][0]["expiresAtMilestone"] = "M4"; mutations.append(candidate)
    candidate = copy.deepcopy(policy); candidate["staticReclassifications"].append(copy.deepcopy(candidate["staticReclassifications"][0])); mutations.append(candidate)
    candidate = copy.deepcopy(policy); candidate["releaseFullOnlyGates"].append(candidate["releaseFullOnlyGates"][0]); mutations.append(candidate)
    candidate = copy.deepcopy(policy); candidate["unknown"] = True; mutations.append(candidate)
    for candidate in mutations:
        try:
            require_policy(candidate)
        except BudgetError:
            controls += 1
        else:
            raise BudgetError("engineering budget negative control was accepted")
    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=unique_pairs)
    except BudgetError:
        controls += 1
    else:
        raise BudgetError("duplicate-key negative control was accepted")
    candidate = copy.deepcopy(policy)
    candidate["releaseFullOnlyGates"].append("scripts/check_selfhost_boundary.sh")
    candidate["releaseFullOnlyGates"].sort()
    try:
        check_profiles(require_policy(candidate), gate_map())
    except BudgetError:
        controls += 1
    else:
        raise BudgetError("release-profile leakage negative control was accepted")
    platform_stdlib = "import contextlib\nimport errno\nimport fcntl\nimport msvcrt\n"
    if undeclared_python_modules((("platform.py", platform_stdlib),), set()):
        raise BudgetError("declared cross-platform Python standard-library modules were rejected")
    controls += 1
    unknown = undeclared_python_modules(
        (("external.py", "import genesis_undeclared_dependency\n"),), set()
    )
    if unknown != {"genesis_undeclared_dependency": {"external.py"}}:
        raise BudgetError("undeclared Python module negative control was accepted")
    controls += 1
    print(f"engineering-gate-budgets: self-test ok (negative_controls={controls})")
    return controls


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("check", "self-test"))
    args = parser.parse_args(argv)
    try:
        if args.mode == "self-test":
            self_test()
            return 0
        summary = run_check()
        print(
            "engineering-gate-budgets: ok "
            f"(budgets=8 static_gates={summary['static']} file_waivers={summary['fileWaivers']} "
            f"crate_waivers={summary['crateWaivers']} python_files={summary['pythonFiles']} bundle={bundle_digest()})"
        )
        return 0
    except (BudgetError, OSError, UnicodeError) as exc:
        print(f"engineering-gate-budgets: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
