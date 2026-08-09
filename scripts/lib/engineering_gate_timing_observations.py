#!/usr/bin/env python3
"""Collect and validate non-authoritative engineering timing observations."""

from __future__ import annotations

import argparse
import copy
import io
import json
import os
from pathlib import Path
import re
import shlex
import signal
import subprocess
import sys
import tempfile
import time
from typing import Any, Iterable, Optional, Sequence

if os.name == "posix":
    import fcntl
else:  # The hosted collector is portable; local append collection is POSIX-only.
    fcntl = None  # type: ignore[assignment]


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/lib"))
import engineering_gate_timing_calibration as calibration
import reference_host_profiles


OBSERVATION_SCHEMA_PATH = (
    ROOT / "docs/spec/ENGINEERING_GATE_TIMING_OBSERVATION_v0.1.schema.json"
)
ZERO_SHA256 = "0" * 64
OBSERVATION_FIELDS = {
    "kind",
    "version",
    "classId",
    "observedAtUnixSeconds",
    "durationMs",
    "outcome",
    "failureKind",
    "exitCode",
    "cleanupStatus",
    "gitCommit",
    "hostObservationCanonicalJson",
    "hostIdentitySha256",
    "toolchainIdentityCanonicalJson",
    "toolchainIdentitySha256",
    "controlObservationCanonicalJson",
    "workloadIdentitySha256",
    "cachePrecondition",
    "competingLaneState",
    "sourceIdentity",
    "chainScope",
    "previousObservationSha256",
    "identitySha256",
}
CANDIDATE_FIELDS = {
    "kind",
    "version",
    "policySha256",
    "observationCount",
    "observationSetSha256",
    "classes",
    "proposedCeilings",
    "nonclaims",
}


class TimingObservationError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise TimingObservationError(message)


def canonical_json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def parse_canonical_json(raw: Any, label: str) -> Any:
    require(isinstance(raw, str) and raw, f"{label} must be canonical JSON text")
    try:
        value = json.loads(raw, object_pairs_hook=calibration.unique_pairs)
    except (json.JSONDecodeError, calibration.TimingCalibrationError) as exc:
        raise TimingObservationError(f"{label} is invalid JSON: {exc}") from exc
    require(raw == canonical_json(value), f"{label} is not canonical JSON")
    return value


def run_text(command: Sequence[str], *, allow_failure: bool = False) -> str:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        if allow_failure:
            return ""
        raise TimingObservationError(f"identity command failed: {command[0]}: {exc}") from exc
    if result.returncode != 0 and not allow_failure:
        raise TimingObservationError(
            f"identity command failed: {command[0]} exit={result.returncode}"
        )
    return result.stdout.strip()


def first_line(raw: str) -> str:
    return raw.splitlines()[0] if raw else ""


def toolchain_observation(runner: str) -> dict[str, Any]:
    nextest = run_text(["cargo", "nextest", "--version"], allow_failure=True)
    node = run_text(["node", "--version"], allow_failure=True)
    runner_version = (
        first_line(nextest)
        if runner == "nextest"
        else first_line(run_text(["cargo", "--version"]))
        if runner == "cargo"
        else os.environ.get("RUNNER_TRACKING_ID", "github-actions")
    )
    document = {
        "bash": first_line(run_text(["bash", "--version"])),
        "cargo": first_line(run_text(["cargo", "--version"])),
        "node": first_line(node) if node else None,
        "python": sys.version.split()[0],
        "runner": runner,
        "runnerImage": (
            f"{os.environ.get('ImageOS', '')}/{os.environ.get('ImageVersion', '')}"
            if os.environ.get("GITHUB_ACTIONS") == "true"
            else "local"
        ),
        "runnerVersion": runner_version,
        "rustc": run_text(["rustc", "-vV"]),
    }
    validate_toolchain(document)
    return document


def validate_toolchain(document: Any) -> dict[str, Any]:
    return calibration.validate_toolchain_observation(document)


def load_host_policy() -> dict[str, Any]:
    policy = reference_host_profiles.load_json(reference_host_profiles.POLICY)
    return dict(reference_host_profiles.validate_policy(policy))


def host_observation(*, require_conformant: bool) -> dict[str, Any]:
    policy = load_host_policy()
    document = dict(reference_host_profiles.probe(policy))
    reference_host_profiles.validate_observation(
        document, policy, require_conformant=require_conformant
    )
    return document


def validate_control(
    document: Any, class_id: str, policy: dict[str, Any]
) -> dict[str, Any]:
    return calibration.validate_control_observation(
        document,
        class_id,
        local_background_load_limit_basis_points=(
            policy["localPreflight"]["backgroundLoadMaxPercent"] * 100
            if class_id.startswith("local-")
            else None
        ),
    )


def observation_identity(document: dict[str, Any]) -> str:
    payload = {key: value for key, value in document.items() if key != "identitySha256"}
    return calibration.canonical_sha256(payload)


def policy_maps(policy: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    return (
        {row["id"]: row for row in policy["classes"]},
        {row["id"]: row for row in policy["workloads"]},
    )


def validate_observation(document: Any, policy: dict[str, Any]) -> dict[str, Any]:
    require(
        isinstance(document, dict) and set(document) == OBSERVATION_FIELDS,
        "timing observation fields mismatch",
    )
    require(
        document["kind"] == "genesis/engineering-gate-timing-observation-v0.1"
        and document["version"] == "0.1",
        "timing observation identity drift",
    )
    classes, workloads = policy_maps(policy)
    class_id = document["classId"]
    require(class_id in classes, "timing observation class is unknown")
    class_policy = classes[class_id]
    require(
        isinstance(document["observedAtUnixSeconds"], int)
        and document["observedAtUnixSeconds"] > 0
        and isinstance(document["durationMs"], int)
        and document["durationMs"] > 0,
        "timing observation timestamp/duration invalid",
    )
    require(
        isinstance(document["gitCommit"], str)
        and len(document["gitCommit"]) == 40
        and all(char in "0123456789abcdef" for char in document["gitCommit"]),
        "timing observation commit invalid",
    )
    expected_source = class_policy["sourceIdentityKind"] + ":"
    require(
        isinstance(document["sourceIdentity"], str)
        and document["sourceIdentity"].startswith(expected_source)
        and len(document["sourceIdentity"]) > len(expected_source),
        "timing observation source/class mismatch",
    )
    require(
        document["cachePrecondition"] == class_policy["cachePrecondition"]
        and document["competingLaneState"] == class_policy["competingLaneState"],
        "timing observation class facts were relabeled",
    )
    workload = workloads[class_policy["workloadIdentity"]]
    require(
        document["workloadIdentitySha256"] == calibration.canonical_sha256(workload),
        "timing observation workload identity mismatch",
    )
    host = parse_canonical_json(
        document["hostObservationCanonicalJson"], "host observation"
    )
    host_policy = load_host_policy()
    reference_host_profiles.validate_observation(host, host_policy)
    require(
        document["hostIdentitySha256"] == host["identitySha256"],
        "timing host identity mismatch",
    )
    toolchain = validate_toolchain(
        parse_canonical_json(
            document["toolchainIdentityCanonicalJson"], "toolchain observation"
        )
    )
    require(
        document["toolchainIdentitySha256"]
        == calibration.canonical_sha256(toolchain),
        "timing toolchain identity mismatch",
    )
    control = validate_control(
        parse_canonical_json(
            document["controlObservationCanonicalJson"], "control observation"
        ),
        class_id,
        policy,
    )
    require(
        control["referenceHostConformant"] == host["conformance"]["ok"],
        "timing control host-conformance claim mismatches its host observation",
    )
    require(
        control["cacheState"] == class_policy["cachePrecondition"]
        and control["competingLaneState"] == class_policy["competingLaneState"],
        "timing control observation does not prove its class",
    )
    if document["outcome"] == "semantic-pass":
        require(
            document["failureKind"] is None
            and document["exitCode"] == 0
            and document["cleanupStatus"] == "reaped"
            and document["durationMs"] <= class_policy["hardCeilingMs"],
            "semantic pass does not satisfy terminal containment",
        )
    else:
        require(
            document["outcome"] == "hard-failure"
            and document["failureKind"]
            in {
                "command-failure",
                "competing-lane",
                "hard-timeout",
                "infrastructure-failure",
                "telemetry-budget",
                "interrupted",
            }
            and (
                document["exitCode"] is None
                or (
                    isinstance(document["exitCode"], int)
                    and not isinstance(document["exitCode"], bool)
                )
            )
            and document["cleanupStatus"] in {"reaped", "containment-failure"},
            "hard failure terminal facts invalid",
        )
    calibration.validate_competing_lane_outcome(
        control, class_id, document["outcome"], document["failureKind"]
    )
    if class_id.startswith("local-"):
        require(
            document["chainScope"] == "append-only-local",
            "local observation is not append-only scoped",
        )
    else:
        require(
            host["platformId"] == "linux-x86-64"
            and host["metadata"]["operatingSystem"]["family"] == "linux",
            "hosted timing observation is not an x86-64 Linux shared runner",
        )
        require(
            toolchain["runner"] == "github-actions"
            and toolchain["runnerImage"].startswith("ubuntu24/"),
            "hosted timing observation lacks the declared runner image",
        )
        require(
            document["chainScope"] == "standalone-hosted"
            and document["previousObservationSha256"] == ZERO_SHA256,
            "hosted observation claims an unverifiable local chain",
        )
    require(
        isinstance(document["previousObservationSha256"], str)
        and len(document["previousObservationSha256"]) == 64
        and all(
            char in "0123456789abcdef"
            for char in document["previousObservationSha256"]
        ),
        "previous timing observation identity invalid",
    )
    require(
        document["identitySha256"] == observation_identity(document),
        "timing observation content identity mismatch",
    )
    return document


def load_history(path: Path, policy: dict[str, Any]) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    require(path.is_file() and not path.is_symlink(), "timing history must be a regular file")
    records: list[dict[str, Any]] = []
    previous_local = ZERO_SHA256
    previous_time = 0
    sources: set[str] = set()
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw:
            continue
        try:
            record = json.loads(raw, object_pairs_hook=calibration.unique_pairs)
        except (json.JSONDecodeError, calibration.TimingCalibrationError) as exc:
            raise TimingObservationError(
                f"timing history line {line_number} is invalid: {exc}"
            ) from exc
        validate_observation(record, policy)
        require(record["sourceIdentity"] not in sources, "duplicate timing source identity")
        require(
            record["observedAtUnixSeconds"] >= previous_time,
            "timing history chronology moved backward",
        )
        if record["chainScope"] == "append-only-local":
            require(
                record["previousObservationSha256"] == previous_local,
                "local timing history chain is broken",
            )
            previous_local = record["identitySha256"]
        sources.add(record["sourceIdentity"])
        previous_time = record["observedAtUnixSeconds"]
        records.append(record)
    return records


def git_output(*args: str) -> str:
    return run_text(["git", *args])


BUILD_PROCESS_NAMES = {
    "cargo",
    "cargo-nextest",
    "genesis",
    "nextest",
    "quarto",
    "rustc",
}
SHELL_NAMES = {"bash", "dash", "sh", "zsh"}


def command_tokens(command: str) -> list[str]:
    try:
        return shlex.split(command)
    except ValueError:
        return command.split()


def build_process_class(command: str) -> Optional[str]:
    tokens = command_tokens(command)
    if not tokens:
        return None
    executable = Path(tokens[0]).name
    if executable in BUILD_PROCESS_NAMES:
        return executable
    if executable == "env":
        index = 1
        while index < len(tokens) and (
            tokens[index].startswith("-") or "=" in tokens[index]
        ):
            index += 1
        return build_process_class(" ".join(tokens[index:]))
    if executable in SHELL_NAMES:
        index = 1
        while index < len(tokens) and tokens[index].startswith("-"):
            if "c" in tokens[index] and index + 1 < len(tokens):
                return build_process_class(tokens[index + 1])
            index += 1
        if index < len(tokens):
            wrapped = Path(tokens[index]).name
            return wrapped if wrapped in BUILD_PROCESS_NAMES else None
        return None
    if executable in {"deno", "node"} and any(
        Path(token).name in {"quarto", "quarto.js"} for token in tokens[1:]
    ):
        return "quarto"
    if executable == "rustup":
        for token in tokens[1:]:
            wrapped = Path(token).name
            if wrapped in BUILD_PROCESS_NAMES:
                return wrapped
    return None


def parse_process_snapshot(output: str) -> list[dict[str, Any]]:
    rows = []
    for line in output.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) != 3:
            continue
        try:
            pid = int(parts[0])
            parent_pid = int(parts[1])
        except ValueError:
            continue
        rows.append(
            {
                "pid": pid,
                "parentPid": parent_pid,
                "command": parts[2],
                "buildClass": build_process_class(parts[2]),
            }
        )
    return rows


def process_snapshot() -> list[dict[str, Any]]:
    return parse_process_snapshot(run_text(["ps", "-axo", "pid=,ppid=,command="]))


def descendant_pids(rows: Sequence[dict[str, Any]], root_pid: int) -> set[int]:
    owned = {root_pid}
    changed = True
    while changed:
        changed = False
        for row in rows:
            if row["parentPid"] in owned and row["pid"] not in owned:
                owned.add(row["pid"])
                changed = True
    return owned


def external_competing_builds(
    rows: Sequence[dict[str, Any]], owned_root_pid: Optional[int] = None
) -> list[dict[str, Any]]:
    owned = (
        descendant_pids(rows, owned_root_pid)
        if owned_root_pid is not None
        else {os.getpid()}
    )
    return sorted(
        [
            row
            for row in rows
            if row["pid"] not in owned and row["buildClass"] is not None
        ],
        key=lambda row: (row["buildClass"], row["pid"]),
    )


def competing_process_count() -> int:
    return len(external_competing_builds(process_snapshot()))


def thermal_state() -> str:
    if sys.platform != "darwin":
        return "nominal"
    output = run_text(["pmset", "-g", "therm"], allow_failure=True).lower()
    if not output:
        return "unknown"
    if "no thermal warning" in output:
        return "nominal"
    limits = [
        int(value)
        for value in re.findall(r"(?:cpu|gpu|speed)_speed_limit\s*=\s*(\d+)", output)
    ]
    return "nominal" if limits and min(limits) >= 100 else "unknown"


def resolved_default_cache() -> Path:
    output = run_text(
        [
            sys.executable,
            "scripts/lib/cargo_cache.py",
            "--root",
            str(ROOT),
            "--scope",
            "root-host",
            "--format",
            "path",
            "--no-materialize",
        ]
    )
    return Path(output)


def sampled_background_load_basis_points(
    logical_cpus: int, sample_count: int, sample_interval_ms: int
) -> int:
    samples = []
    for index in range(sample_count):
        samples.append(round(os.getloadavg()[0] * 10000 / logical_cpus))
        if index + 1 < sample_count:
            time.sleep(sample_interval_ms / 1000)
    return max(samples)


def local_preflight(
    class_id: str, host: dict[str, Any], policy: dict[str, Any]
) -> dict[str, Any]:
    require(git_output("branch", "--show-current") == "main", "local timing requires main")
    require(not git_output("status", "--porcelain"), "local timing requires a clean worktree")
    head = git_output("rev-parse", "HEAD")
    require(head == git_output("rev-parse", "origin/main"), "local timing requires exact origin/main")
    local_policy = policy["localPreflight"]
    load_limit = local_policy["backgroundLoadMaxPercent"] * 100
    logical_cpus = host["metadata"]["cpu"]["logicalCores"]
    load_basis_points = sampled_background_load_basis_points(
        logical_cpus,
        local_policy["backgroundLoadSampleCount"],
        local_policy["backgroundLoadSampleIntervalMs"],
    )
    competing = competing_process_count()
    thermal = thermal_state()
    require(load_basis_points <= load_limit, "local timing background load is too high")
    require(
        competing <= local_policy["competingBuildProcessMaxCount"],
        "local timing has competing build processes",
    )
    require(thermal == "nominal", "local timing thermal state is not nominal")
    if class_id == "local-warm":
        cache = resolved_default_cache()
        require(cache.is_dir(), "local warm cache is absent")
        require(
            any(path.is_file() and path.name != ".genesis-cargo-cache-key.json" for path in cache.rglob("*")),
            "local warm cache has no reusable build products",
        )
    return {
        "backgroundLoadBasisPoints": load_basis_points,
        "backgroundLoadLimitBasisPoints": load_limit,
        "cacheState": {
            "local-warm": "warm-reusable-root-host-target",
            "local-clean-fallback": "empty-generated-authority-stage-and-declared-fallback-cache",
        }[class_id],
        "competingLaneState": "exclusive",
        "competingProcessCount": competing,
        "exactRevision": True,
        "referenceHostConformant": True,
        "source": "local-preflight-and-runtime-monitor",
        "thermalState": thermal,
    }


def wait_for_local_process(
    process: subprocess.Popen[Any],
    *,
    timeout_ms: int,
    poll_interval_ms: int,
    log: Any,
) -> tuple[Optional[int], bool, bool, int]:
    deadline_ns = time.monotonic_ns() + timeout_ms * 1_000_000
    maximum_competing = 0
    monitor_failed = False
    timed_out = False
    exit_code: Optional[int] = None
    while True:
        if process.poll() is not None:
            exit_code = process.returncode
            break
        try:
            competitors = external_competing_builds(process_snapshot(), process.pid)
        except TimingObservationError as exc:
            monitor_failed = True
            log.write(f"runtime exclusivity monitor failure: {exc}\n".encode("utf-8"))
            break
        maximum_competing = max(maximum_competing, len(competitors))
        if competitors:
            classes = ",".join(sorted({row["buildClass"] for row in competitors}))
            log.write(
                (
                    "runtime exclusivity violation: "
                    f"count={len(competitors)} classes={classes}\n"
                ).encode("ascii")
            )
            break
        remaining_ns = deadline_ns - time.monotonic_ns()
        if remaining_ns <= 0:
            timed_out = True
            break
        wait_seconds = min(poll_interval_ms / 1000, remaining_ns / 1_000_000_000)
        try:
            exit_code = process.wait(timeout=wait_seconds)
            break
        except subprocess.TimeoutExpired:
            continue
        except OSError as exc:
            monitor_failed = True
            log.write(f"collector wait failure: {exc}\n".encode("utf-8"))
            break
    return exit_code, timed_out, monitor_failed, maximum_competing


def terminate_group(process: subprocess.Popen[Any]) -> str:
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                return "containment-failure"
    try:
        os.killpg(process.pid, 0)
    except ProcessLookupError:
        return "reaped"
    except PermissionError:
        return "containment-failure"
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        return "reaped"
    time.sleep(0.2)
    try:
        os.killpg(process.pid, 0)
    except ProcessLookupError:
        return "reaped"
    return "containment-failure"


def build_observation(
    *,
    class_policy: dict[str, Any],
    workload: dict[str, Any],
    duration_ms: int,
    outcome: str,
    failure_kind: Optional[str],
    exit_code: Optional[int],
    cleanup_status: str,
    git_commit: str,
    host: dict[str, Any],
    toolchain: dict[str, Any],
    control: dict[str, Any],
    source_identity: str,
    chain_scope: str,
    previous: str,
) -> dict[str, Any]:
    document = {
        "kind": "genesis/engineering-gate-timing-observation-v0.1",
        "version": "0.1",
        "classId": class_policy["id"],
        "observedAtUnixSeconds": int(time.time()),
        "durationMs": max(1, duration_ms),
        "outcome": outcome,
        "failureKind": failure_kind,
        "exitCode": exit_code,
        "cleanupStatus": cleanup_status,
        "gitCommit": git_commit,
        "hostObservationCanonicalJson": canonical_json(host),
        "hostIdentitySha256": host["identitySha256"],
        "toolchainIdentityCanonicalJson": canonical_json(toolchain),
        "toolchainIdentitySha256": calibration.canonical_sha256(toolchain),
        "controlObservationCanonicalJson": canonical_json(control),
        "workloadIdentitySha256": calibration.canonical_sha256(workload),
        "cachePrecondition": class_policy["cachePrecondition"],
        "competingLaneState": class_policy["competingLaneState"],
        "sourceIdentity": source_identity,
        "chainScope": chain_scope,
        "previousObservationSha256": previous,
    }
    document["identitySha256"] = observation_identity(document)
    return document


def append_local_history(path: Path, document: dict[str, Any], policy: dict[str, Any]) -> None:
    require(fcntl is not None, "local timing append requires POSIX file locking")
    expected = (ROOT / policy["observationHistoryPath"]).resolve()
    require(path.resolve() == expected, "local timing history path is not policy-owned")
    path.parent.mkdir(parents=True, exist_ok=True)
    lock_path = path.with_suffix(path.suffix + ".lock")
    with lock_path.open("a+", encoding="ascii") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        records = load_history(path, policy)
        previous = next(
            (
                row["identitySha256"]
                for row in reversed(records)
                if row["chainScope"] == "append-only-local"
            ),
            ZERO_SHA256,
        )
        require(
            document["previousObservationSha256"] == previous,
            "local timing append raced or used a stale head",
        )
        validate_observation(document, policy)
        encoded = canonical_json(document).encode("ascii") + b"\n"
        fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
        try:
            remaining = memoryview(encoded)
            while remaining:
                written = os.write(fd, remaining)
                require(written > 0, "timing history append made no progress")
                remaining = remaining[written:]
            os.fsync(fd)
        finally:
            os.close(fd)
        load_history(path, policy)


def acquire_run_lock(path: Path) -> int:
    require(fcntl is not None, "local timing collection requires POSIX file locking")
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError as exc:
        os.close(fd)
        raise TimingObservationError("another local timing collector owns the run lock") from exc
    os.ftruncate(fd, 0)
    os.write(fd, f"{os.getpid()}\n".encode("ascii"))
    os.fsync(fd)
    return fd


def record_local(class_id: str, history_path: Path) -> dict[str, Any]:
    expected = (
        ROOT
        / calibration.validate_policy(
            calibration.load_json(calibration.POLICY_PATH)
        )["observationHistoryPath"]
    ).resolve()
    require(history_path.resolve() == expected, "record-local requires the policy history path")
    run_lock = history_path.with_suffix(history_path.suffix + ".run.lock")
    lock_fd = acquire_run_lock(run_lock)
    try:
        return _record_local(class_id, history_path)
    finally:
        assert fcntl is not None
        fcntl.flock(lock_fd, fcntl.LOCK_UN)
        os.close(lock_fd)


def _record_local(class_id: str, history_path: Path) -> dict[str, Any]:
    policy = calibration.validate_policy(calibration.load_json(calibration.POLICY_PATH))
    classes, workloads = policy_maps(policy)
    require(class_id in {"local-warm", "local-clean-fallback"}, "record-local requires a local class")
    class_policy = classes[class_id]
    workload = workloads[class_policy["workloadIdentity"]]
    host = host_observation(require_conformant=True)
    control = local_preflight(class_id, host, policy)
    toolchain = toolchain_observation(workload["runner"])
    records = load_history(history_path, policy)
    previous = next(
        (
            row["identitySha256"]
            for row in reversed(records)
            if row["chainScope"] == "append-only-local"
        ),
        ZERO_SHA256,
    )
    head = git_output("rev-parse", "HEAD")
    source = f"local-observation:{head}:{class_id}:{time.time_ns()}"
    log_root = history_path.parent / "engineering_gate_timing_logs"
    log_root.mkdir(parents=True, exist_ok=True)
    safe_source = calibration.canonical_sha256(source)[:24]
    log_path = log_root / f"{safe_source}.log"
    started = time.monotonic_ns()
    timed_out = False
    launch_failed = False
    monitor_failed = False
    competing_processes = 0
    exit_code: Optional[int] = None
    cleanup_status = "reaped"
    report: Any = None
    with tempfile.TemporaryDirectory(prefix="genesis-timing-") as raw_temp:
        temp = Path(raw_temp)
        changed = temp / "changed.txt"
        report_path = temp / "report.json"
        metrics_history = temp / "history.jsonl"
        changed.write_text("\n".join(workload["changedFiles"]) + "\n", encoding="ascii")
        command = [
            "bash",
            workload["command"],
            "--base",
            "HEAD",
            "--runner",
            workload["runner"],
            "--min-history",
            "1",
            "--changed-files-from",
            str(changed),
            "--report",
            str(report_path),
            "--history",
            str(metrics_history),
        ]
        environment = os.environ.copy()
        if class_id == "local-clean-fallback":
            clean_cache = temp / "clean-cargo-cache"
            require(not clean_cache.exists(), "clean timing cache already exists")
            environment["GENESIS_CARGO_CACHE_ROOT"] = str(clean_cache)
        with log_path.open("wb") as log:
            try:
                process = subprocess.Popen(
                    command,
                    cwd=ROOT,
                    env=environment,
                    stdout=log,
                    stderr=subprocess.STDOUT,
                    start_new_session=True,
                )
            except OSError as exc:
                launch_failed = True
                log.write(f"collector launch failure: {exc}\n".encode("utf-8"))
            else:
                (
                    exit_code,
                    timed_out,
                    monitor_failed,
                    competing_processes,
                ) = wait_for_local_process(
                    process,
                    timeout_ms=class_policy["hardCeilingMs"],
                    poll_interval_ms=policy["localPreflight"][
                        "competingBuildPollIntervalMs"
                    ],
                    log=log,
                )
                cleanup_status = terminate_group(process)
                if exit_code is None and process.returncode is not None:
                    exit_code = process.returncode
        if report_path.is_file():
            try:
                report = json.loads(
                    report_path.read_text(encoding="utf-8"),
                    object_pairs_hook=calibration.unique_pairs,
                )
            except (OSError, UnicodeError, json.JSONDecodeError, calibration.TimingCalibrationError):
                report = None
    duration_ms = max(1, (time.monotonic_ns() - started) // 1_000_000)
    report_valid = (
        isinstance(report, dict)
        and report.get("kind") == "genesis/test-changed-fast-metrics-v0.1"
        and report.get("mode") == "profile-fallback"
        and report.get("runner") == workload["runner"]
        and report.get("budget_subject") == "prepush-standard"
        and report.get("changed_file_count") == len(workload["changedFiles"])
        and report.get("budget_ms") == workload["budgetMs"]
    )
    semantic_pass = (
        not launch_failed
        and not monitor_failed
        and not timed_out
        and competing_processes == 0
        and exit_code == 0
        and cleanup_status == "reaped"
        and report_valid
        and duration_ms <= class_policy["hardCeilingMs"]
    )
    if semantic_pass:
        failure_kind = None
    elif competing_processes > 0:
        failure_kind = "competing-lane"
    elif timed_out:
        failure_kind = "hard-timeout"
    elif launch_failed or monitor_failed or cleanup_status != "reaped":
        failure_kind = "infrastructure-failure"
    elif report_valid and (
        report.get("elapsed_ms", 0) > report.get("budget_ms", 0)
        or report.get("generated_disk_delta_bytes", 0)
        > report.get("generated_disk_budget_bytes", 0)
    ):
        failure_kind = "telemetry-budget"
    else:
        failure_kind = "command-failure"
    control["competingProcessCount"] = competing_processes
    document = build_observation(
        class_policy=class_policy,
        workload=workload,
        duration_ms=duration_ms,
        outcome="semantic-pass" if semantic_pass else "hard-failure",
        failure_kind=failure_kind,
        exit_code=exit_code,
        cleanup_status=cleanup_status,
        git_commit=head,
        host=host,
        toolchain=toolchain,
        control=control,
        source_identity=source,
        chain_scope="append-only-local",
        previous=previous,
    )
    append_local_history(history_path, document, policy)
    print(
        "engineering-gate-timing-observation: recorded "
        f"class={class_id} outcome={document['outcome']} duration_ms={duration_ms} "
        f"identity={document['identitySha256']} log={log_path}"
    )
    return document


def begin_hosted(path: Path) -> None:
    require(os.environ.get("GITHUB_ACTIONS") == "true", "begin-hosted requires GitHub Actions")
    document = {
        "kind": "genesis/engineering-gate-timing-hosted-start-v0.1",
        "gitCommit": os.environ.get("GITHUB_SHA"),
        "runAttempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "runId": os.environ.get("GITHUB_RUN_ID"),
        "startedMonotonicNs": time.monotonic_ns(),
        "startedUnixSeconds": int(time.time()),
        "version": "0.1",
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="ascii")


def record_hosted(start_path: Path, status: str, output: Path) -> dict[str, Any]:
    policy = calibration.validate_policy(calibration.load_json(calibration.POLICY_PATH))
    classes, workloads = policy_maps(policy)
    class_policy = classes["hosted-cold-shared-runner"]
    workload = workloads[class_policy["workloadIdentity"]]
    require(os.environ.get("GITHUB_ACTIONS") == "true", "record-hosted requires GitHub Actions")
    require(os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch", "hosted timing requires workflow_dispatch")
    require(os.environ.get("GITHUB_REF_NAME") == "main", "hosted timing requires main")
    require(os.environ.get("GENESIS_CI_PROFILE") == "standard", "hosted timing requires standard profile")
    require(os.environ.get("GENESIS_CI_LANE") == "standard", "hosted timing requires standard lane")
    require(status in {"success", "failure", "cancelled"}, "hosted timing status is invalid")
    start = calibration.load_json(start_path)
    require(
        start
        == {
            "kind": "genesis/engineering-gate-timing-hosted-start-v0.1",
            "gitCommit": os.environ.get("GITHUB_SHA"),
            "runAttempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
            "runId": os.environ.get("GITHUB_RUN_ID"),
            "startedMonotonicNs": start.get("startedMonotonicNs"),
            "startedUnixSeconds": start.get("startedUnixSeconds"),
            "version": "0.1",
        }
        and isinstance(start["startedMonotonicNs"], int)
        and isinstance(start["startedUnixSeconds"], int),
        "hosted timing start envelope mismatch",
    )
    duration_ms = max(1, (time.monotonic_ns() - start["startedMonotonicNs"]) // 1_000_000)
    host = host_observation(require_conformant=False)
    toolchain = toolchain_observation("github-actions")
    control = {
        "backgroundLoadBasisPoints": None,
        "backgroundLoadLimitBasisPoints": None,
        "cacheState": class_policy["cachePrecondition"],
        "competingLaneState": class_policy["competingLaneState"],
        "competingProcessCount": 0,
        "exactRevision": True,
        "referenceHostConformant": host["conformance"]["ok"],
        "source": "github-actions-context",
        "thermalState": "unknown",
    }
    run_id = os.environ["GITHUB_RUN_ID"]
    attempt = os.environ["GITHUB_RUN_ATTEMPT"]
    sha = os.environ["GITHUB_SHA"]
    source = f"github-actions-run:{run_id}:{attempt}:test_suite:standard:{sha}"
    semantic_pass = status == "success" and duration_ms <= class_policy["hardCeilingMs"]
    failure_kind = None
    if not semantic_pass:
        if duration_ms > class_policy["hardCeilingMs"]:
            failure_kind = "telemetry-budget"
        elif status == "cancelled":
            failure_kind = "interrupted"
        else:
            failure_kind = "command-failure"
    document = build_observation(
        class_policy=class_policy,
        workload=workload,
        duration_ms=duration_ms,
        outcome="semantic-pass" if semantic_pass else "hard-failure",
        failure_kind=failure_kind,
        exit_code=0 if semantic_pass else None,
        cleanup_status="reaped",
        git_commit=sha,
        host=host,
        toolchain=toolchain,
        control=control,
        source_identity=source,
        chain_scope="standalone-hosted",
        previous=ZERO_SHA256,
    )
    validate_observation(document, policy)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="ascii")
    print(
        "engineering-gate-timing-observation: wrote "
        f"class={document['classId']} outcome={document['outcome']} "
        f"duration_ms={duration_ms} identity={document['identitySha256']}"
    )
    return document


def sample_from_observation(record: dict[str, Any], sequence: int) -> dict[str, Any]:
    return {
        "sequence": sequence,
        "observedAtUnixSeconds": record["observedAtUnixSeconds"],
        "durationMs": record["durationMs"],
        "outcome": record["outcome"],
        "failureKind": record["failureKind"],
        "exitCode": record["exitCode"],
        "cleanupStatus": record["cleanupStatus"],
        "gitCommit": record["gitCommit"],
        "hostObservationCanonicalJson": record["hostObservationCanonicalJson"],
        "hostIdentitySha256": record["hostIdentitySha256"],
        "toolchainIdentityCanonicalJson": record["toolchainIdentityCanonicalJson"],
        "toolchainIdentitySha256": record["toolchainIdentitySha256"],
        "controlObservationCanonicalJson": record["controlObservationCanonicalJson"],
        "workloadIdentitySha256": record["workloadIdentitySha256"],
        "observationIdentitySha256": record["identitySha256"],
        "cachePrecondition": record["cachePrecondition"],
        "competingLaneState": record["competingLaneState"],
        "sourceIdentity": record["sourceIdentity"],
        "chainScope": record["chainScope"],
        "previousObservationSha256": record["previousObservationSha256"],
    }


def render_candidate(
    records: Iterable[dict[str, Any]], policy: dict[str, Any]
) -> dict[str, Any]:
    validated = [validate_observation(record, policy) for record in records]
    ordered = sorted(
        validated,
        key=lambda row: (
            row["observedAtUnixSeconds"],
            row["sourceIdentity"],
            row["identitySha256"],
        ),
    )
    identities = [row["identitySha256"] for row in ordered]
    require(len(identities) == len(set(identities)), "candidate repeats an observation")
    require(
        len({row["sourceIdentity"] for row in ordered}) == len(ordered),
        "candidate repeats a source identity",
    )
    calibration.validate_append_only_chain(
        ordered,
        "identitySha256",
        "timing review candidate",
    )
    classes = []
    ceilings = []
    for class_policy in policy["classes"]:
        rows = [row for row in ordered if row["classId"] == class_policy["id"]]
        warmups = []
        retained = []
        failures = []
        semantic_count = 0
        for sequence, row in enumerate(rows, 1):
            sample = sample_from_observation(row, sequence)
            if row["outcome"] == "hard-failure":
                failures.append(sample)
            elif semantic_count < policy["sampling"]["discardedWarmups"]:
                warmups.append(sample)
                semantic_count += 1
            else:
                retained.append(sample)
                semantic_count += 1
        values = [row["durationMs"] for row in retained]
        stats = (
            calibration.statistics(values, policy)
            if len(values) >= policy["sampling"]["retainedConformantSamples"]
            else None
        )
        proposed = (
            calibration.derive_hard_ceiling(stats, class_policy, policy)
            if stats is not None
            else None
        )
        classes.append(
            {
                "id": class_policy["id"],
                "warmups": warmups,
                "retainedSamples": retained,
                "failedSamples": failures,
                "statistics": stats,
                "trend": calibration.trend(values, policy) if stats is not None else None,
                "derivedHardCeiling": proposed,
            }
        )
        ceilings.append({"id": class_policy["id"], "proposal": proposed})
    candidate = {
        "kind": "genesis/engineering-gate-timing-review-candidate-v0.1",
        "version": "0.1",
        "policySha256": calibration.file_sha256(calibration.POLICY_PATH),
        "observationCount": len(ordered),
        "observationSetSha256": calibration.canonical_sha256(identities),
        "classes": classes,
        "proposedCeilings": ceilings,
        "nonclaims": [
            "This E0-derived candidate is not canonical evidence.",
            "A proposed ceiling does not authorize a policy or budget change.",
            "This candidate cannot close R0.4.j or qualify a release.",
        ],
    }
    require(set(candidate) == CANDIDATE_FIELDS, "candidate fields drift")
    return candidate


def fixture_host(class_id: str) -> dict[str, Any]:
    policy = load_host_policy()
    profile = (
        next(row for row in policy["profiles"] if row["platformId"] == "linux-x86-64")
        if class_id == "hosted-cold-shared-runner"
        else policy["profiles"][0]
    )
    return dict(reference_host_profiles.synthetic_observation(profile))


def fixture_observation(
    policy: dict[str, Any], class_id: str, source_suffix: str, previous: str
) -> dict[str, Any]:
    classes, workloads = policy_maps(policy)
    class_policy = classes[class_id]
    workload = workloads[class_policy["workloadIdentity"]]
    host = fixture_host(class_id)
    toolchain = {
        "bash": "GNU bash 5.2",
        "cargo": "cargo 1.90.0",
        "node": "v22.23.2",
        "python": "3.12.0",
        "runner": workload["runner"],
        "runnerImage": "local" if class_id.startswith("local-") else "ubuntu24/test",
        "runnerVersion": "fixture",
        "rustc": "rustc 1.90.0\nrelease: 1.90.0\nhost: aarch64-apple-darwin",
    }
    local = class_id.startswith("local-")
    control = {
        "backgroundLoadBasisPoints": 2500 if local else None,
        "backgroundLoadLimitBasisPoints": (
            policy["localPreflight"]["backgroundLoadMaxPercent"] * 100
            if local
            else None
        ),
        "cacheState": class_policy["cachePrecondition"],
        "competingLaneState": class_policy["competingLaneState"],
        "competingProcessCount": 0,
        "exactRevision": True,
        "referenceHostConformant": True,
        "source": (
            "local-preflight-and-runtime-monitor"
            if local
            else "github-actions-context"
        ),
        "thermalState": "nominal" if local else "unknown",
    }
    document = build_observation(
        class_policy=class_policy,
        workload=workload,
        duration_ms=100_000,
        outcome="semantic-pass",
        failure_kind=None,
        exit_code=0,
        cleanup_status="reaped",
        git_commit="1" * 40,
        host=host,
        toolchain=toolchain,
        control=control,
        source_identity=f"{class_policy['sourceIdentityKind']}:{source_suffix}",
        chain_scope="append-only-local" if local else "standalone-hosted",
        previous=previous if local else ZERO_SHA256,
    )
    return document


def self_test() -> int:
    policy = calibration.validate_policy(calibration.load_json(calibration.POLICY_PATH))
    load_samples = iter(
        [
            (0.8, 0.0, 0.0),
            (1.6, 0.0, 0.0),
            (1.2, 0.0, 0.0),
            (2.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
        ]
    )
    sleep_calls = []
    original_getloadavg = os.getloadavg
    original_sleep = time.sleep
    try:
        os.getloadavg = lambda: next(load_samples)
        time.sleep = lambda seconds: sleep_calls.append(seconds)
        require(
            sampled_background_load_basis_points(8, 5, 1000) == 2500,
            "local timing load sampler did not retain the maximum",
        )
        require(
            sleep_calls == [1.0, 1.0, 1.0, 1.0],
            "local timing load sampler interval count drifted",
        )
    finally:
        os.getloadavg = original_getloadavg
        time.sleep = original_sleep
    require(build_process_class("/usr/bin/cargo test") == "cargo", "direct Cargo was missed")
    require(
        build_process_class("/bin/bash /opt/quarto/bin/quarto render") == "quarto",
        "shell-wrapped Quarto was missed",
    )
    require(
        build_process_class("/opt/deno run /opt/quarto/bin/quarto.js render")
        == "quarto",
        "Deno-hosted Quarto was missed",
    )
    require(
        build_process_class("/bin/zsh -c 'cargo test -p gc_kernel'") == "cargo",
        "shell command Cargo was missed",
    )
    require(
        build_process_class("rg cargo scripts") is None,
        "non-build command text was misclassified",
    )
    process_fixture = parse_process_snapshot(
        "\n".join(
            [
                "100 1 bash scripts/test_changed_fast.sh",
                "101 100 /usr/bin/cargo test",
                "102 101 /usr/bin/rustc crate.rs",
                "200 1 /bin/bash /opt/quarto/bin/quarto render",
                "201 200 /opt/deno run /opt/quarto/bin/quarto.js render",
            ]
        )
    )
    require(
        {row["pid"] for row in external_competing_builds(process_fixture, 100)}
        == {200, 201},
        "owned descendant closure did not isolate external builds",
    )

    class PendingProcess:
        pid = 100
        returncode = None

        @staticmethod
        def poll() -> None:
            return None

        @staticmethod
        def wait(timeout: float) -> int:
            raise AssertionError(f"monitor waited despite a competitor: {timeout}")

    original_process_snapshot = process_snapshot
    monitor_log = io.BytesIO()
    try:
        globals()["process_snapshot"] = lambda: process_fixture
        monitor_result = wait_for_local_process(
            PendingProcess(), timeout_ms=1000, poll_interval_ms=100, log=monitor_log
        )
        require(
            monitor_result == (None, False, False, 2)
            and b"classes=quarto" in monitor_log.getvalue(),
            "runtime monitor did not fail closed on external builds",
        )
        globals()["process_snapshot"] = lambda: (_ for _ in ()).throw(
            TimingObservationError("fixture monitor failure")
        )
        require(
            wait_for_local_process(
                PendingProcess(),
                timeout_ms=1000,
                poll_interval_ms=100,
                log=io.BytesIO(),
            )
            == (None, False, True, 0),
            "runtime monitor failure did not fail closed",
        )
    finally:
        globals()["process_snapshot"] = original_process_snapshot
    first = fixture_observation(policy, "local-warm", "fixture-1", ZERO_SHA256)
    second = fixture_observation(
        policy, "local-clean-fallback", "fixture-2", first["identitySha256"]
    )
    hosted = fixture_observation(
        policy, "hosted-cold-shared-runner", "fixture-3", ZERO_SHA256
    )
    for record in (first, second, hosted):
        validate_observation(record, policy)
    controls = 12

    mutations = []
    candidate = copy.deepcopy(first)
    candidate["durationMs"] += 1
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    candidate["cachePrecondition"] = "empty"
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    control = parse_canonical_json(
        candidate["controlObservationCanonicalJson"], "fixture control"
    )
    control["competingProcessCount"] = 1
    candidate["controlObservationCanonicalJson"] = canonical_json(control)
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    candidate["outcome"] = "hard-failure"
    candidate["failureKind"] = "competing-lane"
    candidate["exitCode"] = -15
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    control = parse_canonical_json(
        candidate["controlObservationCanonicalJson"], "fixture control"
    )
    control["competingProcessCount"] = 1
    candidate["controlObservationCanonicalJson"] = canonical_json(control)
    candidate["outcome"] = "hard-failure"
    candidate["failureKind"] = "command-failure"
    candidate["exitCode"] = 1
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    candidate["workloadIdentitySha256"] = "f" * 64
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    candidate["sourceIdentity"] = "github-actions-run:wrong"
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    control = parse_canonical_json(
        candidate["controlObservationCanonicalJson"], "fixture control"
    )
    control["backgroundLoadBasisPoints"] = (
        policy["localPreflight"]["backgroundLoadMaxPercent"] * 100 + 1
    )
    candidate["controlObservationCanonicalJson"] = canonical_json(control)
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    control = parse_canonical_json(
        candidate["controlObservationCanonicalJson"], "fixture control"
    )
    control["backgroundLoadLimitBasisPoints"] = 5001
    candidate["controlObservationCanonicalJson"] = canonical_json(control)
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    candidate = copy.deepcopy(first)
    control = parse_canonical_json(
        candidate["controlObservationCanonicalJson"], "fixture control"
    )
    control["referenceHostConformant"] = False
    candidate["controlObservationCanonicalJson"] = canonical_json(control)
    candidate["identitySha256"] = observation_identity(candidate)
    mutations.append(candidate)
    for candidate in mutations:
        try:
            validate_observation(candidate, policy)
        except (TimingObservationError, calibration.TimingCalibrationError, reference_host_profiles.HostProfileError):
            controls += 1
        else:
            raise TimingObservationError("timing observation mutation was accepted")

    competing_failure = copy.deepcopy(first)
    control = parse_canonical_json(
        competing_failure["controlObservationCanonicalJson"], "fixture control"
    )
    control["competingProcessCount"] = 1
    competing_failure["controlObservationCanonicalJson"] = canonical_json(control)
    competing_failure["outcome"] = "hard-failure"
    competing_failure["failureKind"] = "competing-lane"
    competing_failure["exitCode"] = -15
    competing_failure["identitySha256"] = observation_identity(competing_failure)
    validate_observation(competing_failure, policy)
    controls += 1

    with tempfile.TemporaryDirectory() as raw_temp:
        history = Path(raw_temp) / "history.jsonl"
        history.write_text(
            canonical_json(first) + "\n" + canonical_json(second) + "\n",
            encoding="ascii",
        )
        require(len(load_history(history, policy)) == 2, "valid history was not loaded")
        controls += 1
        broken = copy.deepcopy(second)
        broken["previousObservationSha256"] = ZERO_SHA256
        broken["identitySha256"] = observation_identity(broken)
        history.write_text(
            canonical_json(first) + "\n" + canonical_json(broken) + "\n",
            encoding="ascii",
        )
        try:
            load_history(history, policy)
        except TimingObservationError:
            controls += 1
        else:
            raise TimingObservationError("broken timing history chain was accepted")

        if fcntl is not None:
            lock_path = Path(raw_temp) / "collector.run.lock"
            first_lock = acquire_run_lock(lock_path)
            try:
                try:
                    acquire_run_lock(lock_path)
                except TimingObservationError:
                    controls += 1
                else:
                    raise TimingObservationError("concurrent timing collector lock was accepted")
            finally:
                fcntl.flock(first_lock, fcntl.LOCK_UN)
                os.close(first_lock)
            replacement_lock = acquire_run_lock(lock_path)
            fcntl.flock(replacement_lock, fcntl.LOCK_UN)
            os.close(replacement_lock)
            controls += 1

    failure = copy.deepcopy(first)
    failure["outcome"] = "hard-failure"
    failure["failureKind"] = "hard-timeout"
    failure["exitCode"] = None
    failure["identitySha256"] = observation_identity(failure)
    second_after_failure = copy.deepcopy(second)
    second_after_failure["previousObservationSha256"] = failure["identitySha256"]
    second_after_failure["identitySha256"] = observation_identity(second_after_failure)
    review = render_candidate([failure, second_after_failure, hosted], policy)
    require(
        review["observationCount"] == 3
        and len(review["classes"][0]["failedSamples"]) == 1
        and len(review["classes"][1]["warmups"]) == 1,
        "review candidate role assignment drift",
    )
    controls += 1
    try:
        render_candidate([failure, second, hosted], policy)
    except (TimingObservationError, calibration.TimingCalibrationError):
        controls += 1
    else:
        raise TimingObservationError("disconnected candidate observation chain was accepted")
    return controls


def verify_contract(policy: Optional[dict[str, Any]] = None) -> dict[str, Any]:
    policy = policy or calibration.validate_policy(
        calibration.load_json(calibration.POLICY_PATH)
    )
    schema = calibration.load_json(OBSERVATION_SCHEMA_PATH)
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
        and schema.get("$id")
        == "https://genesiscode.dev/schemas/engineering-gate-timing-observation-v0.1.json"
        and schema.get("type") == "object"
        and schema.get("additionalProperties") is False
        and set(schema.get("required", [])) == OBSERVATION_FIELDS,
        "timing observation schema closure drift",
    )
    history = ROOT / policy["observationHistoryPath"]
    records = load_history(history, policy)
    return {"records": len(records), "schemaSha256": calibration.file_sha256(OBSERVATION_SCHEMA_PATH)}


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        choices=(
            "check",
            "self-test",
            "record-local",
            "begin-hosted",
            "record-hosted",
            "render-candidate",
        ),
    )
    parser.add_argument("--class-id")
    parser.add_argument("--history", type=Path)
    parser.add_argument("--input", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--start", type=Path)
    parser.add_argument("--status")
    args = parser.parse_args(argv)
    try:
        policy = calibration.validate_policy(calibration.load_json(calibration.POLICY_PATH))
        if args.command == "check":
            summary = verify_contract(policy)
            print(
                "engineering-gate-timing-observations: ok "
                f"(e0_records={summary['records']} schema={summary['schemaSha256']})"
            )
        elif args.command == "self-test":
            print(
                "engineering-gate-timing-observations: self-test ok "
                f"(controls={self_test()})"
            )
        elif args.command == "record-local":
            require(args.class_id is not None, "record-local requires --class-id")
            history = args.history or ROOT / policy["observationHistoryPath"]
            record = record_local(args.class_id, history)
            if record["outcome"] != "semantic-pass":
                return 1
        elif args.command == "begin-hosted":
            require(args.output is not None, "begin-hosted requires --output")
            begin_hosted(args.output)
        elif args.command == "record-hosted":
            require(
                args.start is not None
                and args.output is not None
                and args.status is not None,
                "record-hosted requires --start, --status, and --output",
            )
            record_hosted(args.start, args.status, args.output)
        else:
            require(args.input and args.output is not None, "render-candidate requires --input and --output")
            records = []
            for path in args.input:
                if path.suffix == ".jsonl":
                    records.extend(load_history(path, policy))
                else:
                    record = calibration.load_json(path)
                    validate_observation(record, policy)
                    records.append(record)
            candidate = render_candidate(records, policy)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(
                json.dumps(candidate, indent=2, sort_keys=True) + "\n",
                encoding="ascii",
            )
            print(
                "engineering-gate-timing-candidate: wrote "
                f"observations={candidate['observationCount']} output={args.output}"
            )
        return 0
    except (
        TimingObservationError,
        calibration.TimingCalibrationError,
        reference_host_profiles.HostProfileError,
        OSError,
        UnicodeError,
        ValueError,
    ) as exc:
        print(f"engineering-gate-timing-observations: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
