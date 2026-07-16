#!/usr/bin/env python3
"""Render and validate the closed GenesisCode gate manifest."""

from __future__ import annotations

import argparse
from copy import deepcopy
from functools import lru_cache
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Dict, Iterable, Mapping, Optional, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
MANIFEST_REL = "genesis.gates.json"
POLICY_REL = "policies/gates_v0.1.json"
AUDIT_REL = "docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json"
PREREQUISITES_REL = "genesis.prerequisites.json"
SCHEMA_REL = "docs/spec/GATE_MANIFEST_v0.1.schema.json"
INPUT_IDENTITY_EXCLUSIONS = {
    MANIFEST_REL: "generated-self-excluded",
    "CHANGELOG.md": "release-derived-excluded",
    "ROADMAP.md": "evidence-citation-excluded",
    "docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json": "roadmap-derived-excluded",
}

GATE_KINDS = {"static", "build", "test", "benchmark", "proof", "release-only"}
BOUNDARY_CLASSES = {"static", "build-runtime", "benchmark", "aggregate"}
NETWORK_MODES = {"deny", "loopback-only", "external-optional", "external-required"}
SHARD_MODES = {"none", "gate-level", "dependency-dag", "workspace-crate"}
ISOLATION_MODES = {
    "shared-read-only", "shared-cargo-cache", "isolated-worktree-and-cache"
}
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
HOST_PATH_RE = re.compile(r"(?:^|[\s=:])(?:/Users/|/home/|/private/|/tmp/|[A-Za-z]:[\\/])")

POLICY_FIELDS = {
    "kind", "version", "inventory", "platforms", "profiles", "inputSets",
    "outputSets", "classDefaults", "kindOverrides", "kindDefaults",
    "networkOverrides", "gateOverrides", "defaultPlatforms", "platformOverrides", "toolOverrides",
    "governanceBudget",
}
MANIFEST_FIELDS = {
    "kind", "version", "inventory", "platforms", "profiles", "inputSets",
    "outputSets", "gates",
}
GATE_FIELDS = {
    "id", "entrypoint", "sourceSha256", "executionIdentitySha256", "inputIdentitySha256", "boundaryClass",
    "kind", "inputs", "outputs", "dependencies", "profile", "expectedDurationSeconds",
    "diskBudgetMiB", "network", "platforms", "tools", "sharding", "compilation", "readOnly",
}
REPO_REF_RE = re.compile(
    r"(?<![A-Za-z0-9_.-])((?:scripts|policies|docs|crates|tools|benchmarks|prelude|selfhost|examples|tests)/[A-Za-z0-9_./*-]+)"
)
ROOT_REF_RE = re.compile(
    r"(?<![A-Za-z0-9_.-])((?:Cargo\.(?:toml|lock)|README\.md|ROADMAP\.md|CHANGELOG\.md|rust-toolchain\.toml|genesis\.[A-Za-z0-9_.-]+))"
)
GOVERNANCE_SNAPSHOT_FIELDS = {
    "checkEntrypoints", "updateEntrypoints", "renderEntrypoints",
    "declaredDurationSeconds", "declaredDiskMiB",
}
ENTRYPOINT_CEILING_FIELDS = {
    "checkEntrypoints", "updateEntrypoints", "renderEntrypoints",
}
RETIRED_ALIAS_FIELDS = {
    "path", "replacement", "surface", "declaredDurationSeconds", "declaredDiskMiB",
}

TOOL_PATTERNS = {
    "adb": re.compile(r"(?:^|[^A-Za-z0-9_-])adb(?:[^A-Za-z0-9_-]|$)"),
    "cargo-deny": re.compile(r"\bcargo\s+deny\b|\bcargo-deny\b"),
    "cargo-nextest": re.compile(r"\bcargo\s+nextest\b|\bcargo-nextest\b"),
    "clang": re.compile(r"(?:^|[^A-Za-z0-9_-])clang(?:[^A-Za-z0-9_-]|$)"),
    "jq": re.compile(r"(?:^|[^A-Za-z0-9_-])jq(?:[^A-Za-z0-9_-]|$)"),
    "lake": re.compile(r"(?:^|[^A-Za-z0-9_-])lake(?:[^A-Za-z0-9_-]|$)"),
    "lean": re.compile(r"(?:^|[^A-Za-z0-9_-])lean(?:[^A-Za-z0-9_-]|$)"),
    "node": re.compile(r"(?:^|[^A-Za-z0-9_-])node(?:[^A-Za-z0-9_-]|$)"),
    "npm": re.compile(r"(?:^|[^A-Za-z0-9_-])npm(?:[^A-Za-z0-9_-]|$)"),
    "rustc": re.compile(r"(?:^|[^A-Za-z0-9_-])rustc(?:[^A-Za-z0-9_-]|$)"),
    "shellcheck": re.compile(r"(?:^|[^A-Za-z0-9_-])shellcheck(?:[^A-Za-z0-9_-]|$)"),
    "wasm-bindgen": re.compile(r"(?:^|[^A-Za-z0-9_-])wasm-bindgen(?:[^A-Za-z0-9_-]|$)"),
    "wasmtime": re.compile(r"(?:^|[^A-Za-z0-9_-])wasmtime(?:[^A-Za-z0-9_-]|$)"),
    "xcodebuild": re.compile(r"(?:^|[^A-Za-z0-9_-])xcodebuild(?:[^A-Za-z0-9_-]|$)"),
    "xcrun": re.compile(r"(?:^|[^A-Za-z0-9_-])xcrun(?:[^A-Za-z0-9_-]|$)"),
}


class GateManifestError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise GateManifestError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise GateManifestError(f"missing file: {display_path(path)}") from exc
    except json.JSONDecodeError as exc:
        raise GateManifestError(
            f"invalid JSON in {display_path(path)}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


@lru_cache(maxsize=None)
def digest_file(path: Path) -> str:
    return sha256(path.read_bytes()).hexdigest()


def canonical_text(value: Any) -> str:
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n"


def canonical_identity(value: Any) -> str:
    compact = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return sha256(compact.encode("utf-8")).hexdigest()


def require_type(value: Any, expected: type, context: str) -> None:
    if not isinstance(value, expected):
        raise GateManifestError(f"{context} must be {expected.__name__}")


def require_closed(value: Mapping[str, Any], fields: set[str], context: str) -> None:
    actual = set(value)
    if actual != fields:
        raise GateManifestError(
            f"{context} field mismatch: missing={sorted(fields - actual)} "
            f"unknown={sorted(actual - fields)}"
        )


def require_string(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise GateManifestError(f"{context} must be a non-empty string")
    if HOST_PATH_RE.search(value):
        raise GateManifestError(f"{context} contains a host-specific path")
    return value


def require_int(value: Any, context: str, minimum: int = 0) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise GateManifestError(f"{context} must be an integer >= {minimum}")
    return value


def require_unique_strings(values: Any, context: str, *, allow_empty: bool = True) -> list[str]:
    require_type(values, list, context)
    out = [require_string(value, context) for value in values]
    if not allow_empty and not out:
        raise GateManifestError(f"{context} must not be empty")
    if len(out) != len(set(out)):
        raise GateManifestError(f"{context} contains duplicates")
    return out


@lru_cache(maxsize=None)
def _repo_path_error(value: str, must_exist: bool) -> Optional[str]:
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or "." in path.parts or "\\" in value:
        return "must be a normalized repository-relative path"
    if must_exist and not (ROOT / value).is_file():
        return f"references missing file: {value}"
    return None


def validate_repo_path(raw: Any, context: str, *, must_exist: bool = False) -> str:
    value = require_string(raw, context)
    error = _repo_path_error(value, must_exist)
    if error is not None:
        raise GateManifestError(f"{context} {error}")
    return value


def validate_sorted_unique_objects(items: Any, context: str) -> list[Mapping[str, Any]]:
    require_type(items, list, context)
    require_type_items = []
    ids = []
    for index, item in enumerate(items):
        require_type(item, dict, f"{context}[{index}]")
        identifier = require_string(item.get("id"), f"{context}[{index}].id")
        if not ID_RE.fullmatch(identifier):
            raise GateManifestError(f"{context}[{index}].id is malformed")
        ids.append(identifier)
        require_type_items.append(item)
    if ids != sorted(ids) or len(ids) != len(set(ids)):
        raise GateManifestError(f"{context} IDs must be sorted and unique")
    return require_type_items


def entrypoint_counts() -> dict[str, int]:
    return {
        "checkEntrypoints": len(list((ROOT / "scripts").glob("check_*.sh"))),
        "updateEntrypoints": len(list((ROOT / "scripts").glob("update_*.sh"))),
        "renderEntrypoints": len(list((ROOT / "scripts").glob("render_*.sh"))),
    }


def validate_snapshot(value: Any, context: str) -> dict[str, int]:
    require_type(value, dict, context)
    require_closed(value, GOVERNANCE_SNAPSHOT_FIELDS, context)
    return {
        field: require_int(value[field], f"{context}.{field}")
        for field in GOVERNANCE_SNAPSHOT_FIELDS
    }


def validate_governance_policy(value: Any) -> None:
    context = "policy.governanceBudget"
    require_type(value, dict, context)
    require_closed(value, {"baseline", "ceilings", "retiredAliases", "rule"}, context)
    baseline = validate_snapshot(value["baseline"], f"{context}.baseline")
    ceilings = value["ceilings"]
    require_type(ceilings, dict, f"{context}.ceilings")
    require_closed(ceilings, ENTRYPOINT_CEILING_FIELDS, f"{context}.ceilings")
    ceilings = {
        field: require_int(ceilings[field], f"{context}.ceilings.{field}")
        for field in ENTRYPOINT_CEILING_FIELDS
    }
    if value["rule"] != "one-in-one-out-distinct-trust-boundary":
        raise GateManifestError("governance entrypoint budget rule drift")

    retired = value["retiredAliases"]
    require_type(retired, list, f"{context}.retiredAliases")
    if not retired:
        raise GateManifestError("governance budget lacks consolidation records")
    paths = []
    retired_by_surface = {"check": 0, "update": 0, "render": 0}
    for index, item in enumerate(retired):
        item_context = f"{context}.retiredAliases[{index}]"
        require_type(item, dict, item_context)
        require_closed(item, RETIRED_ALIAS_FIELDS, item_context)
        path = validate_repo_path(item["path"], f"{item_context}.path")
        replacement = validate_repo_path(
            item["replacement"], f"{item_context}.replacement", must_exist=True
        )
        surface = item["surface"]
        if surface not in retired_by_surface:
            raise GateManifestError(f"{item_context}.surface is invalid")
        prefix = f"scripts/{surface}_"
        if not path.startswith(prefix) or not replacement.startswith(prefix):
            raise GateManifestError(f"{item_context} surface/path mismatch")
        if (ROOT / path).exists():
            raise GateManifestError(f"retired governance alias still exists: {path}")
        require_int(item["declaredDurationSeconds"], f"{item_context}.declaredDurationSeconds")
        require_int(item["declaredDiskMiB"], f"{item_context}.declaredDiskMiB")
        paths.append(path)
        retired_by_surface[surface] += 1
    if paths != sorted(set(paths)):
        raise GateManifestError("retired governance aliases must be sorted and unique")

    current = entrypoint_counts()
    for field, surface in (
        ("checkEntrypoints", "check"),
        ("updateEntrypoints", "update"),
        ("renderEntrypoints", "render"),
    ):
        if current[field] > ceilings[field]:
            raise GateManifestError(f"governance entrypoint ceiling exceeded: {field}")
        if baseline[field] - current[field] != retired_by_surface[surface]:
            raise GateManifestError(f"governance consolidation inventory drift: {field}")
        if ceilings[field] > baseline[field]:
            raise GateManifestError(f"governance ceiling exceeds baseline: {field}")

    for pattern in ("check_*.sh", "update_*.sh"):
        for path in (ROOT / "scripts").glob(pattern):
            if "Compatibility entrypoint" in path.read_text(encoding="utf-8"):
                raise GateManifestError(
                    f"ungoverned compatibility entrypoint remains: {display_path(path)}"
                )


def validate_policy(policy: Any, audit: Any, prerequisites: Any) -> None:
    require_type(policy, dict, "policy")
    require_closed(policy, POLICY_FIELDS, "policy")
    if policy["kind"] != "genesis/gate-manifest-policy-v0.1" or policy["version"] != "0.1":
        raise GateManifestError("unsupported gate manifest policy identity")
    validate_governance_policy(policy["governanceBudget"])

    inventory = policy["inventory"]
    require_type(inventory, dict, "policy.inventory")
    require_closed(inventory, {"checkGlob", "boundaryAudit", "prerequisiteManifest"}, "policy.inventory")
    if inventory != {
        "checkGlob": "scripts/check_*.sh",
        "boundaryAudit": AUDIT_REL,
        "prerequisiteManifest": PREREQUISITES_REL,
    }:
        raise GateManifestError("gate inventory authority drift")

    prerequisite_platforms = {item["id"]: item["tier"] for item in prerequisites.get("platforms", [])}
    platform_items = validate_sorted_unique_objects(policy["platforms"], "policy.platforms")
    observed_platforms = {}
    for index, item in enumerate(platform_items):
        require_closed(item, {"id", "tier"}, f"policy.platforms[{index}]")
        tier = require_int(item["tier"], f"policy.platforms[{index}].tier", 1)
        observed_platforms[item["id"]] = tier
    if observed_platforms != prerequisite_platforms:
        raise GateManifestError("gate platforms must exactly mirror prerequisite platform tiers")
    default_platforms = require_unique_strings(policy["defaultPlatforms"], "policy.defaultPlatforms", allow_empty=False)
    if default_platforms != sorted(default_platforms) or not set(default_platforms) <= set(observed_platforms):
        raise GateManifestError("default gate platforms must be a sorted known subset")
    platform_overrides = policy["platformOverrides"]
    require_type(platform_overrides, dict, "policy.platformOverrides")
    audit_paths = {entry["path"] for entry in audit.get("entries", [])}
    for path, values in platform_overrides.items():
        if path not in audit_paths:
            raise GateManifestError(f"platform override references unknown gate: {path}")
        scoped = require_unique_strings(values, f"policy.platformOverrides.{path}", allow_empty=False)
        if scoped != sorted(scoped) or not set(scoped) <= set(observed_platforms):
            raise GateManifestError(f"platform override is not a sorted known subset: {path}")

    require_type(policy["profiles"], list, "policy.profiles")
    profile_items = []
    profile_ids_seen = []
    for index, item in enumerate(policy["profiles"]):
        require_type(item, dict, f"policy.profiles[{index}]")
        identifier = require_string(item.get("id"), f"policy.profiles[{index}].id")
        if not ID_RE.fullmatch(identifier):
            raise GateManifestError(f"policy.profiles[{index}].id is malformed")
        profile_ids_seen.append(identifier)
        profile_items.append(item)
    if len(profile_ids_seen) != len(set(profile_ids_seen)):
        raise GateManifestError("policy profile IDs must be unique")
    ranks = []
    for index, item in enumerate(profile_items):
        require_closed(item, {"id", "rank"}, f"policy.profiles[{index}]")
        ranks.append(require_int(item["rank"], f"policy.profiles[{index}].rank"))
    if ranks != list(range(len(ranks))):
        raise GateManifestError("policy profile ranks must be contiguous and ordered")

    input_items = validate_sorted_unique_objects(policy["inputSets"], "policy.inputSets")
    for index, item in enumerate(input_items):
        require_closed(item, {"id", "globs"}, f"policy.inputSets[{index}]")
        globs = require_unique_strings(item["globs"], f"policy.inputSets[{index}].globs", allow_empty=False)
        if globs != sorted(globs):
            raise GateManifestError(f"policy.inputSets[{index}].globs must be sorted")

    output_items = validate_sorted_unique_objects(policy["outputSets"], "policy.outputSets")
    for index, item in enumerate(output_items):
        require_closed(item, {"id", "class", "paths"}, f"policy.outputSets[{index}]")
        if item["class"] not in {"temporary", "rebuildable-cache", "retained-evidence"}:
            raise GateManifestError(f"policy.outputSets[{index}] has invalid class")
        paths = require_unique_strings(item["paths"], f"policy.outputSets[{index}].paths", allow_empty=False)
        if paths != sorted(paths):
            raise GateManifestError(f"policy.outputSets[{index}].paths must be sorted")

    class_defaults = policy["classDefaults"]
    require_type(class_defaults, dict, "policy.classDefaults")
    if set(class_defaults) != BOUNDARY_CLASSES:
        raise GateManifestError("policy class default inventory drift")
    input_ids = {item["id"] for item in input_items}
    profile_ids = {item["id"] for item in profile_items}
    prerequisite_tools = {item["id"] for item in prerequisites.get("tools", [])}
    tool_overrides = policy["toolOverrides"]
    require_type(tool_overrides, dict, "policy.toolOverrides")
    for path, values in tool_overrides.items():
        if path not in audit_paths:
            raise GateManifestError(f"tool override references unknown gate: {path}")
        scoped = require_unique_strings(values, f"policy.toolOverrides.{path}", allow_empty=False)
        if scoped != sorted(scoped) or not set(scoped) <= prerequisite_tools:
            raise GateManifestError(f"tool override is not a sorted declared subset: {path}")
    default_fields = {
        "kind", "inputSets", "profile", "expectedDurationSeconds", "diskBudgetMiB",
        "tools", "sharding",
    }
    for boundary_class, default in class_defaults.items():
        require_type(default, dict, f"policy.classDefaults.{boundary_class}")
        require_closed(default, default_fields, f"policy.classDefaults.{boundary_class}")
        if default["kind"] not in GATE_KINDS:
            raise GateManifestError(f"invalid default kind for {boundary_class}")
        if not set(require_unique_strings(default["inputSets"], f"{boundary_class}.inputSets")) <= input_ids:
            raise GateManifestError(f"{boundary_class} references unknown input set")
        if default["profile"] not in profile_ids:
            raise GateManifestError(f"{boundary_class} references unknown profile")
        require_int(default["expectedDurationSeconds"], f"{boundary_class}.expectedDurationSeconds", 1)
        require_int(default["diskBudgetMiB"], f"{boundary_class}.diskBudgetMiB", 1)
        tools = require_unique_strings(default["tools"], f"{boundary_class}.tools", allow_empty=False)
        if not set(tools) <= prerequisite_tools:
            raise GateManifestError(f"{boundary_class} references undeclared prerequisite tool")
        validate_sharding(default["sharding"], f"policy.classDefaults.{boundary_class}.sharding")

    kind_defaults = policy["kindDefaults"]
    require_type(kind_defaults, dict, "policy.kindDefaults")
    if set(kind_defaults) != GATE_KINDS:
        raise GateManifestError("policy kind default inventory drift")
    for kind, default in kind_defaults.items():
        require_type(default, dict, f"policy.kindDefaults.{kind}")
        require_closed(default, {"profile", "expectedDurationSeconds", "diskBudgetMiB"}, f"policy.kindDefaults.{kind}")
        if default["profile"] not in profile_ids:
            raise GateManifestError(f"kind {kind} references unknown profile")
        require_int(default["expectedDurationSeconds"], f"kind {kind} expected duration", 1)
        require_int(default["diskBudgetMiB"], f"kind {kind} disk budget", 1)

    audit_paths = {entry["path"] for entry in audit.get("entries", [])}
    discovered = discover_gate_paths()
    if audit_paths != discovered:
        raise GateManifestError("boundary audit and live gate inventory differ")
    for entry in audit["entries"]:
        path = validate_repo_path(entry["path"], "boundary audit gate path", must_exist=True)
        source_sha = digest_file(ROOT / path)
        if entry.get("sha256") != source_sha:
            raise GateManifestError(f"boundary audit source hash is stale: {path}")
        identity_rows = [{"path": path, "sha256": source_sha}]
        helpers = entry.get("execution_helper_closure")
        require_type(helpers, list, f"boundary audit helpers for {path}")
        helper_paths = []
        for helper_index, helper in enumerate(helpers):
            context = f"boundary audit helper {path}[{helper_index}]"
            require_type(helper, dict, context)
            require_closed(helper, {"path", "sha256"}, context)
            helper_path = validate_repo_path(helper["path"], f"{context}.path", must_exist=True)
            helper_sha = digest_file(ROOT / helper_path)
            if helper["sha256"] != helper_sha:
                raise GateManifestError(f"boundary audit helper hash is stale: {helper_path}")
            helper_paths.append(helper_path)
            identity_rows.append({"path": helper_path, "sha256": helper_sha})
        if helper_paths != sorted(helper_paths) or len(helper_paths) != len(set(helper_paths)):
            raise GateManifestError(f"boundary audit helper closure is not sorted and unique: {path}")
        execution_sha = sha256(
            json.dumps(identity_rows, sort_keys=True, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        if entry.get("execution_identity_sha256") != execution_sha:
            raise GateManifestError(f"boundary audit execution identity is stale: {path}")

    kind_overrides = policy["kindOverrides"]
    require_type(kind_overrides, dict, "policy.kindOverrides")
    if set(kind_overrides) != GATE_KINDS:
        raise GateManifestError("kind override categories drift")
    assigned: Dict[str, str] = {}
    for kind, paths in kind_overrides.items():
        paths = require_unique_strings(paths, f"policy.kindOverrides.{kind}")
        if paths != sorted(paths):
            raise GateManifestError(f"policy.kindOverrides.{kind} must be sorted")
        for path in paths:
            validate_repo_path(path, f"policy.kindOverrides.{kind}", must_exist=True)
            if path not in audit_paths:
                raise GateManifestError(f"kind override references unknown gate: {path}")
            if path in assigned:
                raise GateManifestError(f"gate has multiple kind overrides: {path}")
            assigned[path] = kind

    network_overrides = policy["networkOverrides"]
    require_type(network_overrides, dict, "policy.networkOverrides")
    detected_network = {entry["path"] for entry in audit["entries"] if entry["network_detected"]}
    if set(network_overrides) != detected_network:
        raise GateManifestError("network overrides must exactly cover detected network gates")
    for path, network in network_overrides.items():
        validate_network(network, f"policy.networkOverrides.{path}")
        if network["mode"] not in {"external-optional", "external-required"}:
            raise GateManifestError(f"detected network gate {path} must declare external access")

    gate_overrides = policy["gateOverrides"]
    require_type(gate_overrides, dict, "policy.gateOverrides")
    override_fields = {"kind", "profile", "expectedDurationSeconds", "diskBudgetMiB"}
    for path, override in gate_overrides.items():
        if path not in audit_paths:
            raise GateManifestError(f"gate override references unknown gate: {path}")
        require_type(override, dict, f"policy.gateOverrides.{path}")
        require_closed(override, override_fields, f"policy.gateOverrides.{path}")
        if override["kind"] not in GATE_KINDS or override["profile"] not in profile_ids:
            raise GateManifestError(f"gate override identity drift: {path}")
        require_int(override["expectedDurationSeconds"], f"{path} expected duration", 1)
        require_int(override["diskBudgetMiB"], f"{path} disk budget", 1)


def validate_sharding(value: Any, context: str) -> None:
    require_type(value, dict, context)
    require_closed(value, {"mode", "maxShards", "isolation"}, context)
    if value["mode"] not in SHARD_MODES:
        raise GateManifestError(f"{context}.mode is invalid")
    max_shards = require_int(value["maxShards"], f"{context}.maxShards")
    if value["mode"] == "none" and max_shards != 1:
        raise GateManifestError(f"{context} non-shardable gates require maxShards=1")
    if value["mode"] != "none" and max_shards == 1:
        raise GateManifestError(f"{context} shardable gates require maxShards=0 or >1")
    if value["isolation"] not in ISOLATION_MODES:
        raise GateManifestError(f"{context}.isolation is invalid")


def validate_network(value: Any, context: str) -> None:
    require_type(value, dict, context)
    require_closed(value, {"mode", "declaredInputs"}, context)
    if value["mode"] not in NETWORK_MODES:
        raise GateManifestError(f"{context}.mode is invalid")
    inputs = require_unique_strings(value["declaredInputs"], f"{context}.declaredInputs")
    if value["mode"] in {"deny", "loopback-only"} and inputs:
        raise GateManifestError(f"{context} non-external mode cannot declare external inputs")
    if value["mode"].startswith("external-") and not inputs:
        raise GateManifestError(f"{context} external mode requires declared inputs")


def discover_gate_paths() -> set[str]:
    return {
        path.relative_to(ROOT).as_posix()
        for path in (ROOT / "scripts").glob("check_*.sh")
        if path.is_file()
    }


def gate_id(path: str) -> str:
    name = PurePosixPath(path).name
    return "gate/" + name.removeprefix("check_").removesuffix(".sh").replace("_", "-")


@lru_cache(maxsize=None)
def direct_tools(path: str) -> tuple[str, ...]:
    source_path = ROOT / path
    # Data and documentation can name every supported tool without executing
    # any of them. Attribute tools only from executable orchestration surfaces.
    if source_path.suffix not in {".sh", ".py", ".js", ".mjs"}:
        return ()
    source = source_path.read_text(encoding="utf-8")
    return tuple(tool for tool, pattern in TOOL_PATTERNS.items() if pattern.search(source))


def detect_tools(paths: Sequence[str], baseline: Sequence[str]) -> list[str]:
    tools = set(baseline)
    for path in paths:
        tools.update(direct_tools(path))
    return sorted(tools)


@lru_cache(maxsize=None)
def direct_repo_inputs(rel: str) -> tuple[str, ...]:
    source = (ROOT / rel).read_text(encoding="utf-8")
    candidates = set(REPO_REF_RE.findall(source)) | set(ROOT_REF_RE.findall(source))
    result = []
    for candidate in sorted(candidates):
        candidate = candidate.rstrip(".,:;)]}'\"")
        if (ROOT / candidate).is_file():
            result.append(candidate)
    return tuple(result)


def discover_repo_inputs(seed_paths: Sequence[str]) -> list[str]:
    seen = set(seed_paths)
    queue = list(seed_paths)
    while queue:
        rel = queue.pop()
        for candidate in direct_repo_inputs(rel):
            path = ROOT / candidate
            if candidate in seen:
                continue
            seen.add(candidate)
            if candidate.startswith("scripts/") and path.suffix in {".sh", ".py", ".mjs", ".js"}:
                queue.append(candidate)
    return sorted(seen)


@lru_cache(maxsize=None)
def _path_set_identity(paths: Tuple[str, ...]) -> str:
    rows = [
        {"path": path, "sha256": digest_file(ROOT / path)}
        for path in paths
        if path not in INPUT_IDENTITY_EXCLUSIONS
    ]
    for path, marker in INPUT_IDENTITY_EXCLUSIONS.items():
        if path in paths:
            rows.append({"path": path, "sha256": marker})
    return sha256(
        json.dumps(rows, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()


def path_set_identity(paths: Sequence[str]) -> str:
    return _path_set_identity(tuple(paths))


def render_manifest(policy: Any, audit: Any, prerequisites: Any) -> Mapping[str, Any]:
    validate_policy(policy, audit, prerequisites)
    kind_by_path = {
        path: kind
        for kind, paths in policy["kindOverrides"].items()
        for path in paths
    }
    platform_ids = [item["id"] for item in policy["platforms"]]
    default_platform_ids = policy["defaultPlatforms"]
    gates = []
    for audit_entry in sorted(audit["entries"], key=lambda item: item["path"]):
        path = audit_entry["path"]
        boundary_class = audit_entry["class"]
        default = deepcopy(policy["classDefaults"][boundary_class])
        kind = kind_by_path.get(path, default["kind"])
        default.update(policy["kindDefaults"][kind])
        if kind == "benchmark":
            default["sharding"] = deepcopy(policy["classDefaults"]["benchmark"]["sharding"])
        if path in policy["gateOverrides"]:
            default.update(policy["gateOverrides"][path])
            kind = default["kind"]
            if kind == "benchmark":
                default["sharding"] = deepcopy(policy["classDefaults"]["benchmark"]["sharding"])

        helper_paths = sorted(item["path"] for item in audit_entry["execution_helper_closure"])
        source_paths = discover_repo_inputs(sorted(set([path] + helper_paths)))
        outputs = ["temporary"]
        if audit_entry["compilation_detected"]:
            outputs.append("cargo-target")
        if kind == "benchmark":
            outputs.append("ephemeral-evidence")
        network = deepcopy(
            policy["networkOverrides"].get(
                path, {"mode": "deny", "declaredInputs": []}
            )
        )
        dependencies = sorted(
            f"scripts/{name}" for name in audit_entry["direct_check_invocations"]
        )
        gates.append(
            {
                "id": gate_id(path),
                "entrypoint": path,
                "sourceSha256": audit_entry["sha256"],
                "executionIdentitySha256": audit_entry["execution_identity_sha256"],
                "inputIdentitySha256": path_set_identity(source_paths),
                "boundaryClass": boundary_class,
                "kind": kind,
                "inputs": {
                    "sets": sorted(default["inputSets"]),
                    "paths": source_paths,
                },
                "outputs": {
                    "sets": sorted(outputs),
                    "retainedPaths": [],
                    "repositoryWrites": False,
                },
                "dependencies": dependencies,
                "profile": default["profile"],
                "expectedDurationSeconds": default["expectedDurationSeconds"],
                "diskBudgetMiB": default["diskBudgetMiB"],
                "network": network,
                "platforms": deepcopy(policy["platformOverrides"].get(path, default_platform_ids)),
                "tools": detect_tools(
                    sorted(set([path] + helper_paths)),
                    policy["toolOverrides"].get(path, default["tools"]),
                ),
                "sharding": deepcopy(default["sharding"]),
                "compilation": audit_entry["compilation_detected"],
                "readOnly": True,
            }
        )

    generated_from = []
    for rel in (AUDIT_REL, POLICY_REL, PREREQUISITES_REL):
        generated_from.append({"path": rel, "sha256": digest_file(ROOT / rel)})
    generated_from.sort(key=lambda item: item["path"])
    governance_policy = policy["governanceBudget"]
    baseline = deepcopy(governance_policy["baseline"])
    current = {
        **entrypoint_counts(),
        "declaredDurationSeconds": sum(gate["expectedDurationSeconds"] for gate in gates),
        "declaredDiskMiB": sum(gate["diskBudgetMiB"] for gate in gates),
    }
    delta = {field: current[field] - baseline[field] for field in GOVERNANCE_SNAPSHOT_FIELDS}
    if any(value > 0 for value in delta.values()):
        raise GateManifestError("governance inventory exceeded its R0 consolidation baseline")
    savings = {
        "declaredDurationSeconds": sum(
            item["declaredDurationSeconds"] for item in governance_policy["retiredAliases"]
        ),
        "declaredDiskMiB": sum(
            item["declaredDiskMiB"] for item in governance_policy["retiredAliases"]
        ),
    }
    for field in savings:
        if baseline[field] - current[field] != savings[field]:
            raise GateManifestError(f"retired governance envelope drift: {field}")
    return {
        "kind": "genesis/gate-manifest-v0.1",
        "version": "0.1",
        "inventory": {
            "checkGlob": "scripts/check_*.sh",
            "gateCount": len(gates),
            "generatedFrom": generated_from,
            "governanceBudget": {
                "baseline": baseline,
                "ceilings": deepcopy(governance_policy["ceilings"]),
                "current": current,
                "delta": delta,
                "retiredAliases": deepcopy(governance_policy["retiredAliases"]),
                "rule": governance_policy["rule"],
                "scheduledEnvelopeSavings": savings,
            },
        },
        "platforms": deepcopy(policy["platforms"]),
        "profiles": deepcopy(policy["profiles"]),
        "inputSets": deepcopy(policy["inputSets"]),
        "outputSets": deepcopy(policy["outputSets"]),
        "gates": gates,
    }


def validate_dependency_dag(gates: Mapping[str, Mapping[str, Any]]) -> None:
    state: Dict[str, int] = {}

    def visit(path: str, chain: list[str]) -> None:
        mark = state.get(path, 0)
        if mark == 1:
            raise GateManifestError(f"gate dependency cycle: {' -> '.join(chain + [path])}")
        if mark == 2:
            return
        state[path] = 1
        for dependency in gates[path]["dependencies"]:
            if dependency not in gates:
                raise GateManifestError(f"{path} references unknown gate dependency {dependency}")
            visit(dependency, chain + [path])
        state[path] = 2

    for path in gates:
        visit(path, [])


def validate_schema_marker() -> None:
    schema = load_json(ROOT / SCHEMA_REL)
    require_type(schema, dict, "gate manifest schema")
    if schema.get("$id") != "https://genesiscode.dev/schemas/gate-manifest-v0.1.json":
        raise GateManifestError("gate manifest schema $id drift")
    if schema.get("additionalProperties") is not False:
        raise GateManifestError("gate manifest schema root must be closed")
    gate = schema.get("$defs", {}).get("gate", {})
    if gate.get("additionalProperties") is not False:
        raise GateManifestError("gate manifest entry schema must be closed")
    kinds = set(gate.get("properties", {}).get("kind", {}).get("enum", []))
    if kinds != GATE_KINDS:
        raise GateManifestError("gate manifest schema kind inventory drift")


def validate_manifest(data: Any, expected: Any, prerequisites: Any) -> None:
    require_type(data, dict, "manifest")
    require_closed(data, MANIFEST_FIELDS, "manifest")
    if data["kind"] != "genesis/gate-manifest-v0.1" or data["version"] != "0.1":
        raise GateManifestError("unsupported gate manifest identity")
    inventory = data["inventory"]
    require_type(inventory, dict, "manifest.inventory")
    require_closed(
        inventory,
        {"checkGlob", "gateCount", "generatedFrom", "governanceBudget"},
        "manifest.inventory",
    )
    if inventory["checkGlob"] != "scripts/check_*.sh":
        raise GateManifestError("manifest inventory glob drift")
    require_int(inventory["gateCount"], "manifest.inventory.gateCount", 1)
    generated = inventory["generatedFrom"]
    require_type(generated, list, "manifest.inventory.generatedFrom")
    if len(generated) != 3:
        raise GateManifestError("manifest must bind exactly three generation authorities")
    for index, source in enumerate(generated):
        require_type(source, dict, f"manifest.inventory.generatedFrom[{index}]")
        require_closed(source, {"path", "sha256"}, f"manifest.inventory.generatedFrom[{index}]")
        validate_repo_path(source["path"], f"manifest.inventory.generatedFrom[{index}].path", must_exist=True)
        if not SHA_RE.fullmatch(require_string(source["sha256"], "generation sha256")):
            raise GateManifestError("generation authority has malformed sha256")

    governance = inventory["governanceBudget"]
    governance_context = "manifest.inventory.governanceBudget"
    require_type(governance, dict, governance_context)
    require_closed(
        governance,
        {"baseline", "ceilings", "current", "delta", "retiredAliases", "rule", "scheduledEnvelopeSavings"},
        governance_context,
    )
    validate_snapshot(governance["baseline"], f"{governance_context}.baseline")
    current_governance = validate_snapshot(
        governance["current"], f"{governance_context}.current"
    )
    ceilings = governance["ceilings"]
    require_type(ceilings, dict, f"{governance_context}.ceilings")
    require_closed(ceilings, ENTRYPOINT_CEILING_FIELDS, f"{governance_context}.ceilings")
    for field in ENTRYPOINT_CEILING_FIELDS:
        ceiling = require_int(ceilings[field], f"{governance_context}.ceilings.{field}")
        if ceiling < current_governance[field]:
            raise GateManifestError(f"manifest governance ceiling exceeded: {field}")
    delta = governance["delta"]
    require_type(delta, dict, f"{governance_context}.delta")
    require_closed(delta, GOVERNANCE_SNAPSHOT_FIELDS, f"{governance_context}.delta")
    if any(
        not isinstance(delta[field], int)
        or isinstance(delta[field], bool)
        or delta[field] > 0
        for field in GOVERNANCE_SNAPSHOT_FIELDS
    ):
        raise GateManifestError("manifest governance delta must be non-positive integers")
    if governance["rule"] != "one-in-one-out-distinct-trust-boundary":
        raise GateManifestError("manifest governance rule drift")
    if governance["retiredAliases"] != expected["inventory"]["governanceBudget"]["retiredAliases"]:
        raise GateManifestError("manifest retired alias inventory drift")
    savings = governance["scheduledEnvelopeSavings"]
    require_type(savings, dict, f"{governance_context}.scheduledEnvelopeSavings")
    require_closed(
        savings,
        {"declaredDurationSeconds", "declaredDiskMiB"},
        f"{governance_context}.scheduledEnvelopeSavings",
    )
    for field in ("declaredDurationSeconds", "declaredDiskMiB"):
        require_int(savings[field], f"{governance_context}.scheduledEnvelopeSavings.{field}")

    input_ids = {item["id"] for item in validate_sorted_unique_objects(data["inputSets"], "manifest.inputSets")}
    output_ids = {item["id"] for item in validate_sorted_unique_objects(data["outputSets"], "manifest.outputSets")}
    require_type(data["profiles"], list, "manifest.profiles")
    profile_ids = set()
    profile_ranks = []
    for index, item in enumerate(data["profiles"]):
        require_type(item, dict, f"manifest.profiles[{index}]")
        require_closed(item, {"id", "rank"}, f"manifest.profiles[{index}]")
        identifier = require_string(item["id"], f"manifest.profiles[{index}].id")
        if identifier in profile_ids:
            raise GateManifestError("manifest profile IDs must be unique")
        profile_ids.add(identifier)
        profile_ranks.append(require_int(item["rank"], f"manifest.profiles[{index}].rank"))
    if profile_ranks != list(range(len(profile_ranks))):
        raise GateManifestError("manifest profile ranks must be contiguous and ordered")
    platform_ids = {item["id"] for item in validate_sorted_unique_objects(data["platforms"], "manifest.platforms")}
    prerequisite_tools = {item["id"] for item in prerequisites.get("tools", [])}

    raw_gates = data["gates"]
    require_type(raw_gates, list, "manifest.gates")
    if inventory["gateCount"] != len(raw_gates):
        raise GateManifestError("manifest gate count does not match inventory")
    if current_governance["checkEntrypoints"] != len(raw_gates):
        raise GateManifestError("manifest governance check count does not match gate inventory")
    gates: Dict[str, Mapping[str, Any]] = {}
    ids = []
    previous_path = ""
    for index, gate in enumerate(raw_gates):
        context = f"manifest.gates[{index}]"
        require_type(gate, dict, context)
        require_closed(gate, GATE_FIELDS, context)
        path = validate_repo_path(gate["entrypoint"], f"{context}.entrypoint", must_exist=True)
        if path <= previous_path:
            raise GateManifestError("manifest gates must be sorted by unique entrypoint")
        previous_path = path
        gates[path] = gate
        identifier = require_string(gate["id"], f"{context}.id")
        if identifier != gate_id(path):
            raise GateManifestError(f"{path} gate ID drift")
        ids.append(identifier)
        for field in ("sourceSha256", "executionIdentitySha256", "inputIdentitySha256"):
            if not SHA_RE.fullmatch(require_string(gate[field], f"{context}.{field}")):
                raise GateManifestError(f"{context}.{field} must be lowercase sha256")
        if gate["boundaryClass"] not in BOUNDARY_CLASSES or gate["kind"] not in GATE_KINDS:
            raise GateManifestError(f"{path} classification is invalid")
        inputs = gate["inputs"]
        require_type(inputs, dict, f"{context}.inputs")
        require_closed(inputs, {"sets", "paths"}, f"{context}.inputs")
        sets = require_unique_strings(inputs["sets"], f"{context}.inputs.sets", allow_empty=False)
        if sets != sorted(sets) or not set(sets) <= input_ids:
            raise GateManifestError(f"{path} input set contract drift")
        source_paths = require_unique_strings(inputs["paths"], f"{context}.inputs.paths", allow_empty=False)
        if source_paths != sorted(source_paths):
            raise GateManifestError(f"{path} input paths must be sorted")
        for source_path in source_paths:
            validate_repo_path(source_path, f"{context}.inputs.paths", must_exist=True)
        if path not in source_paths:
            raise GateManifestError(f"{path} does not include its entrypoint as an input")
        if gate["inputIdentitySha256"] != path_set_identity(source_paths):
            raise GateManifestError(f"{path} exact input identity is stale")
        outputs = gate["outputs"]
        require_type(outputs, dict, f"{context}.outputs")
        require_closed(outputs, {"sets", "retainedPaths", "repositoryWrites"}, f"{context}.outputs")
        output_sets = require_unique_strings(outputs["sets"], f"{context}.outputs.sets", allow_empty=False)
        if output_sets != sorted(output_sets) or not set(output_sets) <= output_ids:
            raise GateManifestError(f"{path} output set contract drift")
        if outputs["retainedPaths"] != [] or outputs["repositoryWrites"] is not False:
            raise GateManifestError(f"{path} violates the read-only gate boundary")
        dependencies = require_unique_strings(gate["dependencies"], f"{context}.dependencies")
        if dependencies != sorted(dependencies):
            raise GateManifestError(f"{path} dependencies must be sorted")
        for dependency in dependencies:
            validate_repo_path(dependency, f"{context}.dependencies", must_exist=True)
        if gate["profile"] not in profile_ids:
            raise GateManifestError(f"{path} references unknown profile")
        require_int(gate["expectedDurationSeconds"], f"{context}.expectedDurationSeconds", 1)
        require_int(gate["diskBudgetMiB"], f"{context}.diskBudgetMiB", 1)
        validate_network(gate["network"], f"{context}.network")
        platforms = require_unique_strings(gate["platforms"], f"{context}.platforms", allow_empty=False)
        if platforms != sorted(platforms) or not set(platforms) <= platform_ids:
            raise GateManifestError(f"{path} platform scope drift")
        tools = require_unique_strings(gate["tools"], f"{context}.tools", allow_empty=False)
        if tools != sorted(tools) or not set(tools) <= prerequisite_tools:
            raise GateManifestError(f"{path} uses undeclared tools")
        validate_sharding(gate["sharding"], f"{context}.sharding")
        if not isinstance(gate["compilation"], bool) or gate["readOnly"] is not True:
            raise GateManifestError(f"{path} boolean execution contract drift")
        if gate["compilation"] and "cargo-target" not in output_sets:
            raise GateManifestError(f"{path} compiles without declaring cargo-target output")
        if gate["kind"] == "benchmark" and gate["sharding"]["isolation"] != "isolated-worktree-and-cache":
            raise GateManifestError(f"benchmark gate {path} lacks isolated execution")
    if len(ids) != len(set(ids)):
        raise GateManifestError("manifest gate IDs are not unique")
    if set(gates) != discover_gate_paths():
        raise GateManifestError("manifest does not exactly cover live check entrypoints")
    validate_dependency_dag(gates)
    if data != expected:
        raise GateManifestError("manifest drift: run bash scripts/update_gate_manifest.sh")
    validate_schema_marker()


def expect_reject(name: str, candidate: Any, expected: Any, prerequisites: Any) -> None:
    try:
        validate_manifest(candidate, expected, prerequisites)
    except GateManifestError:
        return
    raise GateManifestError(f"negative control was accepted: {name}")


def run_self_test(expected: Any, prerequisites: Any) -> int:
    vectors = []
    mutated = deepcopy(expected)
    mutated["unknown"] = True
    vectors.append(("unknown-field", mutated))
    mutated = deepcopy(expected)
    mutated["gates"].pop()
    mutated["inventory"]["gateCount"] -= 1
    vectors.append(("missing-gate", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][1]["id"] = mutated["gates"][0]["id"]
    vectors.append(("duplicate-id", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][0]["sourceSha256"] = "0" * 64
    vectors.append(("source-drift", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][0]["inputIdentitySha256"] = "0" * 64
    vectors.append(("input-closure-drift", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][0]["dependencies"] = ["scripts/check_missing.sh"]
    vectors.append(("unknown-dependency", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][0]["inputs"]["paths"] = ["/Users/example/source"]
    vectors.append(("host-path", mutated))
    mutated = deepcopy(expected)
    network_gate = next(gate for gate in mutated["gates"] if gate["network"]["mode"].startswith("external-"))
    network_gate["network"] = {"mode": "deny", "declaredInputs": []}
    vectors.append(("network-downgrade", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][0]["outputs"]["repositoryWrites"] = True
    vectors.append(("repository-write", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][0]["diskBudgetMiB"] = 0
    vectors.append(("zero-disk-budget", mutated))
    mutated = deepcopy(expected)
    mutated["gates"][0]["profile"] = "unknown"
    vectors.append(("unknown-profile", mutated))
    mutated = deepcopy(expected)
    benchmark = next(gate for gate in mutated["gates"] if gate["kind"] == "benchmark")
    benchmark["sharding"]["isolation"] = "shared-cargo-cache"
    vectors.append(("benchmark-without-isolation", mutated))
    mutated = deepcopy(expected)
    compiled = next(gate for gate in mutated["gates"] if gate["compilation"])
    compiled["outputs"]["sets"].remove("cargo-target")
    vectors.append(("undeclared-build-output", mutated))
    mutated = deepcopy(expected)
    reference_host = next(gate for gate in mutated["gates"] if gate["entrypoint"] == "scripts/check_reference_host_profiles.sh")
    reference_host["platforms"] = ["darwin-arm64", "linux-x86-64"]
    vectors.append(("tier2-platform-scope-drift", mutated))
    mutated = deepcopy(expected)
    reference_host = next(gate for gate in mutated["gates"] if gate["entrypoint"] == "scripts/check_reference_host_profiles.sh")
    reference_host["tools"].remove("rustc")
    vectors.append(("probe-tool-scope-drift", mutated))
    mutated = deepcopy(expected)
    mutated["inventory"]["governanceBudget"]["ceilings"]["checkEntrypoints"] -= 1
    vectors.append(("governance-entrypoint-budget", mutated))
    for name, candidate in vectors:
        expect_reject(name, candidate, expected, prerequisites)
    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=reject_duplicate_keys)
    except GateManifestError:
        pass
    else:
        raise GateManifestError("duplicate JSON key negative control was accepted")
    print(f"gate-manifest: self-test ok (negative_controls={len(vectors) + 1})")
    return len(vectors) + 1


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--render", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        policy = load_json(ROOT / POLICY_REL)
        audit = load_json(ROOT / AUDIT_REL)
        prerequisites = load_json(ROOT / PREREQUISITES_REL)
        expected = render_manifest(policy, audit, prerequisites)
        if args.render:
            if args.self_test:
                raise GateManifestError("--self-test requires --check")
            sys.stdout.write(canonical_text(expected))
            return 0
        manifest = load_json(ROOT / MANIFEST_REL)
        validate_manifest(manifest, expected, prerequisites)
        counts: Dict[str, int] = {}
        for gate in manifest["gates"]:
            counts[gate["kind"]] = counts.get(gate["kind"], 0) + 1
        rendered_counts = ",".join(f"{key}={counts.get(key, 0)}" for key in sorted(GATE_KINDS))
        print(
            f"gate-manifest: ok (gates={len(manifest['gates'])} {rendered_counts} "
            f"identity={canonical_identity(manifest)})"
        )
        if args.self_test:
            run_self_test(expected, prerequisites)
        return 0
    except (GateManifestError, OSError) as exc:
        print(f"gate-manifest: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
