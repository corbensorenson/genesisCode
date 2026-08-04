#!/usr/bin/env python3
"""Build and verify closed health-profile evidence bundles."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys
from typing import Any


KIND = "genesis/health-profile-evidence-bundle-v0.2"
MAX_AGE_SECONDS = 21_600
ENV_PREFIXES = (
    "CARGO_",
    "CI",
    "GENESIS_",
    "NODE_",
    "NPM_",
    "PLAYWRIGHT_",
    "RUST",
    "WASMTIME_",
)
TOOLCHAINS = {
    "bash": ["--version"],
    "cargo": ["--version"],
    "git": ["--version"],
    "node": ["--version"],
    "npm": ["--version"],
    "python3": ["--version"],
    "rustc": ["--version", "--verbose"],
}

ARTIFACTS = {
    "agent_capability_gauntlet_report.json": (
        "genesis/agent-capability-gauntlet-v0.1",
        "agent-gauntlet",
    ),
    "agent_capability_gauntlet_history.jsonl": ("jsonl-history", "agent-timing"),
    "runtime_backend_feature_matrix_report.json": (
        "genesis/runtime-backend-feature-matrix-v0.1",
        "runtime-backend-matrix",
    ),
    "runtime_backend_feature_matrix_history.jsonl": ("jsonl-history", "runtime-timing"),
    "host_bridge_fault_injection_report.json": (
        "genesis/host-bridge-fault-injection-v0.1",
        "host-bridge-fault-injection",
    ),
    "host_bridge_fault_injection_history.jsonl": ("jsonl-history", "host-bridge-timing"),
    "webxr_browser_conformance_report.json": (
        "genesis/webxr-browser-conformance-v0.1",
        "webxr-runtime",
    ),
    "gpu_xr_productization_kits_report.json": (
        "genesis/gpu-xr-productization-kits-v0.1",
        "gpu-xr-productization",
    ),
    "assurance_profile_packs_report.json": (
        "genesis/assurance-profile-packs-v0.1",
        "assurance-profiles",
    ),
    "assurance_profile_packs_history.jsonl": ("jsonl-history", "assurance-history"),
    "agent_workflow_runtime_parity_report.json": (
        "genesis/agent-workflow-runtime-parity-v0.1",
        "runtime-parity",
    ),
    "agent_workflow_runtime_parity_history.jsonl": ("jsonl-history", "parity-timing"),
    "agent_capability_gauntlet_native_report.json": (
        "genesis/agent-capability-gauntlet-v0.1",
        "native-gauntlet",
    ),
    "agent_capability_gauntlet_native_history.jsonl": ("jsonl-history", "native-timing"),
    "agent_capability_gauntlet_wasi_report.json": (
        "genesis/agent-capability-gauntlet-v0.1",
        "wasi-gauntlet",
    ),
    "agent_capability_gauntlet_wasi_history.jsonl": ("jsonl-history", "wasi-timing"),
    "agent_generative_workloads_report.json": (
        "genesis/agent-generative-workloads-v0.1",
        "generative-workloads",
    ),
    "agent_generative_workloads_history.jsonl": ("jsonl-history", "generative-timing"),
}

CONSUMERS = {
    "agent-generative-workloads": {
        "script": "scripts/check_agent_generative_workloads.sh",
        "artifacts": [
            "agent_capability_gauntlet_native_report.json",
            "agent_capability_gauntlet_wasi_report.json",
            "agent_generative_workloads_history.jsonl",
            "agent_generative_workloads_report.json",
        ],
    },
    "agent-runtime-parity": {
        "script": "scripts/check_agent_workflow_runtime_parity.sh",
        "artifacts": [
            "agent_capability_gauntlet_native_history.jsonl",
            "agent_capability_gauntlet_native_report.json",
            "agent_capability_gauntlet_wasi_history.jsonl",
            "agent_capability_gauntlet_wasi_report.json",
            "agent_generative_workloads_history.jsonl",
            "agent_workflow_runtime_parity_history.jsonl",
            "agent_workflow_runtime_parity_report.json",
        ],
    },
    "agent-scenario-performance": {
        "script": "scripts/check_agent_scenario_perf.sh",
        "artifacts": [
            "agent_capability_gauntlet_history.jsonl",
            "agent_capability_gauntlet_report.json",
        ],
    },
    "gpu-xr-productization": {
        "script": "scripts/check_gpu_xr_productization_kits.sh",
        "artifacts": [
            "agent_capability_gauntlet_report.json",
            "gpu_xr_productization_kits_report.json",
            "webxr_browser_conformance_report.json",
        ],
    },
    "host-bridge-fault-injection": {
        "script": "scripts/check_host_bridge_fault_injection.sh",
        "artifacts": [
            "host_bridge_fault_injection_history.jsonl",
            "host_bridge_fault_injection_report.json",
        ],
    },
    "runtime-backend-matrix": {
        "script": "scripts/check_runtime_backend_feature_matrix.sh",
        "artifacts": ["runtime_backend_feature_matrix_report.json"],
    },
    "slo-report-contracts": {
        "script": "scripts/check_slo_report_contracts.sh",
        "artifacts": [
            "agent_capability_gauntlet_report.json",
            "agent_workflow_runtime_parity_report.json",
        ],
    },
    "write-skill-conformance": {
        "script": "scripts/check_write_genesiscode_skill_conformance.sh",
        "artifacts": [
            "agent_capability_gauntlet_report.json",
            "agent_generative_workloads_report.json",
            "assurance_profile_packs_report.json",
            "gpu_xr_productization_kits_report.json",
            "host_bridge_fault_injection_report.json",
            "runtime_backend_feature_matrix_report.json",
        ],
    },
    "write-skill-distribution": {
        "script": "scripts/check_write_genesiscode_skill_distribution.sh",
        "artifacts": [
            "agent_capability_gauntlet_report.json",
            "agent_generative_workloads_report.json",
            "assurance_profile_packs_report.json",
            "gpu_xr_productization_kits_report.json",
            "host_bridge_fault_injection_report.json",
            "runtime_backend_feature_matrix_report.json",
        ],
    },
}

PRODUCERS = {
    "agent-reference-workflows": {
        "command": "scripts/render_agent_reference_workflows_report.sh",
        "artifacts": [
            "agent_capability_gauntlet_history.jsonl",
            "agent_capability_gauntlet_report.json",
        ],
    },
    "agent-runtime-parity": {
        "command": "scripts/render_agent_workflow_runtime_parity_report.sh",
        "artifacts": [
            "agent_capability_gauntlet_native_history.jsonl",
            "agent_capability_gauntlet_native_report.json",
            "agent_capability_gauntlet_wasi_history.jsonl",
            "agent_capability_gauntlet_wasi_report.json",
            "agent_generative_workloads_history.jsonl",
            "agent_generative_workloads_report.json",
            "agent_workflow_runtime_parity_history.jsonl",
            "agent_workflow_runtime_parity_report.json",
        ],
    },
    "assurance-profile-packs": {
        "command": "scripts/render_assurance_profile_packs_report.sh",
        "artifacts": [
            "assurance_profile_packs_history.jsonl",
            "assurance_profile_packs_report.json",
        ],
    },
    "gpu-xr-productization": {
        "command": "scripts/render_gpu_xr_productization_kits_report.sh",
        "artifacts": ["gpu_xr_productization_kits_report.json"],
    },
    "host-bridge-fault-injection": {
        "command": "scripts/render_host_bridge_fault_injection_report.sh",
        "artifacts": [
            "host_bridge_fault_injection_history.jsonl",
            "host_bridge_fault_injection_report.json",
        ],
    },
    "runtime-backend-matrix": {
        "command": "scripts/render_runtime_backend_feature_matrix_report.sh",
        "artifacts": [
            "runtime_backend_feature_matrix_history.jsonl",
            "runtime_backend_feature_matrix_report.json",
        ],
    },
    "webxr-browser-conformance": {
        "command": "scripts/render_webxr_browser_conformance_report.sh",
        "artifacts": ["webxr_browser_conformance_report.json"],
    },
}


class EvidenceError(ValueError):
    pass


def fail(message: str) -> None:
    raise EvidenceError(message)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def object_identity(value: dict[str, Any]) -> str:
    clone = dict(value)
    clone.pop("contentIdentitySha256", None)
    return sha256(canonical(clone))


def exact_keys(value: Any, keys: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    observed = set(value)
    if observed != keys:
        fail(f"{label} fields mismatch: expected={sorted(keys)!r} observed={sorted(observed)!r}")
    return value


def command_output(command: list[str]) -> str:
    proc = subprocess.run(command, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        fail(f"toolchain command failed: {' '.join(command)} (exit={proc.returncode})")
    return (proc.stdout + proc.stderr).strip()


def toolchain_inventory() -> list[dict[str, Any]]:
    rows = []
    for name, args in sorted(TOOLCHAINS.items()):
        executable = shutil.which(name)
        if executable is None:
            fail(f"required toolchain executable is unavailable: {name}")
        resolved = pathlib.Path(executable).resolve(strict=True)
        rows.append(
            {
                "executableSha256": sha256(resolved.read_bytes()),
                "name": name,
                "versionSha256": sha256(command_output([executable, *args]).encode()),
            }
        )
    return rows


def source_inventory(root: pathlib.Path) -> dict[str, Any]:
    proc = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        fail("cannot enumerate semantic source inputs")
    paths = sorted(pathlib.PurePosixPath(raw.decode()) for raw in proc.stdout.split(b"\0") if raw)
    digest = hashlib.sha256()
    count = 0
    byte_count = 0
    for rel in paths:
        path = root / rel
        if not path.is_file():
            continue
        payload = path.read_bytes()
        mode = path.stat().st_mode & 0o777
        digest.update(rel.as_posix().encode())
        digest.update(b"\0")
        digest.update(f"{mode:o}".encode())
        digest.update(b"\0")
        digest.update(payload)
        digest.update(b"\0")
        count += 1
        byte_count += len(payload)
    head = command_output(["git", "-C", str(root), "rev-parse", "HEAD"])
    return {
        "byteCount": byte_count,
        "fileCount": count,
        "gitCommit": head,
        "semanticInputsSha256": digest.hexdigest(),
    }


def producer_environment(profile: str) -> dict[str, Any]:
    variables = []
    for name, value in sorted(os.environ.items()):
        if any(name == prefix or name.startswith(prefix) for prefix in ENV_PREFIXES):
            variables.append({"name": name, "valueSha256": sha256(value.encode())})
    return {"profile": profile, "variables": variables}


def execution_environment(profile: str) -> dict[str, Any]:
    tools = toolchain_inventory()
    core = {
        "architecture": platform.machine(),
        "operatingSystem": platform.system(),
        "operatingSystemRelease": platform.release(),
        "profile": profile,
        "toolchains": tools,
    }
    return {**core, "identitySha256": sha256(canonical(core))}


def validate_json_artifact(path: pathlib.Path, expected_kind: str) -> None:
    try:
        doc = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        fail(f"invalid JSON artifact {path.name}: {exc}")
    if doc.get("kind") != expected_kind:
        fail(f"artifact {path.name} kind mismatch: {doc.get('kind')!r}")
    if doc.get("ok") is not True:
        fail(f"artifact {path.name} reports ok=false")


def validate_jsonl_artifact(path: pathlib.Path) -> None:
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines:
        fail(f"history artifact is empty: {path.name}")
    for index, line in enumerate(lines, 1):
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            fail(f"history artifact {path.name} line {index} is invalid JSON: {exc}")
        if not isinstance(value, dict):
            fail(f"history artifact {path.name} line {index} must be an object")


def artifact_inventory(root: pathlib.Path) -> dict[str, Any]:
    rows = {}
    for name, (kind, evidence_class) in sorted(ARTIFACTS.items()):
        path = root / name
        if not path.is_file():
            fail(f"missing evidence artifact: {name}")
        if kind == "jsonl-history":
            validate_jsonl_artifact(path)
        else:
            validate_json_artifact(path, kind)
        payload = path.read_bytes()
        rows[name] = {
            "bytes": len(payload),
            "evidenceClass": evidence_class,
            "kind": kind,
            "sha256": sha256(payload),
        }
    return rows


def consumer_inventory(profile: str) -> dict[str, Any]:
    rows = {}
    for consumer_id, raw in sorted(CONSUMERS.items()):
        artifacts = sorted(raw["artifacts"])
        rows[consumer_id] = {
            "artifacts": artifacts,
            "evidenceClasses": sorted({ARTIFACTS[name][1] for name in artifacts}),
            "profile": profile,
            "script": raw["script"],
        }
    return rows


def producer_inventory(
    source: dict[str, Any],
    environment: dict[str, Any],
    execution: dict[str, Any],
) -> dict[str, Any]:
    rows = {}
    claimed = []
    for producer_id, raw in sorted(PRODUCERS.items()):
        artifacts = sorted(raw["artifacts"])
        claimed.extend(artifacts)
        declared_environment = declared_producer_environment(producer_id, environment["profile"])
        inputs = {
            "command": raw["command"],
            "declaredEnvironment": declared_environment,
            "executionEnvironmentIdentitySha256": execution["identitySha256"],
            "producerEnvironment": environment,
            "semanticInputsSha256": source["semanticInputsSha256"],
        }
        rows[producer_id] = {
            "artifacts": artifacts,
            "command": raw["command"],
            "completeInputsSha256": sha256(canonical(inputs)),
            "declaredEnvironment": declared_environment,
        }
    if sorted(claimed) != sorted(ARTIFACTS):
        fail("producer artifact ownership must cover every artifact exactly once")
    return rows


def declared_producer_environment(producer_id: str, profile: str) -> dict[str, str]:
    release = profile == "release-full"
    contracts = {
        "agent-reference-workflows": {
            "GENESIS_AGENT_GAUNTLET_PROFILE": profile,
            "GENESIS_AGENT_GAUNTLET_REGRESSION_SLACK_MS": "1500",
            "GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND": "1",
        },
        "agent-runtime-parity": {
            "GENESIS_AGENT_GAUNTLET_REGRESSION_SLACK_MS": "1500",
            "GENESIS_AGENT_PARITY_GAUNTLET_PROFILE": "prepush-standard",
            "GENESIS_AGENT_PARITY_REUSE_REPORTS": "0",
        },
        "assurance-profile-packs": {},
        "gpu-xr-productization": {
            "GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE": "1",
        },
        "host-bridge-fault-injection": {
            "GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS": "300000" if release else "120000",
            "GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT": "0",
            "GENESIS_HOST_BRIDGE_FAULT_RUNS": "6" if release else "1",
        },
        "runtime-backend-matrix": {
            "GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_INCREMENTAL": "0",
            "GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_PROFILE_DEV_DEBUG": "0",
            "GENESIS_RUNTIME_BACKEND_MATRIX_EPHEMERAL_TARGET_DIR": "$OUTPUT_ROOT/runtime-backend-target",
        },
        "webxr-browser-conformance": {},
    }
    return contracts[producer_id]


def build(root: pathlib.Path, output_root: pathlib.Path, profile: str) -> dict[str, Any]:
    if profile not in {"prepush-standard", "release-full"}:
        fail(f"closed evidence reuse is unsupported for profile: {profile}")
    now = dt.datetime.now(dt.timezone.utc)
    source = source_inventory(root)
    environment = producer_environment(profile)
    execution = execution_environment(profile)
    doc = {
        "artifacts": artifact_inventory(output_root),
        "consumers": consumer_inventory(profile),
        "contentIdentitySha256": "",
        "executionEnvironment": execution,
        "expiresAtUtc": (now + dt.timedelta(seconds=MAX_AGE_SECONDS)).isoformat(),
        "generatedAtUtc": now.isoformat(),
        "kind": KIND,
        "maxAgeSeconds": MAX_AGE_SECONDS,
        "ok": True,
        "producerEnvironment": environment,
        "producers": producer_inventory(source, environment, execution),
        "profile": profile,
        "source": source,
    }
    doc["contentIdentitySha256"] = object_identity(doc)
    return doc


def parse_time(raw: Any, label: str) -> dt.datetime:
    if not isinstance(raw, str):
        fail(f"{label} must be an ISO-8601 string")
    try:
        value = dt.datetime.fromisoformat(raw)
    except ValueError:
        fail(f"{label} is not valid ISO-8601")
    if value.tzinfo is None:
        fail(f"{label} must include a timezone")
    return value.astimezone(dt.timezone.utc)


def validate_manifest_shape(doc: Any) -> dict[str, Any]:
    manifest = exact_keys(
        doc,
        {
            "artifacts", "consumers", "contentIdentitySha256", "executionEnvironment",
            "expiresAtUtc", "generatedAtUtc", "kind", "maxAgeSeconds", "ok",
            "producerEnvironment", "producers", "profile", "source",
        },
        "manifest",
    )
    if manifest["kind"] != KIND or manifest["ok"] is not True:
        fail("manifest kind or ok state mismatch")
    if manifest["profile"] not in {"prepush-standard", "release-full"} or manifest["maxAgeSeconds"] != MAX_AGE_SECONDS:
        fail("manifest profile or freshness policy mismatch")
    if manifest["contentIdentitySha256"] != object_identity(manifest):
        fail("manifest content identity mismatch")
    exact_keys(manifest["source"], {"byteCount", "fileCount", "gitCommit", "semanticInputsSha256"}, "source")
    exact_keys(
        manifest["executionEnvironment"],
        {"architecture", "identitySha256", "operatingSystem", "operatingSystemRelease", "profile", "toolchains"},
        "executionEnvironment",
    )
    exact_keys(manifest["producerEnvironment"], {"profile", "variables"}, "producerEnvironment")
    if manifest["producerEnvironment"]["profile"] != manifest["profile"]:
        fail("producer environment profile does not match manifest profile")
    variables = manifest["producerEnvironment"]["variables"]
    if not isinstance(variables, list) or variables != sorted(variables, key=lambda row: row.get("name", "")):
        fail("producerEnvironment variables must be a sorted array")
    for index, row in enumerate(variables):
        exact_keys(row, {"name", "valueSha256"}, f"producerEnvironment variable {index}")
        if not isinstance(row["name"], str) or not isinstance(row["valueSha256"], str) or len(row["valueSha256"]) != 64:
            fail(f"producerEnvironment variable {index} is invalid")
    artifacts = manifest["artifacts"]
    if not isinstance(artifacts, dict) or set(artifacts) != set(ARTIFACTS):
        fail("manifest artifact inventory mismatch")
    for name, (kind, evidence_class) in ARTIFACTS.items():
        entry = exact_keys(artifacts[name], {"bytes", "evidenceClass", "kind", "sha256"}, f"artifact {name}")
        if entry["kind"] != kind or entry["evidenceClass"] != evidence_class:
            fail(f"artifact {name} class or kind mismatch")
    source = manifest["source"]
    if not all(isinstance(source[name], int) and source[name] >= 0 for name in ("byteCount", "fileCount")):
        fail("source counts must be non-negative integers")
    if not isinstance(source["gitCommit"], str) or len(source["gitCommit"]) not in {40, 64}:
        fail("source gitCommit must be a Git object identity")
    if not isinstance(source["semanticInputsSha256"], str) or len(source["semanticInputsSha256"]) != 64:
        fail("source semanticInputsSha256 must be a SHA-256 value")
    if manifest["consumers"] != consumer_inventory(manifest["profile"]):
        fail("manifest consumer contract mismatch")
    execution = manifest["executionEnvironment"]
    environment = manifest["producerEnvironment"]
    if manifest["producers"] != producer_inventory(source, environment, execution):
        fail("manifest producer contract mismatch")
    return manifest


def verify(
    root: pathlib.Path,
    manifest_path: pathlib.Path,
    consumer_id: str,
    script: str,
    supplied_paths: list[pathlib.Path],
) -> None:
    try:
        manifest = validate_manifest_shape(json.loads(manifest_path.read_text(encoding="utf-8")))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        fail(f"cannot read evidence manifest: {exc}")
    generated = parse_time(manifest["generatedAtUtc"], "generatedAtUtc")
    expires = parse_time(manifest["expiresAtUtc"], "expiresAtUtc")
    if expires - generated != dt.timedelta(seconds=manifest["maxAgeSeconds"]):
        fail("evidence manifest freshness window mismatch")
    now = dt.datetime.now(dt.timezone.utc)
    age = (now - generated).total_seconds()
    if age < -300 or age > manifest["maxAgeSeconds"] or now > expires:
        fail(f"evidence manifest is stale: ageSeconds={int(age)}")
    if manifest["source"] != source_inventory(root):
        fail("semantic source inputs changed after evidence production")
    if manifest["executionEnvironment"] != execution_environment(manifest["profile"]):
        fail("runtime/toolchain environment identity mismatch")
    consumer = manifest["consumers"].get(consumer_id)
    if not isinstance(consumer, dict) or consumer.get("script") != script:
        fail(f"invoked gate is not authorized for evidence reuse: {consumer_id}")
    expected_names = consumer["artifacts"]
    observed_names = sorted(path.name for path in supplied_paths)
    if observed_names != expected_names:
        fail(f"consumer artifact set mismatch: expected={expected_names!r} observed={observed_names!r}")
    manifest_root = manifest_path.resolve(strict=True).parent
    for path in supplied_paths:
        resolved = path.resolve(strict=True)
        if resolved.parent != manifest_root:
            fail(f"evidence artifact must be a direct manifest sibling: {path}")
        entry = manifest["artifacts"].get(path.name)
        if not isinstance(entry, dict):
            fail(f"manifest lacks evidence artifact: {path.name}")
        exact_keys(entry, {"bytes", "evidenceClass", "kind", "sha256"}, f"artifact {path.name}")
        payload = resolved.read_bytes()
        if len(payload) != entry["bytes"] or sha256(payload) != entry["sha256"]:
            fail(f"evidence artifact identity mismatch: {path.name}")
    print(
        "health-profile-evidence: verified "
        f"consumer={consumer_id} artifacts={len(supplied_paths)} identity={manifest['contentIdentitySha256']}"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    build_parser = subparsers.add_parser("build")
    build_parser.add_argument("--root", type=pathlib.Path, required=True)
    build_parser.add_argument("--output-root", type=pathlib.Path, required=True)
    build_parser.add_argument("--profile", required=True)
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--root", type=pathlib.Path, required=True)
    verify_parser.add_argument("--manifest", type=pathlib.Path, required=True)
    verify_parser.add_argument("--consumer", required=True)
    verify_parser.add_argument("--script", required=True)
    verify_parser.add_argument("artifacts", nargs="+", type=pathlib.Path)
    args = parser.parse_args()
    try:
        if args.command == "build":
            output_root = args.output_root.resolve()
            doc = build(args.root.resolve(), output_root, args.profile)
            path = output_root / "manifest.json"
            path.write_bytes(json.dumps(doc, indent=2, sort_keys=True).encode() + b"\n")
            print(
                "health-profile-evidence: ok "
                f"profile={args.profile} artifacts={len(doc['artifacts'])} manifest={path}"
            )
        else:
            verify(
                args.root.resolve(),
                args.manifest,
                args.consumer,
                args.script,
                args.artifacts,
            )
    except EvidenceError as exc:
        print(f"health-profile-evidence: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
