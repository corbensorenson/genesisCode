#!/usr/bin/env python3
"""Validate and diagnose the GenesisCode prerequisite manifest without mutation."""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
import os
from pathlib import Path
import platform as host_platform
import re
import shutil
import subprocess
import sys
from typing import Any, Dict, List, Mapping, Optional, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = ROOT / "genesis.prerequisites.json"
SCHEMA_PATH = ROOT / "docs/spec/PREREQUISITES_v0.1.schema.json"
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
VERSION_RE = re.compile(r"^[0-9]+(?:\.[0-9]+){0,2}$")
TOOL_KEYS = {"id", "purpose", "source", "probe", "constraint"}
PROFILE_KEYS = {"id", "description", "requires", "optional", "requireNativeSdk", "platformIds"}
PLATFORM_KEYS = {"id", "operatingSystem", "architecture", "tier", "probes"}
PLATFORM_PROBE_KEYS = {"id", "argv", "versionRegex", "constraint"}
PROBE_KEYS = {"kind", "argv", "versionRegex", "packagePath", "versionField", "target"}
CONSTRAINT_KEYS = {"exact", "minInclusive", "maxExclusive"}
PROBE_KINDS = {"command-version", "command-presence", "node-package", "rustup-target"}
SAFE_COMMANDS = {
    ("adb", "version"),
    ("bash", "--version"),
    ("bash", "scripts/install_wasi_sdk.sh", "--version"),
    ("cargo", "--version"),
    ("cargo", "clippy", "--version"),
    ("cargo-deny", "--version"),
    ("cargo-fuzz", "--version"),
    ("cargo-nextest", "--version"),
    ("cc", "--version"),
    ("cl", "/Bv"),
    ("clang", "--version"),
    ("git", "--version"),
    ("idevice_id", "--version"),
    ("ios-deploy", "--version"),
    ("jq", "--version"),
    ("lake", "--version"),
    ("lean", "--version"),
    ("node", "--version"),
    ("npm", "--version"),
    ("python3", "--version"),
    ("rustc", "--version"),
    ("rustfmt", "--version"),
    ("shellcheck", "--version"),
    ("wasm-bindgen", "--version"),
    ("wasmtime", "--version"),
    ("xcodebuild", "-version"),
    ("xcrun", "--sdk", "macosx", "--show-sdk-version"),
    ("xcrun", "--version"),
    ("xcrun", "clang", "--version"),
}
EXPECTED_PROFILE_TOOLS = {
    "android-device": ({"adb"}, set()),
    "apple-device": ({"xcodebuild", "xcrun"}, {"idevice-id", "ios-deploy"}),
    "ci": ({"bash", "cargo", "cargo-deny", "cargo-nextest", "clippy", "git", "jq", "python", "rustc", "rustfmt"}, set()),
    "core": ({"bash", "cargo", "clippy", "git", "python", "rustc", "rustfmt"}, {"cargo-nextest", "jq", "shellcheck"}),
    "formal": ({"git", "lake", "lean"}, set()),
    "full": ({"bash", "cargo", "cargo-deny", "cargo-nextest", "clippy", "git", "jq", "lake", "lean", "node", "npm", "playwright", "python", "rust-target-wasm32-unknown-unknown", "rust-target-wasm32-wasip1", "rustc", "rustfmt", "wasi-sdk", "wasm-bindgen", "wasmtime"}, {"shellcheck"}),
    "fuzz": ({"cargo", "cargo-fuzz", "clang", "rustc"}, {"cargo-nextest"}),
    "wasi": ({"bash", "cargo", "python", "rust-target-wasm32-wasip1", "rustc", "wasi-sdk", "wasmtime"}, set()),
    "web": ({"bash", "cargo", "node", "npm", "playwright", "python", "rust-target-wasm32-unknown-unknown", "rustc", "wasm-bindgen"}, set()),
}


class PrerequisiteError(ValueError):
    pass


def duplicate_safe_object(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise PrerequisiteError("duplicate JSON key: %s" % key)
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=duplicate_safe_object,
            parse_float=lambda value: (_ for _ in ()).throw(
                PrerequisiteError("floating-point JSON is forbidden: %s" % value)
            ),
        )
    except FileNotFoundError as exc:
        raise PrerequisiteError("missing file: %s" % display_path(path)) from exc
    except json.JSONDecodeError as exc:
        raise PrerequisiteError(
            "invalid JSON in %s:%d:%d: %s"
            % (display_path(path), exc.lineno, exc.colno, exc.msg)
        ) from exc


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.name


def require_object(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise PrerequisiteError("%s must be an object" % label)
    return value


def require_exact_keys(value: Mapping[str, Any], expected: set, label: str) -> None:
    observed = set(value)
    missing = sorted(expected - observed)
    unknown = sorted(observed - expected)
    if missing:
        raise PrerequisiteError("%s missing fields: %s" % (label, ", ".join(missing)))
    if unknown:
        raise PrerequisiteError("%s contains unknown fields: %s" % (label, ", ".join(unknown)))


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise PrerequisiteError("%s must be a non-empty string" % label)
    return value


def require_id(value: Any, label: str) -> str:
    value = require_string(value, label)
    if not ID_RE.fullmatch(value):
        raise PrerequisiteError("%s must be a normalized identifier" % label)
    return value


def require_string_list(value: Any, label: str, *, ids: bool = False) -> List[str]:
    if not isinstance(value, list):
        raise PrerequisiteError("%s must be an array" % label)
    result = []
    for index, item in enumerate(value):
        result.append(require_id(item, "%s[%d]" % (label, index)) if ids else require_string(item, "%s[%d]" % (label, index)))
    return result


def version_tuple(value: str, label: str) -> Tuple[int, int, int]:
    if not VERSION_RE.fullmatch(value):
        raise PrerequisiteError("%s must be a numeric version with at most three components" % label)
    parts = [int(part) for part in value.split(".")]
    return tuple((parts + [0, 0])[:3])  # type: ignore[return-value]


def validate_constraint(value: Any, label: str) -> Optional[Mapping[str, str]]:
    if value is None:
        return None
    constraint = require_object(value, label)
    unknown = set(constraint) - CONSTRAINT_KEYS
    if unknown:
        raise PrerequisiteError("%s contains unknown fields: %s" % (label, ", ".join(sorted(unknown))))
    if not constraint:
        raise PrerequisiteError("%s must not be empty" % label)
    for key, version in constraint.items():
        version_tuple(require_string(version, "%s.%s" % (label, key)), "%s.%s" % (label, key))
    if "exact" in constraint and len(constraint) != 1:
        raise PrerequisiteError("%s exact cannot be combined with range bounds" % label)
    minimum = constraint.get("minInclusive")
    maximum = constraint.get("maxExclusive")
    if minimum and maximum and version_tuple(minimum, label) >= version_tuple(maximum, label):
        raise PrerequisiteError("%s has an empty or inverted range" % label)
    return constraint  # type: ignore[return-value]


def validate_probe(value: Any, constraint: Optional[Mapping[str, str]], label: str) -> Mapping[str, Any]:
    probe = require_object(value, label)
    if set(probe) - PROBE_KEYS:
        raise PrerequisiteError("%s contains unknown fields: %s" % (label, ", ".join(sorted(set(probe) - PROBE_KEYS))))
    kind = require_string(probe.get("kind"), "%s.kind" % label)
    if kind not in PROBE_KINDS:
        raise PrerequisiteError("%s.kind is unsupported: %s" % (label, kind))
    expected = {"kind"}
    if kind in {"command-version", "command-presence"}:
        expected.update({"argv", "versionRegex"})
        argv = require_string_list(probe.get("argv"), "%s.argv" % label)
        if len(argv) > 16:
            raise PrerequisiteError("%s.argv exceeds 16 entries" % label)
        if tuple(argv) not in SAFE_COMMANDS:
            raise PrerequisiteError("%s.argv is not an approved read-only probe" % label)
        regex = probe.get("versionRegex")
        if kind == "command-version":
            require_string(regex, "%s.versionRegex" % label)
            try:
                compiled = re.compile(regex)
            except re.error as exc:
                raise PrerequisiteError("%s.versionRegex is invalid: %s" % (label, exc)) from exc
            if compiled.groups != 1:
                raise PrerequisiteError("%s.versionRegex must contain exactly one capture group" % label)
            if constraint is None:
                raise PrerequisiteError("%s command-version requires a constraint" % label)
        elif regex is not None or constraint is not None:
            raise PrerequisiteError("%s command-presence cannot carry a version or constraint" % label)
    elif kind == "node-package":
        expected.update({"packagePath", "versionField"})
        path = require_string(probe.get("packagePath"), "%s.packagePath" % label)
        if path.startswith("/") or "\\" in path or ".." in Path(path).parts:
            raise PrerequisiteError("%s.packagePath must be repository-relative" % label)
        require_string(probe.get("versionField"), "%s.versionField" % label)
        if constraint is None:
            raise PrerequisiteError("%s node-package requires a constraint" % label)
    else:
        expected.update({"target"})
        require_string(probe.get("target"), "%s.target" % label)
        if constraint is None or set(constraint) != {"exact"}:
            raise PrerequisiteError("%s rustup-target requires an exact Rust version" % label)
    require_exact_keys(probe, expected, label)
    return probe


def validate_manifest(manifest: Any, *, check_sources: bool = True) -> Mapping[str, Any]:
    manifest = require_object(manifest, "manifest")
    require_exact_keys(manifest, {"kind", "version", "profiles", "platforms", "tools"}, "manifest")
    if manifest.get("kind") != "genesis/prerequisite-manifest-v0.1" or manifest.get("version") != "0.1":
        raise PrerequisiteError("manifest identity mismatch")

    tools_value = manifest.get("tools")
    if not isinstance(tools_value, list) or not tools_value:
        raise PrerequisiteError("manifest.tools must be a non-empty array")
    tool_ids: List[str] = []
    for index, raw_tool in enumerate(tools_value):
        label = "manifest.tools[%d]" % index
        tool = require_object(raw_tool, label)
        require_exact_keys(tool, TOOL_KEYS, label)
        tool_id = require_id(tool.get("id"), "%s.id" % label)
        tool_ids.append(tool_id)
        require_string(tool.get("purpose"), "%s.purpose" % label)
        require_string(tool.get("source"), "%s.source" % label)
        constraint = validate_constraint(tool.get("constraint"), "%s.constraint" % label)
        validate_probe(tool.get("probe"), constraint, "%s.probe" % label)
    require_sorted_unique(tool_ids, "manifest tool ids")
    known_tools = set(tool_ids)

    profiles_value = manifest.get("profiles")
    if not isinstance(profiles_value, list) or not profiles_value:
        raise PrerequisiteError("manifest.profiles must be a non-empty array")
    profile_ids: List[str] = []
    for index, raw_profile in enumerate(profiles_value):
        label = "manifest.profiles[%d]" % index
        profile = require_object(raw_profile, label)
        require_exact_keys(profile, PROFILE_KEYS, label)
        profile_id = require_id(profile.get("id"), "%s.id" % label)
        profile_ids.append(profile_id)
        require_string(profile.get("description"), "%s.description" % label)
        required = require_string_list(profile.get("requires"), "%s.requires" % label, ids=True)
        optional = require_string_list(profile.get("optional"), "%s.optional" % label, ids=True)
        platform_ids = require_string_list(profile.get("platformIds"), "%s.platformIds" % label, ids=True)
        require_sorted_unique(required, "%s.requires" % label)
        require_sorted_unique(optional, "%s.optional" % label)
        require_sorted_unique(platform_ids, "%s.platformIds" % label)
        if not platform_ids:
            raise PrerequisiteError("%s.platformIds must not be empty" % label)
        if set(required) & set(optional):
            raise PrerequisiteError("%s required and optional tools overlap" % label)
        unknown = (set(required) | set(optional)) - known_tools
        if unknown:
            raise PrerequisiteError("%s references unknown tools: %s" % (label, ", ".join(sorted(unknown))))
        if not isinstance(profile.get("requireNativeSdk"), bool):
            raise PrerequisiteError("%s.requireNativeSdk must be boolean" % label)
        expected_required, expected_optional = EXPECTED_PROFILE_TOOLS.get(profile_id, (None, None))
        if expected_required is None or set(required) != expected_required or set(optional) != expected_optional:
            raise PrerequisiteError("%s tool membership does not match the versioned profile contract" % label)
    require_sorted_unique(profile_ids, "manifest profile ids")
    if set(profile_ids) != set(EXPECTED_PROFILE_TOOLS):
        raise PrerequisiteError("manifest profile set does not match the versioned profile contract")

    platforms_value = manifest.get("platforms")
    if not isinstance(platforms_value, list) or not platforms_value:
        raise PrerequisiteError("manifest.platforms must be a non-empty array")
    platform_ids: List[str] = []
    platform_matches = set()
    for index, raw_platform in enumerate(platforms_value):
        label = "manifest.platforms[%d]" % index
        platform = require_object(raw_platform, label)
        require_exact_keys(platform, PLATFORM_KEYS, label)
        platform_id = require_id(platform.get("id"), "%s.id" % label)
        platform_ids.append(platform_id)
        operating_system = require_string(platform.get("operatingSystem"), "%s.operatingSystem" % label)
        architecture = require_string(platform.get("architecture"), "%s.architecture" % label)
        if operating_system not in {"darwin", "linux", "windows"} or architecture not in {"arm64", "x86_64"}:
            raise PrerequisiteError("%s has unsupported host match" % label)
        match = (operating_system, architecture)
        if match in platform_matches:
            raise PrerequisiteError("duplicate platform match: %s/%s" % match)
        platform_matches.add(match)
        if platform.get("tier") not in {1, 2}:
            raise PrerequisiteError("%s.tier must be 1 or 2" % label)
        probes = platform.get("probes")
        if not isinstance(probes, list) or not probes:
            raise PrerequisiteError("%s.probes must be a non-empty array" % label)
        probe_ids = []
        for probe_index, raw_probe in enumerate(probes):
            probe_label = "%s.probes[%d]" % (label, probe_index)
            probe = require_object(raw_probe, probe_label)
            require_exact_keys(probe, PLATFORM_PROBE_KEYS, probe_label)
            probe_ids.append(require_id(probe.get("id"), "%s.id" % probe_label))
            argv = require_string_list(probe.get("argv"), "%s.argv" % probe_label)
            if len(argv) > 16:
                raise PrerequisiteError("%s.argv exceeds 16 entries" % probe_label)
            if tuple(argv) not in SAFE_COMMANDS:
                raise PrerequisiteError("%s.argv is not an approved read-only probe" % probe_label)
            constraint = validate_constraint(probe.get("constraint"), "%s.constraint" % probe_label)
            regex = probe.get("versionRegex")
            if constraint is None:
                if regex is not None:
                    raise PrerequisiteError("%s presence probe cannot carry versionRegex" % probe_label)
            else:
                compiled = re.compile(require_string(regex, "%s.versionRegex" % probe_label))
                if compiled.groups != 1:
                    raise PrerequisiteError("%s.versionRegex must contain exactly one capture group" % probe_label)
        require_sorted_unique(probe_ids, "%s probe ids" % label)
    require_sorted_unique(platform_ids, "manifest platform ids")

    known_platforms = set(platform_ids)
    for index, profile in enumerate(profiles_value):
        unknown = set(profile["platformIds"]) - known_platforms
        if unknown:
            raise PrerequisiteError("manifest.profiles[%d] references unknown platforms: %s" % (index, ", ".join(sorted(unknown))))

    if check_sources:
        validate_source_pins(manifest)
    return manifest


def require_sorted_unique(values: Sequence[str], label: str) -> None:
    if list(values) != sorted(set(values)):
        raise PrerequisiteError("%s must be sorted and unique" % label)


def tool_by_id(manifest: Mapping[str, Any], tool_id: str) -> Mapping[str, Any]:
    for tool in manifest["tools"]:
        if tool["id"] == tool_id:
            return tool
    raise PrerequisiteError("missing tool: %s" % tool_id)


def exact_tool_version(manifest: Mapping[str, Any], tool_id: str) -> str:
    constraint = tool_by_id(manifest, tool_id)["constraint"]
    if not isinstance(constraint, dict) or set(constraint) != {"exact"}:
        raise PrerequisiteError("%s must have an exact source pin" % tool_id)
    return constraint["exact"]


def bounded_tool_range(manifest: Mapping[str, Any], tool_id: str) -> str:
    constraint = tool_by_id(manifest, tool_id)["constraint"]
    if not isinstance(constraint, dict) or set(constraint) != {"minInclusive", "maxExclusive"}:
        raise PrerequisiteError("%s must have a closed compatibility band" % tool_id)
    return ">=%s <%s" % (constraint["minInclusive"], constraint["maxExclusive"])


def toml_string_array(source: str, key: str) -> List[str]:
    match = re.search(r"^%s\s*=\s*\[([^\n]*)\]\s*$" % re.escape(key), source, re.MULTILINE)
    if not match:
        raise PrerequisiteError("rust-toolchain.toml %s is missing" % key)
    body = match.group(1)
    values = re.findall(r'"([^"\\]+)"', body)
    residue = re.sub(r'"[^"\\]+"', "", body).replace(",", "").strip()
    if residue or not values or values != sorted(set(values)):
        raise PrerequisiteError("rust-toolchain.toml %s must be a sorted string array" % key)
    return values


def validate_source_pins(manifest: Mapping[str, Any]) -> None:
    toolchain = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    channel_match = re.search(r'^channel\s*=\s*"([^"]+)"\s*$', toolchain, re.MULTILINE)
    if not channel_match:
        raise PrerequisiteError("rust-toolchain.toml channel is missing")
    rust_version = exact_tool_version(manifest, "rustc")
    if channel_match.group(1) != rust_version:
        raise PrerequisiteError("Rust prerequisite does not match rust-toolchain.toml")
    for tool_id in ("cargo", "rust-target-wasm32-unknown-unknown", "rust-target-wasm32-wasip1"):
        if exact_tool_version(manifest, tool_id) != rust_version:
            raise PrerequisiteError("%s must match the Rust toolchain" % tool_id)
    components = toml_string_array(toolchain, "components")
    if components != ["clippy", "rustfmt"]:
        raise PrerequisiteError("rust-toolchain.toml components do not match the prerequisite contract")
    target_ids = ("rust-target-wasm32-unknown-unknown", "rust-target-wasm32-wasip1")
    expected_targets = sorted(tool_by_id(manifest, tool_id)["probe"]["target"] for tool_id in target_ids)
    if toml_string_array(toolchain, "targets") != expected_targets:
        raise PrerequisiteError("rust-toolchain.toml targets do not match the prerequisite contract")

    package = load_json(ROOT / "package.json")
    lock = load_json(ROOT / "package-lock.json")
    playwright = exact_tool_version(manifest, "playwright")
    if package.get("devDependencies", {}).get("playwright") != playwright:
        raise PrerequisiteError("Playwright prerequisite does not match package.json")
    if lock.get("packages", {}).get("", {}).get("devDependencies", {}).get("playwright") != playwright:
        raise PrerequisiteError("Playwright prerequisite does not match package-lock.json")
    expected_engines = {"node": bounded_tool_range(manifest, "node"), "npm": bounded_tool_range(manifest, "npm")}
    if package.get("engines") != expected_engines or lock.get("packages", {}).get("", {}).get("engines") != expected_engines:
        raise PrerequisiteError("Node/npm engine declarations do not match the prerequisite profile")

    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    required_workflow_pins = [
        "toolchain: %s" % rust_version,
        "tool: cargo-deny@%s" % exact_tool_version(manifest, "cargo-deny"),
        "tool: nextest@%s" % exact_tool_version(manifest, "cargo-nextest"),
        "node-version: %d" % version_tuple(tool_by_id(manifest, "node")["constraint"]["minInclusive"], "Node minimum")[0],
        "targets: %s" % ", ".join(expected_targets),
        "--version %s --locked" % exact_tool_version(manifest, "wasm-bindgen"),
        "version: %s" % exact_tool_version(manifest, "wasmtime"),
    ]
    for pin in required_workflow_pins:
        if pin not in workflow:
            raise PrerequisiteError("CI workflow is missing prerequisite pin: %s" % pin)

    wasi_installer = (ROOT / "scripts/install_wasi_sdk.sh").read_text(encoding="utf-8")
    wasi_sdk_version = exact_tool_version(manifest, "wasi-sdk")
    if 'readonly WASI_SDK_VERSION="%s"' % wasi_sdk_version not in wasi_installer:
        raise PrerequisiteError("WASI SDK prerequisite does not match the verified installer")
    if "wasi-sdk-%s-x86_64-linux-sha256-" % wasi_sdk_version not in workflow:
        raise PrerequisiteError("CI workflow is missing the WASI SDK cache version pin")

    schema = load_json(SCHEMA_PATH)
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema" or schema.get("$id") != "https://genesiscode.dev/schemas/prerequisite-manifest-v0.1.json":
        raise PrerequisiteError("prerequisite schema identity mismatch")


def constraint_text(constraint: Optional[Mapping[str, str]]) -> str:
    if constraint is None:
        return "present"
    if "exact" in constraint:
        return "=%s" % constraint["exact"]
    parts = []
    if "minInclusive" in constraint:
        parts.append(">=%s" % constraint["minInclusive"])
    if "maxExclusive" in constraint:
        parts.append("<%s" % constraint["maxExclusive"])
    return " ".join(parts)


def satisfies(version: str, constraint: Optional[Mapping[str, str]]) -> bool:
    if constraint is None:
        return True
    observed = version_tuple(version, "observed version")
    if "exact" in constraint and observed != version_tuple(constraint["exact"], "exact"):
        return False
    if "minInclusive" in constraint and observed < version_tuple(constraint["minInclusive"], "minimum"):
        return False
    if "maxExclusive" in constraint and observed >= version_tuple(constraint["maxExclusive"], "maximum"):
        return False
    return True


def normalized_host() -> Tuple[str, str]:
    system = host_platform.system().lower()
    if system.startswith("msys") or system.startswith("mingw") or system == "windows":
        operating_system = "windows"
    elif system == "darwin":
        operating_system = "darwin"
    elif system == "linux":
        operating_system = "linux"
    else:
        operating_system = system
    machine = host_platform.machine().lower()
    architecture = "arm64" if machine in {"arm64", "aarch64"} else "x86_64" if machine in {"x86_64", "amd64"} else machine
    return operating_system, architecture


def run_command(argv: Sequence[str]) -> Tuple[str, Optional[str]]:
    executable = shutil.which(argv[0])
    if executable is None:
        return "missing", None
    env = os.environ.copy()
    env["LC_ALL"] = "C"
    env["LANG"] = "C"
    try:
        process = subprocess.run(
            list(argv),
            cwd=str(ROOT),
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return "probe-error", None
    output = process.stdout[:65536].strip()
    if process.returncode != 0:
        return "probe-error", output or None
    return "ok", output


def probe_command(check_id: str, argv: Sequence[str], regex: Optional[str], constraint: Optional[Mapping[str, str]], required: bool, scope: str) -> Mapping[str, Any]:
    status, output = run_command(argv)
    observed = None
    if status == "ok" and regex is not None:
        match = re.search(regex, output or "", re.MULTILINE)
        if not match:
            status = "unparseable"
        else:
            observed = match.group(1)
            try:
                status = "ok" if satisfies(observed, constraint) else "mismatch"
            except PrerequisiteError:
                status = "unparseable"
    return {
        "expected": constraint_text(constraint),
        "id": check_id,
        "observedVersion": observed,
        "required": required,
        "scope": scope,
        "status": status,
    }


def probe_tool(tool: Mapping[str, Any], required: bool, rust_version: str) -> Mapping[str, Any]:
    probe = tool["probe"]
    kind = probe["kind"]
    if kind in {"command-version", "command-presence"}:
        return probe_command(tool["id"], probe["argv"], probe["versionRegex"], tool["constraint"], required, "tool")
    if kind == "node-package":
        package_path = ROOT / probe["packagePath"]
        observed = None
        if not package_path.is_file():
            status = "missing"
        else:
            try:
                package = load_json(package_path)
                observed = package.get(probe["versionField"])
                if not isinstance(observed, str):
                    status = "unparseable"
                    observed = None
                else:
                    status = "ok" if satisfies(observed, tool["constraint"]) else "mismatch"
            except PrerequisiteError:
                status = "unparseable"
        return {"expected": constraint_text(tool["constraint"]), "id": tool["id"], "observedVersion": observed, "required": required, "scope": "tool", "status": status}
    status, output = run_command(["rustup", "target", "list", "--installed"])
    target = probe["target"]
    if status == "ok":
        status = "ok" if target in (output or "").splitlines() else "missing"
    return {"expected": "%s via Rust %s" % (target, rust_version), "id": tool["id"], "observedVersion": rust_version if status == "ok" else None, "required": required, "scope": "tool", "status": status}


def diagnose(manifest: Mapping[str, Any], profile_id: str, platform_id: Optional[str], manifest_sha256: str) -> Mapping[str, Any]:
    profiles = {profile["id"]: profile for profile in manifest["profiles"]}
    if profile_id not in profiles:
        raise PrerequisiteError("unknown prerequisite profile: %s" % profile_id)
    profile = profiles[profile_id]
    operating_system, architecture = normalized_host()
    platforms = {item["id"]: item for item in manifest["platforms"]}
    if platform_id is None:
        matches = [item for item in manifest["platforms"] if item["operatingSystem"] == operating_system and item["architecture"] == architecture]
        if len(matches) != 1:
            raise PrerequisiteError("host platform is not declared: %s/%s" % (operating_system, architecture))
        selected_platform = matches[0]
    else:
        if platform_id not in platforms:
            raise PrerequisiteError("unknown platform: %s" % platform_id)
        selected_platform = platforms[platform_id]
    if selected_platform["id"] not in profile["platformIds"]:
        raise PrerequisiteError("profile %s does not support platform %s" % (profile_id, selected_platform["id"]))

    tools = {tool["id"]: tool for tool in manifest["tools"]}
    checks = []
    rust_version = exact_tool_version(manifest, "rustc")
    for tool_id in profile["requires"]:
        checks.append(probe_tool(tools[tool_id], True, rust_version))
    for tool_id in profile["optional"]:
        checks.append(probe_tool(tools[tool_id], False, rust_version))
    if profile["requireNativeSdk"]:
        for probe in selected_platform["probes"]:
            checks.append(probe_command(probe["id"], probe["argv"], probe["versionRegex"], probe["constraint"], True, "platform"))

    required_failures = sum(1 for check in checks if check["required"] and check["status"] != "ok")
    optional_gaps = sum(1 for check in checks if not check["required"] and check["status"] != "ok")
    ok_count = sum(1 for check in checks if check["status"] == "ok")
    return {
        "checks": checks,
        "kind": "genesis/prerequisite-diagnostic-v0.1",
        "manifestSha256": manifest_sha256,
        "ok": required_failures == 0,
        "platform": {"architecture": selected_platform["architecture"], "id": selected_platform["id"], "operatingSystem": selected_platform["operatingSystem"], "tier": selected_platform["tier"]},
        "profile": profile_id,
        "summary": {"checks": len(checks), "ok": ok_count, "optionalGaps": optional_gaps, "requiredFailures": required_failures},
    }


def render_human(report: Mapping[str, Any]) -> str:
    lines = [
        "GenesisCode prerequisites: profile=%s platform=%s ok=%s"
        % (report["profile"], report["platform"]["id"], str(report["ok"]).lower())
    ]
    for check in report["checks"]:
        marker = "required" if check["required"] else "optional"
        observed = check["observedVersion"] or "-"
        lines.append("%-12s %-8s %-10s observed=%-10s expected=%s" % (check["status"], marker, check["id"], observed, check["expected"]))
    summary = report["summary"]
    lines.append("summary: checks=%d ok=%d required_failures=%d optional_gaps=%d" % (summary["checks"], summary["ok"], summary["requiredFailures"], summary["optionalGaps"]))
    return "\n".join(lines)


def pure_self_test() -> None:
    assert version_tuple("3.9", "fixture") == (3, 9, 0)
    assert satisfies("3.9.23", {"minInclusive": "3.9.0", "maxExclusive": "4.0.0"})
    assert not satisfies("4.0.0", {"minInclusive": "3.9.0", "maxExclusive": "4.0.0"})
    try:
        duplicate_safe_object([("version", "0.1"), ("version", "0.1")])
    except PrerequisiteError:
        pass
    else:
        raise PrerequisiteError("self-test accepted a duplicate key")
    manifest = load_json(DEFAULT_MANIFEST)
    validate_manifest(manifest)


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("validate")
    diagnose_parser = subparsers.add_parser("diagnose")
    diagnose_parser.add_argument("--profile", default="core")
    diagnose_parser.add_argument("--platform")
    diagnose_parser.add_argument("--format", choices=("human", "json"), default="human")
    subparsers.add_parser("list-profiles")
    subparsers.add_parser("self-test")
    args = parser.parse_args(argv)
    try:
        manifest = load_json(args.manifest.resolve())
        manifest = validate_manifest(manifest)
        if args.command == "validate":
            print("prerequisite-manifest: ok (profiles=%d platforms=%d tools=%d)" % (len(manifest["profiles"]), len(manifest["platforms"]), len(manifest["tools"])))
            return 0
        if args.command == "list-profiles":
            for profile in manifest["profiles"]:
                print("%s\t%s" % (profile["id"], profile["description"]))
            return 0
        if args.command == "self-test":
            pure_self_test()
            print("prerequisite-manifest-self-test: ok")
            return 0
        report = diagnose(manifest, args.profile, args.platform, sha256(args.manifest.resolve().read_bytes()).hexdigest())
        if args.format == "json":
            print(json.dumps(report, sort_keys=True, separators=(",", ":")))
        else:
            print(render_human(report))
        return 0 if report["ok"] else 2
    except (PrerequisiteError, OSError) as exc:
        print("prerequisite-manifest: %s" % exc, file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
