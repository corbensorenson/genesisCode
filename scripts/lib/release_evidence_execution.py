#!/usr/bin/env python3
"""Execute and aggregate the closed release-evidence DAG v0.2."""

from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any, Mapping, Optional, Sequence

import cargo_cache
import dependency_mirror
import health_profile_evidence as health_evidence
import release_evidence_dag as evidence_dag
import release_evidence_fanout as fanout
import release_full_measurement as measurement_support


WORKER_KIND = "genesis/release-evidence-worker-observation-v0.2"
AGGREGATE_KIND = "genesis/release-evidence-aggregate-v0.2"
VERSION = "0.2.0"
PROFILE_KIND = "genesis/upgrade-plan-health-profile-v0.1"
STATE_NAME = ".worker-state.json"
MANIFEST_NAME = "manifest.json"
ORCHESTRATION_DIR = "orchestration"
WORKER_FIELDS = {
    "artifacts", "budgets", "cleanup", "contentIdentitySha256", "dag",
    "executionEnvironment", "fanout", "generatedAtUtc", "github", "kind",
    "measurement", "node", "precondition", "source", "version",
}
PHASE_FIELDS = {
    "artifactAttributionBytes", "artifactPeakBytes", "commandIds",
    "commandIdsSha256", "elapsedMs", "exitCode", "logArtifacts", "name",
    "peakRssBytes", "profileReportArtifact", "profileReportSha256", "timedOut",
}
TARGETS = ["android", "edge", "ios", "service-runtime"]
SUDO_RELAY_ENV_NAMES = frozenset(
    {
        "AR_wasm32_wasip1",
        "CARGO",
        "CARGO_HOME",
        "CARGO_INCREMENTAL",
        "CARGO_NET_OFFLINE",
        "CARGO_TERM_COLOR",
        "CC_wasm32_wasip1",
        "CFLAGS_wasm32_wasip1",
        "CI",
        "GENESIS_AGENT_GPU_PROFILE",
        "GENESIS_CARGO_CACHE_ROOT",
        "GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT",
        "GENESIS_CHECK_HEALTH_OUTPUT_ROOT",
        "GENESIS_CHECK_HEALTH_RELEASE_FULL_HISTORY_INPUT",
        "GENESIS_GATE_TELEMETRY_DISABLE",
        "GENESIS_HEALTH_PROFILE",
        "GENESIS_HEALTH_PROFILE_GATE_CACHE",
        "GENESIS_HEALTH_WARM_CARGO_CACHE",
        "GENESIS_RELEASE_EVIDENCE_NODE_CLASS",
        "GENESIS_RELEASE_EVIDENCE_PHASE",
        "GENESIS_RELEASE_MEASUREMENT_RUN_CLASS",
        "GITHUB_ACTIONS",
        "GITHUB_RUN_ATTEMPT",
        "GITHUB_RUN_ID",
        "GITHUB_SHA",
        "GITHUB_WORKSPACE",
        "HOME",
        "LANG",
        "LC_ALL",
        "LOGNAME",
        "NPM_CONFIG_OFFLINE",
        "PATH",
        "RUSTC",
        "RUSTDOC",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "RUNNER_ARCH",
        "RUNNER_OS",
        "RUNNER_TEMP",
        "RUNNER_TOOL_CACHE",
        "SHELL",
        "TMPDIR",
        "TZ",
        "USER",
        "WASI_SDK_PATH",
        "WASI_SYSROOT",
        "WASMTIME_VERSION",
    }
)


class ExecutionError(ValueError):
    pass


def fail(message: str) -> None:
    raise ExecutionError(message)


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def identity(value: Mapping[str, Any]) -> str:
    clone = dict(value)
    clone.pop("contentIdentitySha256", None)
    return sha256_bytes(canonical(clone))


def is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(char in "0123456789abcdef" for char in value)
    )


def exact_keys(value: Any, expected: set[str], label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        fail(f"{label} fields mismatch: expected={sorted(expected)!r} observed={observed!r}")
    return value


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        fail(f"cannot read JSON {path}: {exc}")
    if not isinstance(value, dict):
        fail(f"JSON root must be an object: {path}")
    return value


def write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def initialize_worker_output(
    output: Path,
    evidence_class: str,
    cache_state: str,
    index: int,
) -> None:
    if output.exists() and any(output.iterdir()):
        fail("worker diagnostic output must start absent or empty")
    output.mkdir(parents=True, exist_ok=True)
    write_json(
        output / ORCHESTRATION_DIR / "started.json",
        {
            "cacheState": cache_state,
            "evidenceClass": evidence_class,
            "githubJob": os.environ.get("GITHUB_JOB", ""),
            "githubRunAttempt": os.environ.get("GITHUB_RUN_ATTEMPT", ""),
            "githubRunId": os.environ.get("GITHUB_RUN_ID", ""),
            "githubSha": os.environ.get("GITHUB_SHA", ""),
            "index": index,
            "kind": "genesis/release-evidence-worker-start-v0.2",
            "version": VERSION,
        },
    )


def require_initialized_output(
    output: Path,
    evidence_class: str,
    cache_state: str,
    index: int,
) -> None:
    start = exact_keys(
        load_json(output / ORCHESTRATION_DIR / "started.json"),
        {
            "cacheState", "evidenceClass", "githubJob", "githubRunAttempt",
            "githubRunId", "githubSha", "index", "kind", "version",
        },
        "worker start observation",
    )
    if (
        start["kind"] != "genesis/release-evidence-worker-start-v0.2"
        or start["version"] != VERSION
        or start["evidenceClass"] != evidence_class
        or start["cacheState"] != cache_state
        or start["index"] != index
        or start["githubJob"] != os.environ.get("GITHUB_JOB", "")
        or start["githubRunAttempt"] != os.environ.get("GITHUB_RUN_ATTEMPT", "")
        or start["githubRunId"] != os.environ.get("GITHUB_RUN_ID", "")
        or start["githubSha"] != os.environ.get("GITHUB_SHA", "")
    ):
        fail("worker start observation does not match the executing job")


def policy_context(root: Path) -> tuple[dict[str, Any], str]:
    policy = evidence_dag.load_policy(root)
    health_source = (root / "scripts/render_upgrade_plan_health_report.sh").read_text(
        encoding="utf-8"
    )
    evidence_dag.validate(policy, health_source)
    return policy, evidence_dag.sha256(policy)


def active_command_ids(
    policy: Mapping[str, Any],
    evidence_class: str,
    phase: str,
    gpu_profile: str,
) -> list[str]:
    rows = []
    for position, row in enumerate(policy["commands"]):
        if row["evidenceClass"] != evidence_class:
            continue
        if phase == "setup" and row["group"] != "setup":
            continue
        if phase == "commands" and row["group"] == "setup":
            continue
        if row.get("condition") == "agent-gpu-strict" and gpu_profile != "agent-gpu-strict":
            continue
        rows.append(({"setup": 0, "common": 1, "profile": 2}[row["group"]], position, row["id"]))
    if not rows:
        fail(f"release evidence class/phase selects no commands: {evidence_class}/{phase}")
    return [row[2] for row in sorted(rows)]


def command_identity(command_ids: Sequence[str]) -> str:
    return sha256_bytes(canonical(list(command_ids)))


def github_context(source: Mapping[str, Any], expected_job: str) -> dict[str, str]:
    values = {
        "job": os.environ.get("GITHUB_JOB", ""),
        "repository": os.environ.get("GITHUB_REPOSITORY", ""),
        "runAttempt": os.environ.get("GITHUB_RUN_ATTEMPT", ""),
        "runId": os.environ.get("GITHUB_RUN_ID", ""),
        "sha": os.environ.get("GITHUB_SHA", ""),
    }
    if values["job"] != expected_job:
        fail(f"release worker job identity mismatch: expected={expected_job!r}")
    if "/" not in values["repository"]:
        fail("release worker repository identity is invalid")
    for field in ("runAttempt", "runId"):
        if not values[field].isdigit() or values[field].startswith("0"):
            fail(f"release worker GitHub {field} is invalid")
    if values["sha"] != source.get("gitCommit"):
        fail("release worker source does not match GITHUB_SHA")
    return values


def expected_job(evidence_class: str, cache_state: str) -> str:
    if evidence_class == "cache-sensitive":
        if cache_state not in {"cold", "warm"}:
            fail("cache-sensitive worker state must be cold or warm")
        return f"release_evidence_{cache_state}_worker"
    if evidence_class == "invariant":
        return "release_evidence_invariant_worker"
    if evidence_class == "stress-performance":
        return "release_evidence_stress_worker"
    fail(f"unknown release evidence class: {evidence_class}")


def output_inventory(output: Path, ignored: Sequence[str] = ()) -> list[dict[str, Any]]:
    ignored_set = set(ignored)
    rows = []
    for path in sorted(output.rglob("*")):
        if not path.is_file() or path.name in ignored_set:
            continue
        relative = PurePosixPath(path.relative_to(output).as_posix())
        if relative.is_absolute() or ".." in relative.parts:
            fail(f"worker artifact path is unsafe: {relative}")
        rows.append(
            {
                "bytes": path.stat().st_size,
                "path": relative.as_posix(),
                "sha256": sha256_file(path),
            }
        )
    return rows


def verify_inventory(output: Path, rows: Any, ignored: Sequence[str] = ()) -> None:
    if not isinstance(rows, list):
        fail("worker artifact inventory must be an array")
    observed = output_inventory(output, ignored)
    if rows != observed:
        fail("worker artifact inventory is incomplete or does not match retained bytes")


def tree_identity(path: Path) -> str:
    rows = []
    if path.exists():
        for item in sorted(path.rglob("*")):
            if item.is_file() and not item.is_symlink():
                rows.append(
                    {
                        "bytes": item.stat().st_size,
                        "path": item.relative_to(path).as_posix(),
                        "sha256": sha256_file(item),
                    }
                )
    return sha256_bytes(canonical(rows))


def feature_set_identity(environ: Mapping[str, str]) -> str:
    names = [
        name
        for name in environ
        if name.startswith(("CARGO_PROFILE_", "GENESIS_AGENT_GPU_", "RUSTFLAGS"))
    ]
    return sha256_bytes(canonical({name: environ[name] for name in sorted(names)}))


def cache_key(root: Path, cache_root: Path) -> str:
    env = dict(os.environ)
    for name in measurement_support.CARGO_CACHE_ENV:
        env.pop(name, None)
    env["GENESIS_CARGO_CACHE_ROOT"] = str(cache_root)
    try:
        resolved = cargo_cache.resolve(root, "root-host", env)
    except cargo_cache.CachePolicyError as exc:
        fail(f"cannot resolve release worker cache key: {exc}")
    value = resolved["metadata"].get("cacheKeySha256")
    if not is_sha256(value):
        fail("release worker cache resolver emitted an invalid key")
    return value


def sudo_relay_environment(root: Path, environ: Mapping[str, str]) -> dict[str, str]:
    cache_policy = cargo_cache.load_policy(root)
    allowed = SUDO_RELAY_ENV_NAMES | set(cache_policy["buildEnvironment"])
    relay = {name: environ[name] for name in sorted(allowed) if name in environ}
    for name in (
        "CARGO_HOME",
        "CI",
        "GENESIS_AGENT_GPU_PROFILE",
        "GENESIS_CARGO_CACHE_ROOT",
        "GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT",
        "GENESIS_CHECK_HEALTH_OUTPUT_ROOT",
        "GENESIS_CHECK_HEALTH_RELEASE_FULL_HISTORY_INPUT",
        "GENESIS_HEALTH_PROFILE",
        "GENESIS_RELEASE_EVIDENCE_NODE_CLASS",
        "GENESIS_RELEASE_EVIDENCE_PHASE",
        "GENESIS_RELEASE_MEASUREMENT_RUN_CLASS",
        "HOME",
        "PATH",
    ):
        if not relay.get(name):
            fail(f"sudo-isolated release child is missing required environment: {name}")
    if shutil.which("rustc", path=relay["PATH"]) is None:
        fail("sudo-isolated release child PATH cannot resolve rustc")
    return relay


def guarded_process_command(
    root: Path,
    network_prefix: Sequence[str],
    argv: Sequence[str],
    environ: Mapping[str, str],
) -> tuple[list[str], dict[str, str]]:
    prefix = list(network_prefix)
    if prefix and Path(prefix[0]).name == "sudo":
        env_tool = shutil.which("env") or "/usr/bin/env"
        relay = sudo_relay_environment(root, environ)
        command = prefix + [
            env_tool,
            "-i",
            *(f"{name}={relay[name]}" for name in sorted(relay)),
            *argv,
        ]
        return command, {"LANG": "C", "PATH": "/usr/bin:/bin"}
    return prefix + list(argv), dict(environ)


def copy_phase_tree(source: Path, output: Path, prefix: str) -> None:
    for path in sorted(source.rglob("*")):
        if path.is_file():
            relative = path.relative_to(source)
            destination = output / prefix / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)


def validate_profile_report(
    path: Path,
    evidence_class: str,
    dag_sha: str,
    expected_ids: Sequence[str],
) -> dict[str, Any]:
    report = load_json(path)
    if (
        report.get("kind") != PROFILE_KIND
        or report.get("profile") != "release-full"
        or report.get("ok") is not True
        or report.get("release_evidence_partial") is not True
        or report.get("release_evidence_node_class") != evidence_class
        or report.get("release_evidence_dag_identity_sha256") != dag_sha
        or report.get("release_evidence_command_ids_sha256")
        != command_identity(expected_ids)
        or report.get("release_evidence_command_count") != len(expected_ids)
    ):
        fail(f"release profile report does not bind {evidence_class} command coverage")
    return report


def run_phase(
    *,
    root: Path,
    containment: Path,
    output: Path,
    evidence_class: str,
    cache_state: str,
    index: int,
    phase: str,
    cache_root: Path,
    timeout_ms: int,
    dag_sha: str,
    expected_ids: Sequence[str],
    input_root: Optional[Path] = None,
    fanout_token: str = "",
    export_root: Optional[Path] = None,
    network_prefix: Sequence[str] = (),
) -> dict[str, Any]:
    phase_key = phase.replace("/", "-")
    raw = containment / f"raw-{phase_key}"
    raw.mkdir(parents=True)
    stdout_path = containment / f"{phase_key}.stdout.log"
    stderr_path = containment / f"{phase_key}.stderr.log"
    history = containment / f"{phase_key}.history.jsonl"
    history.touch()
    env = measurement_support.measurement_environment(root, cache_root)
    env.update(
        {
            "CI": "true",
            "CARGO_NET_OFFLINE": "true",
            "GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT": str(containment),
            "GENESIS_CHECK_HEALTH_OUTPUT_ROOT": str(raw),
            "GENESIS_CHECK_HEALTH_RELEASE_FULL_HISTORY_INPUT": str(history),
            "GENESIS_GATE_TELEMETRY_DISABLE": "1",
            "GENESIS_HEALTH_PROFILE": "release-full",
            "GENESIS_HEALTH_PROFILE_GATE_CACHE": "0",
            "GENESIS_HEALTH_WARM_CARGO_CACHE": "0",
            "GENESIS_RELEASE_EVIDENCE_NODE_CLASS": evidence_class,
            "GENESIS_RELEASE_EVIDENCE_PHASE": phase if phase in {"setup", "commands"} else "all",
            "GENESIS_RELEASE_MEASUREMENT_RUN_CLASS": (
                cache_state if evidence_class == "cache-sensitive" else evidence_class
            ),
            "NPM_CONFIG_OFFLINE": "true",
        }
    )
    if input_root is not None:
        env["GENESIS_RELEASE_EVIDENCE_INPUT_ROOT"] = str(input_root)
        env["GENESIS_RELEASE_EVIDENCE_FANOUT_TOKEN"] = fanout_token
    if export_root is not None:
        env["GENESIS_RELEASE_EVIDENCE_EXPORT_ROOT"] = str(export_root)
    roots = {
        "containment": containment,
        "node-modules": root / "node_modules",
        "worker-output": output,
        "workspace-build": root / ".genesis/build",
        "workspace-target": root / "target",
    }
    argv = [
        "bash",
        "scripts/check_upgrade_plan_health.sh",
        "--profile",
        "release-full",
    ]
    argv, process_env = guarded_process_command(root, network_prefix, argv, env)
    started = time.monotonic_ns()
    timed_out = False
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        proc = subprocess.Popen(
            argv,
            cwd=root,
            env=process_env,
            stdout=stdout,
            stderr=stderr,
            start_new_session=True,
        )
        sampler = measurement_support.ResourceSampler(proc.pid, roots)
        sampler_thread = threading.Thread(target=sampler.run, daemon=True)
        sampler_thread.start()
        try:
            exit_code = proc.wait(timeout=max(1, timeout_ms) / 1000)
        except subprocess.TimeoutExpired:
            timed_out = True
            measurement_support.terminate_group(proc)
            exit_code = 124
        finally:
            sampler.stop.set()
            sampler_thread.join(timeout=30)
    elapsed_ms = max(1, (time.monotonic_ns() - started) // 1_000_000)
    if sampler_thread.is_alive():
        fail("release resource sampler did not terminate")
    if sampler.error is not None:
        fail(f"release resource sampler failed: {sampler.error}")
    retained = f"phases/{phase_key}"
    log_dir = output / retained
    log_dir.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(stdout_path, log_dir / "stdout.log")
    shutil.copyfile(stderr_path, log_dir / "stderr.log")
    copy_phase_tree(raw, output, f"{retained}/output")
    report_path = raw / "profile-report.json"
    report_artifact: Optional[str] = None
    report_sha: Optional[str] = None
    command_ids = list(expected_ids)
    if exit_code == 0:
        report = validate_profile_report(report_path, evidence_class, dag_sha, expected_ids)
        command_ids = list(expected_ids)
        report_artifact = f"{retained}/output/profile-report.json"
        report_sha = sha256_file(output / report_artifact)
        profile_elapsed = report.get("elapsed_ms")
        if not isinstance(profile_elapsed, int) or profile_elapsed <= 0:
            fail("release profile elapsed time is invalid")
    else:
        tail = measurement_support.diagnostic_tail(stderr_path, root)
        if tail:
            print("release-evidence: bounded child stderr tail:", file=sys.stderr)
            for line in tail.splitlines():
                print(f"release-evidence: | {line}", file=sys.stderr)
    return {
        "artifactAttributionBytes": sampler.peak_attribution,
        "artifactPeakBytes": sampler.peak_artifact_bytes,
        "commandIds": command_ids,
        "commandIdsSha256": command_identity(command_ids),
        "elapsedMs": elapsed_ms,
        "exitCode": exit_code,
        "logArtifacts": [f"{retained}/stdout.log", f"{retained}/stderr.log"],
        "name": phase,
        "peakRssBytes": sampler.peak_rss_bytes,
        "profileReportArtifact": report_artifact,
        "profileReportSha256": report_sha,
        "timedOut": timed_out,
    }


def isolation_record(
    github: Mapping[str, str],
    evidence_class: str,
    cache_state: str,
    index: int,
    nonce: str,
) -> dict[str, Any]:
    core = {
        "cacheState": cache_state,
        "evidenceClass": evidence_class,
        "githubJob": github["job"],
        "githubRunAttempt": github["runAttempt"],
        "githubRunId": github["runId"],
        "index": index,
        "method": (
            "exclusive-owned-ephemeral-root"
            if evidence_class == "stress-performance"
            else "owned-ephemeral-root"
        ),
        "nonceSha256": sha256_bytes(nonce.encode()),
    }
    return {**core, "identitySha256": sha256_bytes(canonical(core))}


def cleanup_record(containment: Path, started_empty: bool) -> dict[str, Any]:
    recovered = measurement_support.allocated_bytes(containment)
    shutil.rmtree(containment, ignore_errors=False)
    remaining = measurement_support.allocated_bytes(containment)
    return {
        "method": "owned-ephemeral-root-removal",
        "recoveredBytes": recovered,
        "remainingBytes": remaining,
        "rootRemoved": not containment.exists() and remaining == 0,
        "rootStartedEmpty": started_empty,
    }


def phase_success(phases: Sequence[Mapping[str, Any]]) -> bool:
    return all(row["exitCode"] == 0 and row["timedOut"] is False for row in phases)


def finalize_worker(
    *,
    root: Path,
    output: Path,
    policy: Mapping[str, Any],
    dag_sha: str,
    source: Mapping[str, Any],
    execution: Mapping[str, Any],
    github: Mapping[str, str],
    evidence_class: str,
    cache_state: str,
    index: int,
    isolation: Mapping[str, Any],
    phases: list[dict[str, Any]],
    precondition: Optional[dict[str, Any]],
    fanout_record: Optional[dict[str, Any]],
    cleanup: Mapping[str, Any],
) -> dict[str, Any]:
    gpu_profile = os.environ.get("GENESIS_AGENT_GPU_PROFILE", "")
    expected_ids = active_command_ids(policy, evidence_class, "all", gpu_profile)
    observed_ids = [item for phase in phases for item in phase["commandIds"]]
    elapsed = sum(int(phase["elapsedMs"]) for phase in phases)
    peak_artifacts = max(int(phase["artifactPeakBytes"]) for phase in phases)
    peak_rss = max(int(phase["peakRssBytes"]) for phase in phases)
    report = {
        "artifacts": output_inventory(output, (MANIFEST_NAME, STATE_NAME)),
        "budgets": {
            "artifactBytes": policy["budgets"]["artifactBytesPerWorker"],
            "diagnosticTailBytes": policy["budgets"]["diagnosticTailBytes"],
            "diagnosticTailLines": policy["budgets"]["diagnosticTailLines"],
            "measuredWallMs": policy["budgets"]["measuredWorkerWallMs"],
        },
        "cleanup": cleanup,
        "contentIdentitySha256": "",
        "dag": {
            "identitySha256": dag_sha,
            "path": "policies/release_evidence_dag_v0.2.json",
        },
        "executionEnvironment": execution,
        "fanout": fanout_record,
        "generatedAtUtc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "github": github,
        "kind": WORKER_KIND,
        "measurement": {
            "artifactPeakBytes": peak_artifacts,
            "commandCoverageExact": observed_ids == expected_ids,
            "elapsedMs": elapsed,
            "exitCode": 0 if phase_success(phases) else next(
                phase["exitCode"] for phase in phases if phase["exitCode"] != 0
            ),
            "peakRssBytes": peak_rss,
            "phases": phases,
            "timedOut": any(phase["timedOut"] for phase in phases),
        },
        "node": {
            "cacheState": cache_state,
            "commandIds": expected_ids,
            "commandIdsSha256": command_identity(expected_ids),
            "evidenceClass": evidence_class,
            "index": index,
            "isolation": isolation,
        },
        "precondition": precondition,
        "source": source,
        "version": VERSION,
    }
    report["contentIdentitySha256"] = identity(report)
    write_json(output / MANIFEST_NAME, report)
    return report


def create_containment(output: Path) -> tuple[Path, str]:
    base = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir())).resolve()
    base.mkdir(parents=True, exist_ok=True)
    nonce = os.urandom(32).hex()
    containment = Path(tempfile.mkdtemp(prefix="genesis-release-evidence-", dir=base))
    if any(containment.iterdir()):
        fail("owned release worker root did not start empty")
    (containment / "owner.json").write_text(
        json.dumps({"nonceSha256": sha256_bytes(nonce.encode())}) + "\n",
        encoding="utf-8",
    )
    output.mkdir(parents=True, exist_ok=True)
    return containment, nonce


def prepare_cache(root: Path, output: Path, cache_state: str, index: int) -> int:
    require_initialized_output(output, "cache-sensitive", cache_state, index)
    policy, dag_sha = policy_context(root)
    source = health_evidence.source_inventory(root)
    execution = health_evidence.execution_environment("release-full")
    github = github_context(source, expected_job("cache-sensitive", cache_state))
    gpu_profile = os.environ.get("GENESIS_AGENT_GPU_PROFILE", "")
    if gpu_profile not in {"agent-gpu-strict", "agent-gpu-fallback"}:
        fail("release worker requires an explicit agent GPU profile")
    containment, nonce = create_containment(output)
    cache_root = containment / "cache"
    isolation = isolation_record(github, "cache-sensitive", cache_state, index, nonce)
    precondition = None
    phases: list[dict[str, Any]] = []
    if cache_state == "warm":
        root_started_empty = not cache_root.exists()
        backend, prefix = dependency_mirror.network_guard_prefix(allow_loopback=True)
        dependency_mirror.prove_network_denial(prefix, require_loopback=True)
        expected_all = active_command_ids(policy, "cache-sensitive", "all", gpu_profile)
        before_source = health_evidence.source_inventory(root)
        before_execution = health_evidence.execution_environment("release-full")
        before_features = feature_set_identity(os.environ)
        before_key = cache_key(root, cache_root)
        unmeasured = run_phase(
            root=root,
            containment=containment,
            output=output,
            evidence_class="cache-sensitive",
            cache_state="warm",
            index=index,
            phase="precondition",
            cache_root=cache_root,
            timeout_ms=policy["budgets"]["measuredWorkerWallMs"],
            dag_sha=dag_sha,
            expected_ids=expected_all,
            network_prefix=prefix,
        )
        inventory_after = tree_identity(cache_root)
        after_source = health_evidence.source_inventory(root)
        after_execution = health_evidence.execution_environment("release-full")
        after_features = feature_set_identity(os.environ)
        after_key = cache_key(root, cache_root)
        precondition = {
            "artifactInventoryAtMeasuredStartSha256": "",
            "artifactInventoryAtPreconditionEndSha256": inventory_after,
            "artifactInventoryMatched": False,
            "cacheKeyMatched": before_key == after_key,
            "cacheKeySha256": after_key,
            "commandIds": expected_all,
            "commandIdsSha256": command_identity(expected_all),
            "executionEnvironmentMatched": before_execution == after_execution,
            "featureSetMatched": before_features == after_features,
            "featureSetSha256": after_features,
            "measured": False,
            "network": {
                "backend": backend,
                "mode": "deny",
                "proofExecuted": True,
            },
            "phase": unmeasured,
            "rootStartedEmpty": root_started_empty,
            "sourceMatched": before_source == after_source,
            "toolchainMatched": before_execution == after_execution,
        }
        if not phase_success([unmeasured]):
            cleanup = cleanup_record(containment, True)
            finalize_worker(
                root=root, output=output, policy=policy, dag_sha=dag_sha,
                source=source, execution=execution, github=github,
                evidence_class="cache-sensitive", cache_state=cache_state,
                index=index, isolation=isolation, phases=[unmeasured],
                precondition=precondition, fanout_record=None, cleanup=cleanup,
            )
            return unmeasured["exitCode"]
    setup_ids = active_command_ids(policy, "cache-sensitive", "setup", gpu_profile)
    if precondition is not None:
        measured_inventory = tree_identity(cache_root)
        measured_source = health_evidence.source_inventory(root)
        measured_execution = health_evidence.execution_environment("release-full")
        measured_features = feature_set_identity(os.environ)
        measured_key = cache_key(root, cache_root)
        precondition.update(
            {
                "artifactInventoryAtMeasuredStartSha256": measured_inventory,
                "artifactInventoryMatched": inventory_after == measured_inventory,
                "cacheKeyMatched": before_key == after_key == measured_key,
                "cacheKeySha256": measured_key,
                "executionEnvironmentMatched": (
                    before_execution == after_execution == measured_execution
                ),
                "featureSetMatched": before_features == after_features == measured_features,
                "featureSetSha256": measured_features,
                "sourceMatched": before_source == after_source == measured_source,
                "toolchainMatched": before_execution == after_execution == measured_execution,
            }
        )
    export_root = output / "fanout-bundle" if cache_state == "cold" and index == 1 else None
    setup = run_phase(
        root=root,
        containment=containment,
        output=output,
        evidence_class="cache-sensitive",
        cache_state=cache_state,
        index=index,
        phase="setup",
        cache_root=cache_root,
        timeout_ms=policy["budgets"]["measuredWorkerWallMs"],
        dag_sha=dag_sha,
        expected_ids=setup_ids,
        export_root=export_root,
    )
    phases.append(setup)
    producer = None
    if export_root is not None and setup["exitCode"] == 0:
        manifest = health_evidence.validate_manifest_shape(
            json.loads((export_root / "manifest.json").read_text(encoding="utf-8"))
        )
        producer = {
            "artifactName": fanout.artifact_name(
                {
                    "runId": github["runId"],
                    "runAttempt": github["runAttempt"],
                    "sha": github["sha"],
                }
            ),
            "bundleIdentitySha256": manifest["contentIdentitySha256"],
            "dagIdentitySha256": dag_sha,
            "evidenceClass": "cache-sensitive",
            "index": 1,
            "manifestSha256": sha256_file(export_root / "manifest.json"),
            "role": "producer",
        }
    state = {
        "cacheRoot": str(cache_root),
        "containment": str(containment),
        "dagIdentitySha256": dag_sha,
        "executionEnvironment": execution,
        "fanout": producer,
        "github": github,
        "index": index,
        "isolation": isolation,
        "nonce": nonce,
        "phases": phases,
        "precondition": precondition,
        "source": source,
        "state": cache_state,
    }
    write_json(output / STATE_NAME, state)
    return setup["exitCode"]


def finish_cache(root: Path, output: Path, cache_state: str, index: int) -> int:
    state = load_json(output / STATE_NAME)
    if state.get("state") != cache_state or state.get("index") != index:
        fail("cache worker continuation identity mismatch")
    policy, dag_sha = policy_context(root)
    if state.get("dagIdentitySha256") != dag_sha:
        fail("cache worker DAG changed between setup and measured commands")
    containment = Path(state["containment"]).resolve(strict=True)
    cache_root = Path(state["cacheRoot"]).resolve(strict=True)
    gpu_profile = os.environ.get("GENESIS_AGENT_GPU_PROFILE", "")
    phases = list(state["phases"])
    remaining = policy["budgets"]["measuredWorkerWallMs"] - sum(
        int(phase["elapsedMs"]) for phase in phases
    )
    command_ids = active_command_ids(policy, "cache-sensitive", "commands", gpu_profile)
    commands = run_phase(
        root=root,
        containment=containment,
        output=output,
        evidence_class="cache-sensitive",
        cache_state=cache_state,
        index=index,
        phase="commands",
        cache_root=cache_root,
        timeout_ms=remaining,
        dag_sha=dag_sha,
        expected_ids=command_ids,
    )
    phases.append(commands)
    fanout_bundle = output / "fanout-bundle"
    if fanout_bundle.exists():
        shutil.rmtree(fanout_bundle)
    producer = state["fanout"]
    if producer is not None:
        digest = os.environ.get("GENESIS_RELEASE_FANOUT_DIGEST", "")
        if not is_sha256(digest):
            fail("cold-1 fanout upload lacks the service-issued artifact digest")
        producer = {**producer, "digestSha256": digest}
    (output / STATE_NAME).unlink(missing_ok=True)
    cleanup = cleanup_record(containment, True)
    report = finalize_worker(
        root=root,
        output=output,
        policy=policy,
        dag_sha=dag_sha,
        source=state["source"],
        execution=state["executionEnvironment"],
        github=state["github"],
        evidence_class="cache-sensitive",
        cache_state=cache_state,
        index=index,
        isolation=state["isolation"],
        phases=phases,
        precondition=state["precondition"],
        fanout_record=producer,
        cleanup=cleanup,
    )
    return int(report["measurement"]["exitCode"])


def run_node(
    root: Path,
    output: Path,
    evidence_class: str,
    index: int,
    fanout_root: Path,
    fanout_token: str,
) -> int:
    if evidence_class not in {"invariant", "stress-performance"}:
        fail("run-node only accepts invariant or stress-performance")
    require_initialized_output(output, evidence_class, "not-measured", index)
    policy, dag_sha = policy_context(root)
    source = health_evidence.source_inventory(root)
    execution = health_evidence.execution_environment("release-full")
    github = github_context(source, expected_job(evidence_class, "not-measured"))
    gpu_profile = os.environ.get("GENESIS_AGENT_GPU_PROFILE", "")
    if gpu_profile not in {"agent-gpu-strict", "agent-gpu-fallback"}:
        fail("release worker requires an explicit agent GPU profile")
    auth_path = fanout_root.parent / fanout.AUTH_NAME
    auth = fanout.validate_auth(root, fanout_root, auth_path, fanout_token)
    containment, nonce = create_containment(output)
    cache_root = containment / "cache"
    isolation = isolation_record(github, evidence_class, "not-measured", index, nonce)
    expected_ids = active_command_ids(policy, evidence_class, "all", gpu_profile)
    phase = run_phase(
        root=root,
        containment=containment,
        output=output,
        evidence_class=evidence_class,
        cache_state="not-measured",
        index=index,
        phase="all",
        cache_root=cache_root,
        timeout_ms=policy["budgets"]["measuredWorkerWallMs"],
        dag_sha=dag_sha,
        expected_ids=expected_ids,
        input_root=fanout_root,
        fanout_token=fanout_token,
    )
    cleanup = cleanup_record(containment, True)
    fanout_record = {
        "artifactName": auth["artifact"]["name"],
        "bundleIdentitySha256": auth["producer"]["bundleIdentitySha256"],
        "dagIdentitySha256": auth["producer"]["dagIdentitySha256"],
        "digestSha256": auth["artifact"]["digestSha256"],
        "evidenceClass": auth["producer"]["evidenceClass"],
        "index": auth["producer"]["index"],
        "manifestSha256": auth["producer"]["manifestSha256"],
        "role": "consumer",
    }
    report = finalize_worker(
        root=root,
        output=output,
        policy=policy,
        dag_sha=dag_sha,
        source=source,
        execution=execution,
        github=github,
        evidence_class=evidence_class,
        cache_state="not-measured",
        index=index,
        isolation=isolation,
        phases=[phase],
        precondition=None,
        fanout_record=fanout_record,
        cleanup=cleanup,
    )
    return int(report["measurement"]["exitCode"])


def validate_phase(
    root: Path,
    output: Path,
    row: Any,
    evidence_class: str,
    dag_sha: str,
    expected_name: str,
    expected_ids: Sequence[str],
) -> Mapping[str, Any]:
    phase = exact_keys(row, PHASE_FIELDS, f"{evidence_class} phase")
    if (
        phase["name"] != expected_name
        or phase["commandIds"] != list(expected_ids)
        or phase["commandIdsSha256"] != command_identity(expected_ids)
        or phase["exitCode"] != 0
        or phase["timedOut"] is not False
        or not isinstance(phase["elapsedMs"], int)
        or phase["elapsedMs"] <= 0
        or not isinstance(phase["peakRssBytes"], int)
        or phase["peakRssBytes"] <= 0
        or not isinstance(phase["artifactPeakBytes"], int)
        or phase["artifactPeakBytes"] < 0
    ):
        fail(f"{evidence_class} phase observation is unsuccessful or relabeled")
    logs = phase["logArtifacts"]
    if not isinstance(logs, list) or len(logs) != 2 or len(set(logs)) != 2:
        fail("release phase must bind distinct stdout/stderr logs")
    for relative in logs:
        if not (output / relative).is_file():
            fail(f"release phase log is missing: {relative}")
    report_relative = phase["profileReportArtifact"]
    if not isinstance(report_relative, str) or not is_sha256(phase["profileReportSha256"]):
        fail("release phase profile report binding is absent")
    report_path = output / report_relative
    if sha256_file(report_path) != phase["profileReportSha256"]:
        fail("release phase profile report digest mismatch")
    validate_profile_report(report_path, evidence_class, dag_sha, expected_ids)
    return phase


def validate_worker(root: Path, output: Path, policy: Mapping[str, Any], dag_sha: str) -> dict[str, Any]:
    report = exact_keys(load_json(output / MANIFEST_NAME), WORKER_FIELDS, "worker observation")
    if (
        report["kind"] != WORKER_KIND
        or report["version"] != VERSION
        or report["contentIdentitySha256"] != identity(report)
    ):
        fail("worker observation identity mismatch")
    generated = dt.datetime.fromisoformat(report["generatedAtUtc"])
    now = dt.datetime.now(dt.timezone.utc)
    if generated.tzinfo is None or generated > now + dt.timedelta(minutes=5) or now - generated > dt.timedelta(hours=6):
        fail("worker observation is stale")
    if report["source"] != health_evidence.source_inventory(root):
        fail("worker source identity drift")
    try:
        fanout.validated_execution_environment("worker", report["executionEnvironment"])
    except fanout.FanoutError as exc:
        fail(str(exc))
    if report["dag"] != {
        "identitySha256": dag_sha,
        "path": "policies/release_evidence_dag_v0.2.json",
    }:
        fail("worker DAG identity drift")
    verify_inventory(output, report["artifacts"], (MANIFEST_NAME, STATE_NAME))
    budgets = report["budgets"]
    if budgets != {
        "artifactBytes": policy["budgets"]["artifactBytesPerWorker"],
        "diagnosticTailBytes": policy["budgets"]["diagnosticTailBytes"],
        "diagnosticTailLines": policy["budgets"]["diagnosticTailLines"],
        "measuredWallMs": policy["budgets"]["measuredWorkerWallMs"],
    }:
        fail("worker budget contract mismatch")
    node = exact_keys(
        report["node"],
        {
            "cacheState", "commandIds", "commandIdsSha256", "evidenceClass",
            "index", "isolation",
        },
        "worker node",
    )
    evidence_class = node["evidenceClass"]
    cache_state = node["cacheState"]
    index = node["index"]
    gpu_profile = os.environ.get("GENESIS_AGENT_GPU_PROFILE", "agent-gpu-fallback")
    expected_ids = active_command_ids(policy, evidence_class, "all", gpu_profile)
    if (
        node["commandIds"] != expected_ids
        or node["commandIdsSha256"] != command_identity(expected_ids)
        or not isinstance(index, int)
        or isinstance(index, bool)
        or index < 1
    ):
        fail("worker node command coverage or index mismatch")
    github = exact_keys(
        report["github"],
        {"job", "repository", "runAttempt", "runId", "sha"},
        "worker GitHub provenance",
    )
    if github["job"] != expected_job(evidence_class, cache_state) or github["sha"] != report["source"]["gitCommit"]:
        fail("worker GitHub job or source provenance mismatch")
    isolation = exact_keys(
        node["isolation"],
        {
            "cacheState", "evidenceClass", "githubJob", "githubRunAttempt",
            "githubRunId", "identitySha256", "index", "method", "nonceSha256",
        },
        "worker isolation",
    )
    core = dict(isolation)
    observed_isolation_identity = core.pop("identitySha256")
    if (
        isolation["cacheState"] != cache_state
        or isolation["evidenceClass"] != evidence_class
        or isolation["index"] != index
        or isolation["githubJob"] != github["job"]
        or isolation["githubRunAttempt"] != github["runAttempt"]
        or isolation["githubRunId"] != github["runId"]
        or not is_sha256(isolation["nonceSha256"])
        or observed_isolation_identity != sha256_bytes(canonical(core))
    ):
        fail("worker isolation identity mismatch")
    expected_method = (
        "exclusive-owned-ephemeral-root"
        if evidence_class == "stress-performance"
        else "owned-ephemeral-root"
    )
    if isolation["method"] != expected_method:
        fail("worker isolation method mismatch")
    cleanup = exact_keys(
        report["cleanup"],
        {
            "method", "recoveredBytes", "remainingBytes", "rootRemoved",
            "rootStartedEmpty",
        },
        "worker cleanup",
    )
    if (
        cleanup["method"] != "owned-ephemeral-root-removal"
        or cleanup["rootStartedEmpty"] is not True
        or cleanup["rootRemoved"] is not True
        or cleanup["remainingBytes"] != 0
        or not isinstance(cleanup["recoveredBytes"], int)
        or cleanup["recoveredBytes"] <= 0
    ):
        fail("worker cleanup is incomplete")
    measurement = exact_keys(
        report["measurement"],
        {
            "artifactPeakBytes", "commandCoverageExact", "elapsedMs", "exitCode",
            "peakRssBytes", "phases", "timedOut",
        },
        "worker measurement",
    )
    if (
        measurement["exitCode"] != 0
        or measurement["timedOut"] is not False
        or measurement["commandCoverageExact"] is not True
        or measurement["elapsedMs"] > policy["budgets"]["measuredWorkerWallMs"]
        or measurement["artifactPeakBytes"] > policy["budgets"]["artifactBytesPerWorker"]
    ):
        fail("worker measurement failed a resource or command-coverage bound")
    phases = measurement["phases"]
    if evidence_class == "cache-sensitive":
        if not isinstance(phases, list) or len(phases) != 2:
            fail("cache-sensitive worker requires setup and commands phases")
        setup_ids = active_command_ids(policy, evidence_class, "setup", gpu_profile)
        command_ids = active_command_ids(policy, evidence_class, "commands", gpu_profile)
        validate_phase(root, output, phases[0], evidence_class, dag_sha, "setup", setup_ids)
        validate_phase(root, output, phases[1], evidence_class, dag_sha, "commands", command_ids)
        if cache_state == "warm":
            pre = exact_keys(
                report["precondition"],
                {
                    "artifactInventoryAtMeasuredStartSha256",
                    "artifactInventoryAtPreconditionEndSha256",
                    "artifactInventoryMatched", "cacheKeyMatched", "cacheKeySha256",
                    "commandIds", "commandIdsSha256", "executionEnvironmentMatched",
                    "featureSetMatched", "featureSetSha256", "measured", "network",
                    "phase", "rootStartedEmpty", "sourceMatched", "toolchainMatched",
                },
                "warm precondition",
            )
            required_true = [
                "artifactInventoryMatched", "cacheKeyMatched",
                "executionEnvironmentMatched", "featureSetMatched", "rootStartedEmpty",
                "sourceMatched", "toolchainMatched",
            ]
            if (
                pre["measured"] is not False
                or any(pre[name] is not True for name in required_true)
                or pre["commandIds"] != expected_ids
                or pre["commandIdsSha256"] != command_identity(expected_ids)
                or not is_sha256(pre["cacheKeySha256"])
                or not is_sha256(pre["featureSetSha256"])
                or pre["artifactInventoryAtMeasuredStartSha256"]
                != pre["artifactInventoryAtPreconditionEndSha256"]
            ):
                fail("warm precondition proof is incomplete")
            network = exact_keys(
                pre["network"], {"backend", "mode", "proofExecuted"}, "warm network proof"
            )
            if network["mode"] != "deny" or network["proofExecuted"] is not True:
                fail("warm precondition did not prove hard network denial")
            validate_phase(root, output, pre["phase"], evidence_class, dag_sha, "precondition", expected_ids)
        elif cache_state == "cold":
            if report["precondition"] is not None:
                fail("cold worker carries a forged warm precondition")
        else:
            fail("cache-sensitive worker state is invalid")
    else:
        if cache_state != "not-measured" or report["precondition"] is not None:
            fail("invariant/stress worker cache state is relabeled")
        if not isinstance(phases, list) or len(phases) != 1:
            fail("invariant/stress worker requires one measured phase")
        validate_phase(root, output, phases[0], evidence_class, dag_sha, "all", expected_ids)
    return dict(report)


def validate_worker_environment_cohort(workers: Sequence[Mapping[str, Any]]) -> None:
    if not workers:
        fail("release worker environment cohort is empty")
    expected = workers[0]["executionEnvironment"]
    for worker in workers[1:]:
        observed = worker["executionEnvironment"]
        if observed == expected:
            continue
        mismatches = fanout.execution_environment_mismatches(expected, observed)
        if not mismatches:
            mismatches = [
                "operatingSystemRelease "
                f"expected={expected['operatingSystemRelease']!r} "
                f"observed={observed['operatingSystemRelease']!r}"
            ]
        fail("worker execution environment identity drift: " + "; ".join(mismatches))


def percentile95(values: Sequence[int]) -> int:
    return sorted(values)[max(0, math.ceil(len(values) * 0.95) - 1)]


def expected_node_keys() -> list[tuple[str, str, int]]:
    return [
        *[("cache-sensitive", "cold", index) for index in range(1, 4)],
        *[("cache-sensitive", "warm", index) for index in range(1, 4)],
        ("invariant", "not-measured", 1),
        *[("stress-performance", "not-measured", index) for index in range(1, 4)],
    ]


def validate_node_topology(workers: Sequence[Mapping[str, Any]]) -> None:
    observed = [
        (row["node"]["evidenceClass"], row["node"]["cacheState"], row["node"]["index"])
        for row in workers
    ]
    if sorted(observed) != sorted(expected_node_keys()) or len(observed) != len(set(observed)):
        fail("release aggregate has a missing or duplicate execution node")


def validate_isolation_set(workers: Sequence[Mapping[str, Any]]) -> None:
    isolation_ids = [row["node"]["isolation"]["identitySha256"] for row in workers]
    nonce_ids = [row["node"]["isolation"]["nonceSha256"] for row in workers]
    if len(set(isolation_ids)) != len(workers) or len(set(nonce_ids)) != len(workers):
        fail("release workers reused an owned or exclusive root identity")


def validate_fanout_custody(
    workers: Sequence[Mapping[str, Any]],
    dag_sha: str,
) -> Mapping[str, Any]:
    cold_one = next(
        row
        for row in workers
        if (row["node"]["evidenceClass"], row["node"]["cacheState"], row["node"]["index"])
        == ("cache-sensitive", "cold", 1)
    )
    producer = exact_keys(
        cold_one["fanout"],
        {
            "artifactName", "bundleIdentitySha256", "dagIdentitySha256",
            "digestSha256", "evidenceClass", "index", "manifestSha256", "role",
        },
        "cold-1 fanout producer",
    )
    if (
        producer["role"] != "producer"
        or producer["evidenceClass"] != "cache-sensitive"
        or producer["index"] != 1
        or producer["dagIdentitySha256"] != dag_sha
        or not is_sha256(producer["digestSha256"])
    ):
        fail("cold-1 fanout producer binding is invalid")
    consumer_digests = set()
    for row in workers:
        node = row["node"]
        if node["evidenceClass"] == "cache-sensitive":
            if node["cacheState"] == "cold" and node["index"] == 1:
                continue
            if row["fanout"] is not None:
                fail("non-producer cache worker carries cross-class fanout")
            continue
        consumer = exact_keys(
            row["fanout"],
            {
                "artifactName", "bundleIdentitySha256", "dagIdentitySha256",
                "digestSha256", "evidenceClass", "index", "manifestSha256", "role",
            },
            "fanout consumer",
        )
        for name in (
            "artifactName", "bundleIdentitySha256", "dagIdentitySha256",
            "evidenceClass", "index", "manifestSha256",
        ):
            if consumer[name] != producer[name]:
                fail("fanout consumer does not bind the cold-1 producer")
        if consumer["role"] != "consumer" or not is_sha256(consumer["digestSha256"]):
            fail("fanout consumer authentication is invalid")
        if consumer["digestSha256"] != producer["digestSha256"]:
            fail("fanout consumer digest does not match the producer upload receipt")
        consumer_digests.add(consumer["digestSha256"])
    if len(consumer_digests) != 1:
        fail("fanout consumers disagree on the authenticated artifact digest")
    return producer


def aggregate(
    root: Path,
    output: Path,
    worker_outputs: Sequence[Path],
    target_reports: Sequence[Path],
) -> dict[str, Any]:
    started = time.monotonic_ns()
    if output.exists() and any(output.iterdir()):
        fail("aggregate output must start absent or empty")
    output.mkdir(parents=True, exist_ok=True)
    policy, dag_sha = policy_context(root)
    workers = [validate_worker(root, path.resolve(strict=True), policy, dag_sha) for path in worker_outputs]
    validate_worker_environment_cohort(workers)
    expected_nodes = expected_node_keys()
    validate_node_topology(workers)
    workers.sort(
        key=lambda row: expected_nodes.index(
            (row["node"]["evidenceClass"], row["node"]["cacheState"], row["node"]["index"])
        )
    )
    github_cores = {
        (
            row["github"]["repository"], row["github"]["runId"],
            row["github"]["runAttempt"], row["github"]["sha"],
        )
        for row in workers
    }
    if len(github_cores) != 1:
        fail("release workers do not share one workflow run, attempt, and revision")
    validate_isolation_set(workers)
    producer = validate_fanout_custody(workers, dag_sha)
    targets = measurement_support.validate_target_reports(root, target_reports)
    workflow = next(iter(github_cores))
    if any(
        target["githubRunId"] != workflow[1]
        or target["githubRunAttempt"] != workflow[2]
        or target["githubSha"] != workflow[3]
        for target in targets
    ):
        fail("target dispositions do not share the worker workflow provenance")
    if [row["target"] for row in targets] != TARGETS:
        fail("target disposition coverage is incomplete")
    summaries = []
    for worker, worker_output in zip(workers, [
        next(
            path
            for path in worker_outputs
            if load_json(path / MANIFEST_NAME)["contentIdentitySha256"]
            == worker["contentIdentitySha256"]
        )
        for worker in workers
    ]):
        manifest_path = worker_output / MANIFEST_NAME
        summaries.append(
            {
                "cacheState": worker["node"]["cacheState"],
                "contentIdentitySha256": worker["contentIdentitySha256"],
                "evidenceClass": worker["node"]["evidenceClass"],
                "index": worker["node"]["index"],
                "manifestSha256": sha256_file(manifest_path),
            }
        )
    cache_workers = [row for row in workers if row["node"]["evidenceClass"] == "cache-sensitive"]
    cold = [row for row in cache_workers if row["node"]["cacheState"] == "cold"]
    warm = [row for row in cache_workers if row["node"]["cacheState"] == "warm"]
    report = {
        "contentIdentitySha256": "",
        "dag": {
            "identitySha256": dag_sha,
            "path": "policies/release_evidence_dag_v0.2.json",
        },
        "derivedAtUtc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "derivation": {
            "readOnly": True,
            "rejectedConditions": list(policy["aggregate"]["reject"]),
            "wallMs": max(1, (time.monotonic_ns() - started) // 1_000_000),
        },
        "github": {
            "repository": workflow[0],
            "runAttempt": workflow[2],
            "runId": workflow[1],
            "sha": workflow[3],
        },
        "fanout": {
            "artifactName": producer["artifactName"],
            "bundleIdentitySha256": producer["bundleIdentitySha256"],
            "digestSha256": producer["digestSha256"],
            "manifestSha256": producer["manifestSha256"],
            "producer": {"cacheState": "cold", "evidenceClass": "cache-sensitive", "index": 1},
        },
        "history": {
            "coldP95ArtifactBytes": percentile95(
                [row["measurement"]["artifactPeakBytes"] for row in cold]
            ),
            "coldP95PeakRssBytes": percentile95(
                [row["measurement"]["peakRssBytes"] for row in cold]
            ),
            "coldP95WallMs": percentile95(
                [row["measurement"]["elapsedMs"] for row in cold]
            ),
            "samplesPerCacheState": 3,
            "warmP95ArtifactBytes": percentile95(
                [row["measurement"]["artifactPeakBytes"] for row in warm]
            ),
            "warmP95PeakRssBytes": percentile95(
                [row["measurement"]["peakRssBytes"] for row in warm]
            ),
            "warmP95WallMs": percentile95(
                [row["measurement"]["elapsedMs"] for row in warm]
            ),
        },
        "kind": AGGREGATE_KIND,
        "productReleaseQualified": False,
        "profileOperational": True,
        "readinessStatus": "unsupported-product",
        "source": health_evidence.source_inventory(root),
        "status": "pass",
        "targetDispositions": targets,
        "version": VERSION,
        "workers": summaries,
    }
    if report["derivation"]["wallMs"] > policy["budgets"]["aggregateWallMs"]:
        fail("release aggregate exceeded its read-only wall budget")
    report["contentIdentitySha256"] = identity(report)
    write_json(output / MANIFEST_NAME, report)
    validate_aggregate(root, report, policy, dag_sha)
    return report


def validate_aggregate(
    root: Path,
    report: Mapping[str, Any],
    policy: Mapping[str, Any],
    dag_sha: str,
) -> None:
    exact_keys(
        report,
        {
            "contentIdentitySha256", "dag", "derivedAtUtc", "derivation", "fanout", "github",
            "history", "kind", "productReleaseQualified", "profileOperational",
            "readinessStatus", "source", "status", "targetDispositions", "version",
            "workers",
        },
        "release aggregate",
    )
    if (
        report["kind"] != AGGREGATE_KIND
        or report["version"] != VERSION
        or report["status"] != "pass"
        or report["profileOperational"] is not True
        or report["productReleaseQualified"] is not False
        or report["readinessStatus"] != "unsupported-product"
        or report["contentIdentitySha256"] != identity(report)
        or report["source"] != health_evidence.source_inventory(root)
        or report["dag"] != {
            "identitySha256": dag_sha,
            "path": "policies/release_evidence_dag_v0.2.json",
        }
    ):
        fail("release aggregate identity, status, or nonclaim mismatch")
    derivation = exact_keys(
        report["derivation"], {"readOnly", "rejectedConditions", "wallMs"},
        "aggregate derivation",
    )
    if (
        derivation["readOnly"] is not True
        or derivation["rejectedConditions"] != policy["aggregate"]["reject"]
        or not isinstance(derivation["wallMs"], int)
        or not 1 <= derivation["wallMs"] <= policy["budgets"]["aggregateWallMs"]
    ):
        fail("release aggregate derivation contract mismatch")
    github = exact_keys(
        report["github"], {"repository", "runAttempt", "runId", "sha"},
        "aggregate GitHub provenance",
    )
    if (
        "/" not in github["repository"]
        or not github["runAttempt"].isdigit()
        or github["runAttempt"].startswith("0")
        or not github["runId"].isdigit()
        or github["runId"].startswith("0")
        or github["sha"] != report["source"]["gitCommit"]
    ):
        fail("release aggregate GitHub provenance mismatch")
    fanout_receipt = exact_keys(
        report["fanout"],
        {
            "artifactName", "bundleIdentitySha256", "digestSha256",
            "manifestSha256", "producer",
        },
        "aggregate fanout receipt",
    )
    if (
        fanout_receipt["producer"]
        != {"cacheState": "cold", "evidenceClass": "cache-sensitive", "index": 1}
        or any(
            not is_sha256(fanout_receipt[name])
            for name in ("bundleIdentitySha256", "digestSha256", "manifestSha256")
        )
        or fanout_receipt["artifactName"]
        != fanout.artifact_name(github)
    ):
        fail("release aggregate fanout receipt mismatch")
    history = exact_keys(
        report["history"],
        {
            "coldP95ArtifactBytes", "coldP95PeakRssBytes", "coldP95WallMs",
            "samplesPerCacheState", "warmP95ArtifactBytes", "warmP95PeakRssBytes",
            "warmP95WallMs",
        },
        "aggregate history",
    )
    if history["samplesPerCacheState"] != 3:
        fail("release aggregate cache cohort is not the required odd sample count")
    for name, value in history.items():
        if name == "samplesPerCacheState":
            continue
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            fail("release aggregate history contains an invalid observation")
        if ("WallMs" in name and value > policy["budgets"]["measuredWorkerWallMs"]) or (
            "ArtifactBytes" in name and value > policy["budgets"]["artifactBytesPerWorker"]
        ):
            fail("release aggregate history exceeds a resource bound")
    workers = report["workers"]
    if not isinstance(workers, list) or len(workers) != 10:
        fail("release aggregate worker summary is incomplete")
    summary_nodes = []
    content_ids = set()
    manifest_ids = set()
    for row in workers:
        summary = exact_keys(
            row,
            {
                "cacheState", "contentIdentitySha256", "evidenceClass", "index",
                "manifestSha256",
            },
            "aggregate worker summary",
        )
        summary_nodes.append(
            (summary["evidenceClass"], summary["cacheState"], summary["index"])
        )
        if not is_sha256(summary["contentIdentitySha256"]) or not is_sha256(
            summary["manifestSha256"]
        ):
            fail("release aggregate worker summary identity is invalid")
        content_ids.add(summary["contentIdentitySha256"])
        manifest_ids.add(summary["manifestSha256"])
    if (
        summary_nodes != expected_node_keys()
        or len(content_ids) != 10
        or len(manifest_ids) != 10
    ):
        fail("release aggregate worker summaries are missing, reordered, or reused")
    targets = report["targetDispositions"]
    if not isinstance(targets, list) or [row.get("target") for row in targets] != TARGETS:
        fail("release aggregate target disposition summary is incomplete")
    for target in targets:
        exact_keys(
            target,
            {
                "expectedOutcome", "githubRunAttempt", "githubRunId", "githubSha",
                "releaseQualified", "reportArtifact", "reportSha256", "runner", "target",
            },
            "aggregate target disposition",
        )
        if (
            target["expectedOutcome"] != "unsupported-product"
            or target["releaseQualified"] is not False
            or target["githubRunAttempt"] != github["runAttempt"]
            or target["githubRunId"] != github["runId"]
            or target["githubSha"] != github["sha"]
            or not is_sha256(target["reportSha256"])
        ):
            fail("release aggregate target disposition was relabeled or crossed runs")


def self_test(root: Path) -> int:
    policy, dag_sha = policy_context(root)
    setup = active_command_ids(policy, "cache-sensitive", "setup", "agent-gpu-fallback")
    commands = active_command_ids(
        policy, "cache-sensitive", "commands", "agent-gpu-fallback"
    )
    all_cache = active_command_ids(policy, "cache-sensitive", "all", "agent-gpu-fallback")
    if setup + commands != all_cache or len(all_cache) != 19:
        fail("release cache command phase partition is not exact")
    producer = {
        "artifactName": "release-evidence-fanout-42-1-" + "a" * 40,
        "bundleIdentitySha256": "b" * 64,
        "dagIdentitySha256": dag_sha,
        "digestSha256": "d" * 64,
        "evidenceClass": "cache-sensitive",
        "index": 1,
        "manifestSha256": "c" * 64,
        "role": "producer",
    }
    consumer = {
        **producer,
        "role": "consumer",
    }
    workers = []
    for position, (evidence_class, cache_state, index) in enumerate(expected_node_keys(), 1):
        workers.append(
            {
                "fanout": (
                    producer
                    if (evidence_class, cache_state, index)
                    == ("cache-sensitive", "cold", 1)
                    else consumer if evidence_class != "cache-sensitive" else None
                ),
                "node": {
                    "cacheState": cache_state,
                    "evidenceClass": evidence_class,
                    "index": index,
                    "isolation": {
                        "identitySha256": f"{position:064x}",
                        "nonceSha256": f"{position + 100:064x}",
                    },
                },
            }
        )
    validate_node_topology(workers)
    validate_isolation_set(workers)
    validate_fanout_custody(workers, dag_sha)
    controls = health_evidence.source_inventory_self_test()
    mutations = [
        lambda rows: rows.pop(),
        lambda rows: rows.append(copy.deepcopy(rows[0])),
        lambda rows: rows[0]["node"].__setitem__("index", 2),
    ]
    for mutate in mutations:
        candidate = copy.deepcopy(workers)
        mutate(candidate)
        try:
            validate_node_topology(candidate)
        except ExecutionError:
            controls += 1
        else:
            fail("release execution self-test accepted invalid node topology")
    environment = fanout.execution_environment_fixture()
    environment_workers = [
        {"executionEnvironment": copy.deepcopy(environment)} for _ in range(3)
    ]
    validate_worker_environment_cohort(environment_workers)
    environment_workers[1]["executionEnvironment"]["architecture"] = "forged-arch"
    environment_workers[1]["executionEnvironment"] = (
        fanout.reidentify_execution_environment(
            environment_workers[1]["executionEnvironment"]
        )
    )
    try:
        validate_worker_environment_cohort(environment_workers)
    except ExecutionError as exc:
        if "architecture" not in str(exc):
            fail("release execution self-test lost environment drift diagnostics")
        controls += 1
    else:
        fail("release execution self-test accepted worker environment drift")
    candidate = copy.deepcopy(workers)
    candidate[1]["node"]["isolation"]["nonceSha256"] = candidate[0]["node"]["isolation"][
        "nonceSha256"
    ]
    try:
        validate_isolation_set(candidate)
    except ExecutionError:
        controls += 1
    else:
        fail("release execution self-test accepted reused isolation")
    custody_mutations = [
        lambda rows: rows[1].__setitem__("fanout", copy.deepcopy(producer)),
        lambda rows: rows[6]["fanout"].__setitem__("manifestSha256", "e" * 64),
        lambda rows: rows[7].__setitem__(
            "fanout", {**rows[7]["fanout"], "digestSha256": "e" * 64}
        ),
    ]
    for mutate in custody_mutations:
        candidate = copy.deepcopy(workers)
        mutate(candidate)
        try:
            validate_fanout_custody(candidate, dag_sha)
        except ExecutionError:
            controls += 1
        else:
            fail("release execution self-test accepted invalid fanout custody")
    try:
        exact_keys(
            {name: None for name in WORKER_FIELDS} | {"status": "pass"},
            WORKER_FIELDS,
            "worker observation",
        )
    except ExecutionError:
        controls += 1
    else:
        fail("release execution self-test accepted a producer-authored verdict")
    try:
        fanout.exact_keys(
            {
                "artifact": {},
                "contentIdentitySha256": "f" * 64,
                "github": {},
                "kind": fanout.KIND,
                "producer": {},
                "version": fanout.VERSION,
                "verdict": "pass",
            },
            {
                "artifact", "contentIdentitySha256", "github", "kind", "producer",
                "version",
            },
            "fanout authentication",
        )
    except fanout.FanoutError:
        controls += 1
    else:
        fail("release execution self-test accepted an open fanout authentication")
    with tempfile.TemporaryDirectory(prefix="genesis-release-start-control-") as temp:
        output = Path(temp) / "worker"
        initialize_worker_output(output, "invariant", "not-measured", 1)
        require_initialized_output(output, "invariant", "not-measured", 1)
        start_path = output / ORCHESTRATION_DIR / "started.json"
        start = load_json(start_path)
        start["githubRunId"] = "forged"
        write_json(start_path, start)
        try:
            require_initialized_output(output, "invariant", "not-measured", 1)
        except ExecutionError:
            controls += 1
        else:
            fail("release execution self-test accepted a forged worker start")
    required_env = {
        name: "fixture"
        for name in SUDO_RELAY_ENV_NAMES | set(cargo_cache.load_policy(root)["buildEnvironment"])
    }
    required_env.update(
        {
            "CARGO_HOME": str(Path.home() / ".cargo"),
            "CI": "true",
            "HOME": str(Path.home()),
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "SUPER_SECRET_SENTINEL": "must-not-cross-sudo-boundary",
        }
    )
    relay = sudo_relay_environment(root, required_env)
    loopback_prefix = dependency_mirror._loopback_namespace_prefix(
        "unshare-sudo-net",
        ["/usr/bin/sudo", "-n", "/usr/bin/unshare", "--net", "--"],
        ip_tool="/usr/sbin/ip",
        setpriv_tool="/usr/bin/setpriv",
    )
    command, process_env = guarded_process_command(
        root,
        loopback_prefix,
        ["bash", "-c", "true"],
        required_env,
    )
    if (
        relay.get("PATH") != required_env["PATH"]
        or "SUPER_SECRET_SENTINEL" in relay
        or any("SUPER_SECRET_SENTINEL" in argument for argument in command)
        or process_env != {"LANG": "C", "PATH": "/usr/bin:/bin"}
        or shutil.which("rustc", path=relay["PATH"]) is None
        or not any("link set lo up" in argument for argument in command)
        or "/usr/bin/setpriv" not in command
    ):
        fail("release execution self-test found an open or incomplete sudo relay")
    controls += 1
    missing_path = dict(required_env)
    missing_path.pop("PATH")
    try:
        sudo_relay_environment(root, missing_path)
    except ExecutionError:
        controls += 1
    else:
        fail("release execution self-test accepted a sudo relay without PATH")
    return controls


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    sub = parser.add_subparsers(dest="action", required=True)
    prepare = sub.add_parser("prepare-cache")
    prepare.add_argument("--state", choices=("cold", "warm"), required=True)
    prepare.add_argument("--index", type=int, choices=(1, 2, 3), required=True)
    prepare.add_argument("--output", type=Path, required=True)
    finish = sub.add_parser("finish-cache")
    finish.add_argument("--state", choices=("cold", "warm"), required=True)
    finish.add_argument("--index", type=int, choices=(1, 2, 3), required=True)
    finish.add_argument("--output", type=Path, required=True)
    node = sub.add_parser("run-node")
    node.add_argument("--evidence-class", choices=("invariant", "stress-performance"), required=True)
    node.add_argument("--index", type=int, choices=(1, 2, 3), required=True)
    node.add_argument("--fanout-root", type=Path, required=True)
    node.add_argument("--fanout-token", required=True)
    node.add_argument("--output", type=Path, required=True)
    aggregate_parser = sub.add_parser("aggregate")
    aggregate_parser.add_argument("--worker-output", type=Path, action="append", default=[])
    aggregate_parser.add_argument("--target-report", type=Path, action="append", default=[])
    aggregate_parser.add_argument("--output", type=Path, required=True)
    verify_worker_parser = sub.add_parser("verify-worker")
    verify_worker_parser.add_argument("--output", type=Path, required=True)
    verify_aggregate_parser = sub.add_parser("verify-aggregate")
    verify_aggregate_parser.add_argument("--report", type=Path, required=True)
    initialize = sub.add_parser("initialize-worker")
    initialize.add_argument(
        "--evidence-class",
        choices=("cache-sensitive", "invariant", "stress-performance"),
        required=True,
    )
    initialize.add_argument(
        "--state", choices=("cold", "warm", "not-measured"), required=True
    )
    initialize.add_argument("--index", type=int, choices=(1, 2, 3), required=True)
    initialize.add_argument("--output", type=Path, required=True)
    sub.add_parser("self-test")
    args = parser.parse_args(argv)
    try:
        root = args.root.resolve(strict=True)
        if args.action == "initialize-worker":
            initialize_worker_output(
                args.output.resolve(), args.evidence_class, args.state, args.index
            )
            return 0
        if args.action == "prepare-cache":
            return prepare_cache(root, args.output.resolve(), args.state, args.index)
        if args.action == "finish-cache":
            return finish_cache(root, args.output.resolve(strict=True), args.state, args.index)
        if args.action == "run-node":
            return run_node(
                root, args.output.resolve(), args.evidence_class, args.index,
                args.fanout_root.resolve(strict=True), args.fanout_token,
            )
        if args.action == "aggregate":
            aggregate(
                root,
                args.output.resolve(),
                [path.resolve(strict=True) for path in args.worker_output],
                [path.resolve(strict=True) for path in args.target_report],
            )
            print("release-evidence-aggregate: pass")
            return 0
        if args.action == "self-test":
            controls = self_test(root)
            print(f"release-evidence-execution: self-test ok (negative_controls={controls})")
            return 0
        policy, dag_sha = policy_context(root)
        if args.action == "verify-worker":
            validate_worker(root, args.output.resolve(strict=True), policy, dag_sha)
            print("release-evidence-worker: verified")
        else:
            validate_aggregate(root, load_json(args.report), policy, dag_sha)
            print("release-evidence-aggregate: verified")
    except (
        ExecutionError,
        evidence_dag.DagError,
        fanout.FanoutError,
        health_evidence.EvidenceError,
        cargo_cache.CachePolicyError,
        dependency_mirror.MirrorError,
        OSError,
        UnicodeError,
        json.JSONDecodeError,
        subprocess.SubprocessError,
        ValueError,
    ) as exc:
        print(f"release-evidence-execution: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
