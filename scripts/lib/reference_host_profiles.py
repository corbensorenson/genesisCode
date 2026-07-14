#!/usr/bin/env python3
"""Validate reference hosts and render portable, unsigned E0 host observations."""

from __future__ import annotations

import argparse
import copy
from hashlib import sha256
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
from typing import Any, Dict, Iterable, Mapping, Optional, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
POLICY = ROOT / "policies/reference_host_profiles_v0.1.json"
POLICY_SCHEMA = ROOT / "docs/spec/REFERENCE_HOST_PROFILES_v0.1.schema.json"
OBSERVATION_SCHEMA = ROOT / "docs/spec/REFERENCE_HOST_OBSERVATION_v0.1.schema.json"
PREREQUISITES = ROOT / "genesis.prerequisites.json"

TOP_KEYS = {
    "kind", "version", "metadataDimensions", "observationPolicy", "profiles", "promotionPolicy"
}
PROFILE_KEYS = {
    "id", "platformId", "tier", "promotionStatus", "cpu", "memory", "filesystem",
    "operatingSystem", "compiler", "powerMode", "benchmarkControls"
}
OBSERVATION_KEYS = {
    "kind", "version", "profileId", "platformId", "tier", "promotionStatus", "metadata",
    "conformance", "identitySha256"
}
PLATFORMS = {
    "darwin-arm64": (1, "reference"),
    "linux-arm64": (2, "candidate"),
    "linux-x86-64": (1, "reference"),
    "windows-x86-64": (2, "candidate"),
}
DIMENSIONS = ["compiler", "cpu", "filesystem", "memory", "operating-system", "power-mode"]
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
VERSION_RE = re.compile(r"([0-9]+(?:\.[0-9]+){0,3})")
FORBIDDEN_PATH_RE = re.compile(r"(?:^|\s)(?:/Users/|/home/|[A-Za-z]:[\\/]Users[\\/]|~[\\/])")


class HostProfileError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise HostProfileError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys)
    except (OSError, json.JSONDecodeError) as exc:
        raise HostProfileError(f"cannot load {path}: {exc}") from exc


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def version_tuple(raw: str) -> Tuple[int, int, int]:
    match = VERSION_RE.search(raw)
    if match is None:
        raise HostProfileError(f"version is not numeric: {raw!r}")
    parts = [int(part) for part in match.group(1).split(".")[:3]]
    return tuple((parts + [0, 0, 0])[:3])  # type: ignore[return-value]


def require_keys(value: Any, expected: Iterable[str], label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict) or set(value) != set(expected):
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        raise HostProfileError(f"{label} fields mismatch: {observed}")
    return value


def require_int(value: Any, label: str, minimum: int = 1) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise HostProfileError(f"{label} must be an integer >= {minimum}")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise HostProfileError(f"{label} must be a non-empty string")
    return value


def validate_range(value: Any, label: str) -> Mapping[str, str]:
    row = require_keys(value, {"minInclusive", "maxExclusive"}, label)
    minimum = require_string(row["minInclusive"], label + ".minInclusive")
    maximum = require_string(row["maxExclusive"], label + ".maxExclusive")
    if version_tuple(minimum) >= version_tuple(maximum):
        raise HostProfileError(f"{label} is empty or reversed")
    return {"minInclusive": minimum, "maxExclusive": maximum}


def validate_schema(path: Path, schema_id: str, required: Iterable[str]) -> None:
    schema = load_json(path)
    if (
        not isinstance(schema, dict)
        or schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
        or schema.get("$id") != schema_id
        or schema.get("additionalProperties") is not False
        or set(schema.get("required", [])) != set(required)
    ):
        raise HostProfileError(f"schema identity or closure drift: {path.relative_to(ROOT)}")


def validate_profile(profile: Any, expected_platform: str) -> Mapping[str, Any]:
    row = require_keys(profile, PROFILE_KEYS, f"profile[{expected_platform}]")
    if row["platformId"] != expected_platform:
        raise HostProfileError(f"profile platform order/identity drift: {expected_platform}")
    tier, status = PLATFORMS[expected_platform]
    if row["tier"] != tier or row["promotionStatus"] != status:
        raise HostProfileError(f"profile tier/promotion drift: {expected_platform}")
    require_string(row["id"], "profile.id")

    cpu = require_keys(row["cpu"], {"architecture", "modelPattern", "minPhysicalCores", "minLogicalCores"}, "profile.cpu")
    architecture = expected_platform.split("-", 1)[1]
    if architecture == "x86-64":
        architecture = "x86_64"
    if cpu["architecture"] != architecture:
        raise HostProfileError(f"CPU architecture does not mirror platform: {expected_platform}")
    try:
        re.compile(require_string(cpu["modelPattern"], "profile.cpu.modelPattern"))
    except re.error as exc:
        raise HostProfileError(f"invalid CPU model pattern: {exc}") from exc
    physical = require_int(cpu["minPhysicalCores"], "profile.cpu.minPhysicalCores")
    logical = require_int(cpu["minLogicalCores"], "profile.cpu.minLogicalCores")
    if logical < physical:
        raise HostProfileError("logical-core minimum is below physical-core minimum")

    memory = require_keys(row["memory"], {"minBytes"}, "profile.memory")
    require_int(memory["minBytes"], "profile.memory.minBytes", 8589934592)
    filesystem = require_keys(row["filesystem"], {"allowedTypes", "caseSensitivity", "minBlockSizeBytes"}, "profile.filesystem")
    allowed_types = filesystem["allowedTypes"]
    if not isinstance(allowed_types, list) or not allowed_types or allowed_types != sorted(set(allowed_types)):
        raise HostProfileError("filesystem allowedTypes must be sorted and unique")
    if filesystem["caseSensitivity"] not in {"sensitive", "insensitive", "either"}:
        raise HostProfileError("invalid filesystem case sensitivity")
    require_int(filesystem["minBlockSizeBytes"], "profile.filesystem.minBlockSizeBytes", 512)

    operating_system = require_keys(row["operatingSystem"], {"family", "version"}, "profile.operatingSystem")
    if operating_system["family"] != expected_platform.split("-", 1)[0]:
        raise HostProfileError(f"OS family does not mirror platform: {expected_platform}")
    validate_range(operating_system["version"], "profile.operatingSystem.version")

    compiler = require_keys(row["compiler"], {"rustcVersion", "rustcHostPattern", "nativeFamilies", "nativeVersion"}, "profile.compiler")
    if compiler["rustcVersion"] != "1.90.0":
        raise HostProfileError("reference rustc version drift")
    try:
        re.compile(require_string(compiler["rustcHostPattern"], "profile.compiler.rustcHostPattern"))
    except re.error as exc:
        raise HostProfileError(f"invalid rustc host pattern: {exc}") from exc
    families = compiler["nativeFamilies"]
    if not isinstance(families, list) or not families or families != sorted(set(families)) or not set(families) <= {"apple-clang", "clang", "gcc", "msvc"}:
        raise HostProfileError("native compiler families must be sorted, unique, and known")
    validate_range(compiler["nativeVersion"], "profile.compiler.nativeVersion")

    power = require_keys(row["powerMode"], {"requiredSource", "lowPowerModeAllowed", "allowedGovernorModes"}, "profile.powerMode")
    if power["requiredSource"] != "ac" or power["lowPowerModeAllowed"] is not False:
        raise HostProfileError("reference measurements must require AC power with low-power mode disabled")
    governors = power["allowedGovernorModes"]
    if not isinstance(governors, list) or not governors or governors != sorted(set(governors)):
        raise HostProfileError("governor modes must be sorted and unique")

    controls = require_keys(row["benchmarkControls"], {"exclusiveHostRequired", "virtualizationAllowed", "thermalStateRequired", "backgroundLoadMaxPercent"}, "profile.benchmarkControls")
    if controls != {"exclusiveHostRequired": True, "virtualizationAllowed": False, "thermalStateRequired": "nominal", "backgroundLoadMaxPercent": 5}:
        raise HostProfileError("benchmark controls drift")
    return row


def validate_policy(policy: Any) -> Mapping[str, Any]:
    row = require_keys(policy, TOP_KEYS, "reference host policy")
    if row["kind"] != "genesis/reference-host-profiles-v0.1" or row["version"] != "0.1":
        raise HostProfileError("reference host policy identity drift")
    if row["metadataDimensions"] != DIMENSIONS:
        raise HostProfileError("reference host metadata dimensions drift")
    observation = require_keys(row["observationPolicy"], {"allowAbsolutePaths", "allowHostnames", "allowSerialNumbers", "allowUserNames", "identityAlgorithm", "storageClassBeforeSigning"}, "observationPolicy")
    if observation != {"allowAbsolutePaths": False, "allowHostnames": False, "allowSerialNumbers": False, "allowUserNames": False, "identityAlgorithm": "sha256-canonical-json", "storageClassBeforeSigning": "E0"}:
        raise HostProfileError("observation privacy/identity policy drift")
    profiles = row["profiles"]
    if not isinstance(profiles, list) or len(profiles) != 4:
        raise HostProfileError("reference host policy must contain exactly four profiles")
    expected_platforms = sorted(PLATFORMS)
    if [profile.get("platformId") for profile in profiles if isinstance(profile, dict)] != expected_platforms:
        raise HostProfileError("reference host profiles must be sorted by platformId")
    for profile_row, platform_id in zip(profiles, expected_platforms):
        validate_profile(profile_row, platform_id)
    promotion = require_keys(row["promotionPolicy"], {"minimumIndependentHosts", "minimumSamplesPerWorkload", "requiredEvidenceClass", "requiredOperatingSystems", "requireIndependentVerification", "requireSignedBaseline"}, "promotionPolicy")
    expected_promotion = {"minimumIndependentHosts": 2, "minimumSamplesPerWorkload": 30, "requiredEvidenceClass": "E3", "requiredOperatingSystems": 1, "requireIndependentVerification": True, "requireSignedBaseline": True}
    if promotion != expected_promotion:
        raise HostProfileError("tier promotion policy drift")

    prerequisites = load_json(PREREQUISITES)
    platforms = {item["id"]: item["tier"] for item in prerequisites.get("platforms", []) if isinstance(item, dict)}
    if platforms != {platform_id: tier for platform_id, (tier, _) in PLATFORMS.items()}:
        raise HostProfileError("reference hosts do not mirror prerequisite platform tiers")
    return row


def recursively_reject_private_material(value: Any) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key.lower() in {"hostname", "username", "serialnumber", "path", "cwd", "home"}:
                raise HostProfileError(f"forbidden host-specific observation field: {key}")
            recursively_reject_private_material(child)
    elif isinstance(value, list):
        for child in value:
            recursively_reject_private_material(child)
    elif isinstance(value, str) and FORBIDDEN_PATH_RE.search(value):
        raise HostProfileError("host-specific path material is forbidden")


def profile_map(policy: Mapping[str, Any]) -> Dict[str, Mapping[str, Any]]:
    return {profile["platformId"]: profile for profile in policy["profiles"]}


def in_range(value: str, constraint: Mapping[str, Any]) -> bool:
    current = version_tuple(value)
    return version_tuple(constraint["minInclusive"]) <= current < version_tuple(constraint["maxExclusive"])


def conformance_failures(profile_row: Mapping[str, Any], metadata: Mapping[str, Any]) -> list[str]:
    failures = []
    cpu = metadata["cpu"]
    expected_cpu = profile_row["cpu"]
    if cpu["architecture"] != expected_cpu["architecture"]:
        failures.append("cpu.architecture")
    if re.fullmatch(expected_cpu["modelPattern"], cpu["model"]) is None:
        failures.append("cpu.model")
    if cpu["physicalCores"] < expected_cpu["minPhysicalCores"]:
        failures.append("cpu.physical-cores")
    if cpu["logicalCores"] < expected_cpu["minLogicalCores"]:
        failures.append("cpu.logical-cores")
    if metadata["memory"]["totalBytes"] < profile_row["memory"]["minBytes"]:
        failures.append("memory.total-bytes")
    filesystem = metadata["filesystem"]
    expected_fs = profile_row["filesystem"]
    if filesystem["type"] not in expected_fs["allowedTypes"]:
        failures.append("filesystem.type")
    if filesystem["blockSizeBytes"] < expected_fs["minBlockSizeBytes"]:
        failures.append("filesystem.block-size")
    sensitivity = expected_fs["caseSensitivity"]
    if sensitivity != "either" and filesystem["caseSensitive"] != (sensitivity == "sensitive"):
        failures.append("filesystem.case-sensitivity")
    operating_system = metadata["operatingSystem"]
    expected_os = profile_row["operatingSystem"]
    if operating_system["family"] != expected_os["family"]:
        failures.append("operating-system.family")
    if not in_range(operating_system["version"], expected_os["version"]):
        failures.append("operating-system.version")
    compiler = metadata["compiler"]
    expected_compiler = profile_row["compiler"]
    if compiler["rustcVersion"] != expected_compiler["rustcVersion"]:
        failures.append("compiler.rustc-version")
    if re.fullmatch(expected_compiler["rustcHostPattern"], compiler["rustcHost"]) is None:
        failures.append("compiler.rustc-host")
    if compiler["nativeFamily"] not in expected_compiler["nativeFamilies"]:
        failures.append("compiler.native-family")
    elif not in_range(compiler["nativeVersion"], expected_compiler["nativeVersion"]):
        failures.append("compiler.native-version")
    power = metadata["powerMode"]
    expected_power = profile_row["powerMode"]
    if power["source"] != expected_power["requiredSource"]:
        failures.append("power-mode.source")
    if power["lowPowerMode"] and not expected_power["lowPowerModeAllowed"]:
        failures.append("power-mode.low-power")
    if power["governorMode"] not in expected_power["allowedGovernorModes"]:
        failures.append("power-mode.governor")
    return sorted(failures)


def validate_metadata(metadata: Any) -> Mapping[str, Any]:
    row = require_keys(metadata, {"cpu", "memory", "filesystem", "operatingSystem", "compiler", "powerMode"}, "observation.metadata")
    cpu = require_keys(row["cpu"], {"architecture", "model", "physicalCores", "logicalCores"}, "observation.cpu")
    if cpu["architecture"] not in {"arm64", "x86_64"}:
        raise HostProfileError("observation CPU architecture is unsupported")
    require_string(cpu["model"], "observation.cpu.model")
    require_int(cpu["physicalCores"], "observation.cpu.physicalCores")
    require_int(cpu["logicalCores"], "observation.cpu.logicalCores")
    memory = require_keys(row["memory"], {"totalBytes"}, "observation.memory")
    require_int(memory["totalBytes"], "observation.memory.totalBytes")
    filesystem = require_keys(row["filesystem"], {"type", "blockSizeBytes", "caseSensitive"}, "observation.filesystem")
    require_string(filesystem["type"], "observation.filesystem.type")
    require_int(filesystem["blockSizeBytes"], "observation.filesystem.blockSizeBytes")
    if not isinstance(filesystem["caseSensitive"], bool):
        raise HostProfileError("observation filesystem caseSensitive must be boolean")
    operating_system = require_keys(row["operatingSystem"], {"family", "version", "kernelRelease"}, "observation.operatingSystem")
    if operating_system["family"] not in {"darwin", "linux", "windows"}:
        raise HostProfileError("observation OS family is unsupported")
    version_tuple(require_string(operating_system["version"], "observation.operatingSystem.version"))
    require_string(operating_system["kernelRelease"], "observation.operatingSystem.kernelRelease")
    compiler = require_keys(row["compiler"], {"rustcVersion", "rustcHost", "nativeFamily", "nativeVersion"}, "observation.compiler")
    version_tuple(require_string(compiler["rustcVersion"], "observation.compiler.rustcVersion"))
    require_string(compiler["rustcHost"], "observation.compiler.rustcHost")
    if compiler["nativeFamily"] not in {"apple-clang", "clang", "gcc", "msvc"}:
        raise HostProfileError("observation native compiler family is unsupported")
    version_tuple(require_string(compiler["nativeVersion"], "observation.compiler.nativeVersion"))
    power = require_keys(row["powerMode"], {"source", "lowPowerMode", "governorMode"}, "observation.powerMode")
    if power["source"] not in {"ac", "battery", "unknown"} or not isinstance(power["lowPowerMode"], bool):
        raise HostProfileError("observation power mode is invalid")
    require_string(power["governorMode"], "observation.powerMode.governorMode")
    recursively_reject_private_material(row)
    return row


def observation_identity(document: Mapping[str, Any]) -> str:
    payload = {key: value for key, value in document.items() if key != "identitySha256"}
    return sha256(canonical_bytes(payload)).hexdigest()


def validate_observation(document: Any, policy: Mapping[str, Any], require_conformant: bool = False) -> Mapping[str, Any]:
    row = require_keys(document, OBSERVATION_KEYS, "host observation")
    if row["kind"] != "genesis/reference-host-observation-v0.1" or row["version"] != "0.1":
        raise HostProfileError("host observation identity drift")
    platform_id = row["platformId"]
    profiles = profile_map(policy)
    if platform_id not in profiles:
        raise HostProfileError(f"unknown host observation platform: {platform_id}")
    profile_row = profiles[platform_id]
    for field in ("profileId", "tier", "promotionStatus"):
        expected_field = {"profileId": "id", "tier": "tier", "promotionStatus": "promotionStatus"}[field]
        if row[field] != profile_row[expected_field]:
            raise HostProfileError(f"host observation {field} does not match policy")
    metadata = validate_metadata(row["metadata"])
    failures = conformance_failures(profile_row, metadata)
    expected_conformance = {"ok": not failures, "failures": failures}
    if row["conformance"] != expected_conformance:
        raise HostProfileError("host observation conformance was not derived from policy")
    identity = row["identitySha256"]
    if not isinstance(identity, str) or not SHA_RE.fullmatch(identity) or identity != observation_identity(row):
        raise HostProfileError("host observation identity mismatch")
    if require_conformant and failures:
        raise HostProfileError("host does not conform to reference profile: " + ",".join(failures))
    return row


def run(argv: Sequence[str], allow_failure: bool = False) -> str:
    try:
        proc = subprocess.run(argv, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=8, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        if allow_failure:
            return ""
        raise HostProfileError(f"host probe failed: {argv[0]}: {exc}") from exc
    if proc.returncode != 0 and not allow_failure:
        raise HostProfileError(f"host probe failed: {argv[0]} exit={proc.returncode}")
    return proc.stdout.strip()


def canonical_architecture(raw: str) -> str:
    value = raw.lower()
    if value in {"arm64", "aarch64"}:
        return "arm64"
    if value in {"x86_64", "amd64", "x64"}:
        return "x86_64"
    raise HostProfileError(f"unsupported host architecture: {raw}")


def current_platform_id() -> str:
    family = platform.system().lower()
    if family == "macos":
        family = "darwin"
    architecture = canonical_architecture(platform.machine())
    candidate = f"{family}-{architecture.replace('_', '-')}"
    if candidate not in PLATFORMS:
        raise HostProfileError(f"unsupported reference host platform: {candidate}")
    return candidate


def parse_rustc() -> Tuple[str, str]:
    output = run(["rustc", "-vV"])
    release = re.search(r"^release: ([0-9.]+)$", output, re.MULTILINE)
    host = re.search(r"^host: (\S+)$", output, re.MULTILINE)
    if release is None or host is None:
        raise HostProfileError("rustc -vV output is incomplete")
    return release.group(1), host.group(1)


def native_compiler(family: str) -> Tuple[str, str]:
    if family == "darwin":
        output = run(["xcrun", "clang", "--version"])
        match = re.search(r"Apple clang version ([0-9.]+)", output)
        if match is None:
            raise HostProfileError("Apple clang version is unavailable")
        return "apple-clang", match.group(1)
    if family == "linux":
        output = run(["cc", "--version"])
        if "clang" in output.lower():
            match = VERSION_RE.search(output)
            native_family = "clang"
        else:
            match = re.search(r"(?:gcc|GCC|\) )[^\n]*?([0-9]+(?:\.[0-9]+){1,3})", output)
            if match is None:
                match = VERSION_RE.search(output)
            native_family = "gcc"
        if match is None:
            raise HostProfileError("native C compiler version is unavailable")
        return native_family, match.group(1)
    output = run(["cl", "/Bv"])
    match = re.search(r"Version ([0-9.]+)", output)
    if match is None:
        raise HostProfileError("MSVC version is unavailable")
    return "msvc", match.group(1)


def darwin_metadata(architecture: str) -> Mapping[str, Any]:
    model = run(["sysctl", "-n", "machdep.cpu.brand_string"])
    physical = int(run(["sysctl", "-n", "hw.physicalcpu"]))
    logical = int(run(["sysctl", "-n", "hw.logicalcpu"]))
    memory = int(run(["sysctl", "-n", "hw.memsize"]))
    device = run(["df", "-P", str(ROOT)]).splitlines()[-1].split()[0]
    disk = run(["diskutil", "info", device])
    fs_match = re.search(r"Type \(Bundle\):\s+(\S+)", disk)
    block_match = re.search(r"Allocation Block Size:\s+([0-9]+) Bytes", disk)
    if fs_match is None or block_match is None:
        raise HostProfileError("workspace filesystem metadata is unavailable")
    fs_type = fs_match.group(1).lower()
    case_sensitive = "case-sensitive" in disk.lower() or fs_type in {"apfsx", "hfsx"}
    os_version = run(["sw_vers", "-productVersion"])
    power_source = "ac" if "AC Power" in run(["pmset", "-g", "ps"]) else "battery"
    custom = run(["pmset", "-g", "custom"])
    ac_section = custom.split("AC Power:", 1)[-1]
    low_match = re.search(r"^\s*lowpowermode\s+([01])$", ac_section, re.MULTILINE)
    low_power = low_match is not None and low_match.group(1) == "1"
    rustc_version, rustc_host = parse_rustc()
    native_family, native_version = native_compiler("darwin")
    return {
        "cpu": {"architecture": architecture, "model": model, "physicalCores": physical, "logicalCores": logical},
        "memory": {"totalBytes": memory},
        "filesystem": {"type": "apfs" if fs_type == "apfsx" else fs_type, "blockSizeBytes": int(block_match.group(1)), "caseSensitive": case_sensitive},
        "operatingSystem": {"family": "darwin", "version": os_version, "kernelRelease": platform.release()},
        "compiler": {"rustcVersion": rustc_version, "rustcHost": rustc_host, "nativeFamily": native_family, "nativeVersion": native_version},
        "powerMode": {"source": power_source, "lowPowerMode": low_power, "governorMode": "not-applicable"},
    }


def linux_metadata(architecture: str) -> Mapping[str, Any]:
    cpuinfo = Path("/proc/cpuinfo").read_text(encoding="utf-8", errors="replace")
    model_match = re.search(r"^(?:model name|Processor)\s*:\s*(.+)$", cpuinfo, re.MULTILINE)
    model = model_match.group(1).strip() if model_match else platform.processor().strip()
    if not model:
        raise HostProfileError("Linux CPU model is unavailable")
    pairs = set(re.findall(r"^physical id\s*:\s*(\d+).*?^core id\s*:\s*(\d+)", cpuinfo, re.MULTILINE | re.DOTALL))
    logical = os.cpu_count() or 1
    physical = len(pairs) if pairs else logical
    memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    fs_type = run(["stat", "-f", "-c", "%T", str(ROOT)]).lower()
    governor_path = Path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
    governor = governor_path.read_text(encoding="utf-8").strip().lower() if governor_path.is_file() else "unknown"
    battery_present = any((path / "type").is_file() and (path / "type").read_text(encoding="utf-8").strip() == "Battery" for path in Path("/sys/class/power_supply").glob("*") if path.is_dir())
    source = "ac" if not battery_present else "unknown"
    rustc_version, rustc_host = parse_rustc()
    native_family, native_version = native_compiler("linux")
    return {
        "cpu": {"architecture": architecture, "model": model, "physicalCores": physical, "logicalCores": logical},
        "memory": {"totalBytes": memory},
        "filesystem": {"type": fs_type, "blockSizeBytes": os.statvfs(ROOT).f_frsize, "caseSensitive": True},
        "operatingSystem": {"family": "linux", "version": platform.release().split("-", 1)[0], "kernelRelease": platform.release()},
        "compiler": {"rustcVersion": rustc_version, "rustcHost": rustc_host, "nativeFamily": native_family, "nativeVersion": native_version},
        "powerMode": {"source": source, "lowPowerMode": False, "governorMode": governor},
    }


def powershell(expression: str) -> str:
    return run(["powershell", "-NoProfile", "-NonInteractive", "-Command", expression])


def windows_metadata(architecture: str) -> Mapping[str, Any]:
    cpu_json = json.loads(powershell("Get-CimInstance Win32_Processor | Select-Object -First 1 Name,NumberOfCores,NumberOfLogicalProcessors | ConvertTo-Json -Compress"))
    memory = int(powershell("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"))
    fs_type = powershell("(Get-Volume -DriveLetter (Get-Location).Drive.Name).FileSystem").lower()
    batteries = powershell("@(Get-CimInstance Win32_Battery).Count")
    scheme = run(["powercfg", "/getactivescheme"], allow_failure=True).lower()
    governor = "ultimate-performance" if "ultimate performance" in scheme else "high-performance" if "high performance" in scheme else "balanced"
    rustc_version, rustc_host = parse_rustc()
    native_family, native_version = native_compiler("windows")
    return {
        "cpu": {"architecture": architecture, "model": str(cpu_json["Name"]).strip(), "physicalCores": int(cpu_json["NumberOfCores"]), "logicalCores": int(cpu_json["NumberOfLogicalProcessors"])},
        "memory": {"totalBytes": memory},
        "filesystem": {"type": fs_type, "blockSizeBytes": os.statvfs(ROOT).f_frsize if hasattr(os, "statvfs") else 4096, "caseSensitive": False},
        "operatingSystem": {"family": "windows", "version": platform.version(), "kernelRelease": platform.release()},
        "compiler": {"rustcVersion": rustc_version, "rustcHost": rustc_host, "nativeFamily": native_family, "nativeVersion": native_version},
        "powerMode": {"source": "ac" if batteries == "0" else "unknown", "lowPowerMode": governor == "balanced", "governorMode": governor},
    }


def probe(policy: Mapping[str, Any]) -> Mapping[str, Any]:
    platform_id = current_platform_id()
    family, architecture_raw = platform_id.split("-", 1)
    architecture = "x86_64" if architecture_raw == "x86-64" else architecture_raw
    if family == "darwin":
        metadata = darwin_metadata(architecture)
    elif family == "linux":
        metadata = linux_metadata(architecture)
    else:
        metadata = windows_metadata(architecture)
    profile_row = profile_map(policy)[platform_id]
    failures = conformance_failures(profile_row, metadata)
    document: Dict[str, Any] = {
        "kind": "genesis/reference-host-observation-v0.1",
        "version": "0.1",
        "profileId": profile_row["id"],
        "platformId": platform_id,
        "tier": profile_row["tier"],
        "promotionStatus": profile_row["promotionStatus"],
        "metadata": metadata,
        "conformance": {"ok": not failures, "failures": failures},
    }
    document["identitySha256"] = observation_identity(document)
    validate_observation(document, policy)
    return document


def synthetic_observation(profile_row: Mapping[str, Any]) -> Mapping[str, Any]:
    model = {
        "darwin-arm64": "Apple M1",
        "linux-arm64": "Neoverse-N1",
        "linux-x86-64": "AMD EPYC 7763",
        "windows-x86-64": "AMD EPYC 7763",
    }[profile_row["platformId"]]
    metadata = {
        "cpu": {"architecture": profile_row["cpu"]["architecture"], "model": model, "physicalCores": profile_row["cpu"]["minPhysicalCores"], "logicalCores": profile_row["cpu"]["minLogicalCores"]},
        "memory": {"totalBytes": profile_row["memory"]["minBytes"]},
        "filesystem": {"type": profile_row["filesystem"]["allowedTypes"][0], "blockSizeBytes": profile_row["filesystem"]["minBlockSizeBytes"], "caseSensitive": profile_row["filesystem"]["caseSensitivity"] != "insensitive"},
        "operatingSystem": {"family": profile_row["operatingSystem"]["family"], "version": profile_row["operatingSystem"]["version"]["minInclusive"], "kernelRelease": profile_row["operatingSystem"]["version"]["minInclusive"]},
        "compiler": {"rustcVersion": profile_row["compiler"]["rustcVersion"], "rustcHost": profile_row["compiler"]["rustcHostPattern"].strip("^$"), "nativeFamily": profile_row["compiler"]["nativeFamilies"][0], "nativeVersion": profile_row["compiler"]["nativeVersion"]["minInclusive"]},
        "powerMode": {"source": "ac", "lowPowerMode": False, "governorMode": profile_row["powerMode"]["allowedGovernorModes"][0]},
    }
    failures = conformance_failures(profile_row, metadata)
    document: Dict[str, Any] = {"kind": "genesis/reference-host-observation-v0.1", "version": "0.1", "profileId": profile_row["id"], "platformId": profile_row["platformId"], "tier": profile_row["tier"], "promotionStatus": profile_row["promotionStatus"], "metadata": metadata, "conformance": {"ok": not failures, "failures": failures}}
    document["identitySha256"] = observation_identity(document)
    return document


def self_test(policy: Mapping[str, Any]) -> int:
    controls = 0
    for profile_row in policy["profiles"]:
        observation = synthetic_observation(profile_row)
        validate_observation(observation, policy, require_conformant=True)
        controls += 1

    candidate = copy.deepcopy(policy)
    candidate["profiles"][0]["tier"] = 2
    try:
        validate_policy(candidate)
    except HostProfileError:
        controls += 1
    else:
        raise HostProfileError("tier-drift control was accepted")

    observation = copy.deepcopy(synthetic_observation(policy["profiles"][0]))
    observation["identitySha256"] = "0" * 64
    try:
        validate_observation(observation, policy)
    except HostProfileError:
        controls += 1
    else:
        raise HostProfileError("identity-tamper control was accepted")

    observation = copy.deepcopy(synthetic_observation(policy["profiles"][0]))
    observation["metadata"]["cpu"]["model"] = "/Users/example/private"
    observation["conformance"] = {"ok": False, "failures": ["cpu.model"]}
    observation["identitySha256"] = observation_identity(observation)
    try:
        validate_observation(observation, policy)
    except HostProfileError:
        controls += 1
    else:
        raise HostProfileError("host-path leakage control was accepted")

    observation = copy.deepcopy(synthetic_observation(policy["profiles"][0]))
    observation["metadata"]["memory"]["totalBytes"] -= 1
    observation["identitySha256"] = observation_identity(observation)
    try:
        validate_observation(observation, policy)
    except HostProfileError:
        controls += 1
    else:
        raise HostProfileError("derived-conformance control was accepted")

    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=reject_duplicate_keys)
    except HostProfileError:
        controls += 1
    else:
        raise HostProfileError("duplicate-key control was accepted")
    print(f"reference-host-profiles: self-test ok (controls={controls})")
    return controls


def bundle_digest() -> str:
    paths = [POLICY, POLICY_SCHEMA, OBSERVATION_SCHEMA, PREREQUISITES, Path(__file__).resolve()]
    digest = sha256()
    for path in paths:
        rel = path.relative_to(ROOT).as_posix().encode("ascii")
        data = path.read_bytes()
        digest.update(len(rel).to_bytes(8, "big")); digest.update(rel)
        digest.update(len(data).to_bytes(8, "big")); digest.update(data)
    return digest.hexdigest()


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("check", "self-test", "probe", "verify-observation"))
    parser.add_argument("--policy", type=Path, default=POLICY)
    parser.add_argument("--observation", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--require-conformant", action="store_true")
    args = parser.parse_args(argv)
    try:
        validate_schema(POLICY_SCHEMA, "https://genesiscode.dev/schemas/reference-host-profiles-v0.1.json", TOP_KEYS)
        validate_schema(OBSERVATION_SCHEMA, "https://genesiscode.dev/schemas/reference-host-observation-v0.1.json", OBSERVATION_KEYS)
        policy = validate_policy(load_json(args.policy))
        if args.command == "check":
            tier1 = sum(1 for row in policy["profiles"] if row["tier"] == 1)
            tier2 = len(policy["profiles"]) - tier1
            print(f"reference-host-profiles: ok (profiles=4 tier1={tier1} tier2={tier2} dimensions=6 bundle={bundle_digest()})")
        elif args.command == "self-test":
            self_test(policy)
        elif args.command == "probe":
            if args.output is None:
                raise HostProfileError("probe requires --output")
            document = probe(policy)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_bytes(json.dumps(document, indent=2, sort_keys=True).encode("ascii") + b"\n")
            print(f"reference-host-observation: wrote {args.output} platform={document['platformId']} conformant={str(document['conformance']['ok']).lower()}")
        else:
            if args.observation is None:
                raise HostProfileError("verify-observation requires --observation")
            document = validate_observation(load_json(args.observation), policy, args.require_conformant)
            print(f"reference-host-observation: ok platform={document['platformId']} tier={document['tier']} conformant={str(document['conformance']['ok']).lower()} identity={document['identitySha256']}")
        return 0
    except (HostProfileError, OSError, UnicodeError, ValueError) as exc:
        print(f"reference-host-profiles: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
