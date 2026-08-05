#!/usr/bin/env python3
"""Run and verify paired cold/warm release-full measurements."""

from __future__ import annotations

import argparse
from collections import defaultdict, deque
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any, Mapping, Optional, Sequence

import cargo_cache
import health_profile_evidence as health_evidence


KIND = "genesis/release-full-measurement-v0.1"
VERSION = "0.1.0"
PAIR_KIND = "genesis/release-full-measurement-pair-v0.1"
PAIR_VERSION = "0.1.0"
PROFILE_KIND = "genesis/upgrade-plan-health-profile-v0.1"
TARGET_KIND = "genesis/gcpm-target-runtime-evidence-v0.1"
REFERENCE_KIND = "genesis/release-target-reference-set-v0.1"
WALL_BUDGET_MS = 2_700_000
SESSION_BUDGET_MS = 3_000_000
ARTIFACT_BUDGET_BYTES = 20 * 1024 * 1024 * 1024
DIAGNOSTIC_TAIL_MAX_BYTES = 4096
DIAGNOSTIC_TAIL_MAX_LINES = 40
MIN_PAIRS = 2
MAX_PAIRS = 5
TARGETS = ["android", "edge", "ios", "service-runtime"]
TARGET_RUNNERS = {
    "android": "ubuntu-24.04",
    "edge": "ubuntu-24.04",
    "ios": "macos-15",
    "service-runtime": "ubuntu-24.04",
}
TARGET_PRODUCT_IDS = {
    "android": "TARGET-ANDROID",
    "edge": "TARGET-WEB-INTERACTIVE-SSR-PWA",
    "ios": "TARGET-IOS",
    "service-runtime": "TARGET-SERVICE-DATA",
}
TARGET_CLASS_ENVS = {
    "android": "GENESIS_GCPM_ANDROID_RUNTIME_CLASS",
    "edge": "GENESIS_GCPM_EDGE_RUNTIME_CLASS",
    "ios": "GENESIS_GCPM_IOS_RUNTIME_CLASS",
    "service-runtime": "GENESIS_GCPM_SERVICE_RUNTIME_RUNTIME_CLASS",
}
CARGO_CACHE_ENV = {
    "CARGO_TARGET_DIR",
    "GENESIS_CARGO_CACHE_EPHEMERAL",
    "GENESIS_CARGO_CACHE_HIT",
    "GENESIS_CARGO_CACHE_KEY_SHA256",
    "GENESIS_CARGO_CACHE_RESOLVED",
    "GENESIS_CARGO_CACHE_ROOT",
    "GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON",
    "GENESIS_CARGO_CACHE_SCOPE",
    "GENESIS_GENERATED_STATE_LEASE_PID",
    "GENESIS_GENERATED_STATE_LEASE_TOKEN",
    "GENESIS_GENERATED_STATE_ROOT",
}
CARGO_CACHE_PROVENANCE_ENV = CARGO_CACHE_ENV - {
    "GENESIS_CARGO_CACHE_ROOT",
    "GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON",
}
RUN_FIELDS = {
    "agentGpuProfile",
    "artifactAttributionBytes",
    "artifactPeakBytes",
    "cacheRootStartedEmpty",
    "class",
    "exitCode",
    "index",
    "logArtifacts",
    "peakRssBytes",
    "profileElapsedMs",
    "profileReportArtifact",
    "profileReportSha256",
    "telemetryElapsedMs",
}
PAIR_WORKER_FIELDS = {
    "cacheIsolationIdentitySha256",
    "executionEnvironmentSha256",
    "githubJob",
    "githubRunAttempt",
    "githubRunId",
    "githubSha",
    "pair",
    "workerContentIdentitySha256",
    "workerManifestArtifact",
    "workerManifestSha256",
}
PAIR_REPORT_FIELDS = {
    "artifacts",
    "budgets",
    "cacheIsolation",
    "cleanupRecovery",
    "contentIdentitySha256",
    "executionEnvironment",
    "generatedAtUtc",
    "github",
    "kind",
    "ok",
    "pair",
    "runs",
    "source",
    "version",
}


class MeasurementError(ValueError):
    pass


def fail(message: str) -> None:
    raise MeasurementError(message)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def identity(value: dict[str, Any]) -> str:
    clone = dict(value)
    clone.pop("contentIdentitySha256", None)
    return sha256_bytes(canonical(clone))


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        fail(f"cannot read JSON {path}: {exc}")
    if not isinstance(value, dict):
        fail(f"JSON root must be an object: {path}")
    return value


def exact_keys(value: Any, expected: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        fail(f"{label} fields mismatch: expected={sorted(expected)!r} observed={observed!r}")
    return value


def percentile95(values: Sequence[int]) -> int:
    if not values:
        fail("p95 requires at least one sample")
    return sorted(values)[max(0, math.ceil(0.95 * len(values)) - 1)]


def allocated_bytes(path: Path) -> int:
    if not path.exists() and not path.is_symlink():
        return 0
    total = 0
    stack = [path]
    seen: set[tuple[int, int]] = set()
    while stack:
        current = stack.pop()
        try:
            stat = current.lstat()
        except OSError:
            continue
        key = (stat.st_dev, stat.st_ino)
        if key in seen:
            continue
        seen.add(key)
        total += int(getattr(stat, "st_blocks", 0)) * 512
        if current.is_dir() and not current.is_symlink():
            try:
                stack.extend(Path(entry.path) for entry in os.scandir(current))
            except OSError:
                pass
    return total


def process_tree(root_pid: int) -> tuple[set[int], dict[int, int]]:
    rows: list[tuple[int, int, int]] = []
    system = platform.system().lower()
    if system == "linux":
        proc = subprocess.run(
            ["ps", "-e", "-o", "pid=,ppid=,rss="],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    elif system == "darwin":
        proc = subprocess.run(
            ["ps", "-axo", "pid=,ppid=,rss="],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    else:
        return {root_pid}, {}
    for line in proc.stdout.splitlines():
        fields = line.split()
        if len(fields) == 3 and all(field.isdigit() for field in fields):
            rows.append(tuple(map(int, fields)))
    children: dict[int, list[int]] = defaultdict(list)
    rss: dict[int, int] = {}
    for pid, ppid, rss_kib in rows:
        children[ppid].append(pid)
        rss[pid] = rss_kib * 1024
    found: set[int] = set()
    queue = deque([root_pid])
    while queue:
        pid = queue.popleft()
        if pid not in found:
            found.add(pid)
            queue.extend(children.get(pid, ()))
    return found, rss


class ResourceSampler:
    def __init__(self, pid: int, roots: dict[str, Path]):
        self.pid = pid
        self.roots = roots
        self.stop = threading.Event()
        self.peak_rss_bytes = 0
        self.peak_artifact_bytes = 0
        self.peak_attribution: dict[str, int] = {name: 0 for name in roots}
        self.error: Optional[str] = None

    def run(self) -> None:
        try:
            next_disk = 0.0
            while not self.stop.is_set():
                pids, rss = process_tree(self.pid)
                self.peak_rss_bytes = max(
                    self.peak_rss_bytes,
                    sum(rss.get(pid, 0) for pid in pids),
                )
                now = time.monotonic()
                if now >= next_disk:
                    sizes = {name: allocated_bytes(path) for name, path in self.roots.items()}
                    total = sum(sizes.values())
                    if total >= self.peak_artifact_bytes:
                        self.peak_artifact_bytes = total
                        self.peak_attribution = sizes
                    next_disk = now + 10.0
                self.stop.wait(0.25)
            sizes = {name: allocated_bytes(path) for name, path in self.roots.items()}
            total = sum(sizes.values())
            if total >= self.peak_artifact_bytes:
                self.peak_artifact_bytes = total
                self.peak_attribution = sizes
        except (OSError, subprocess.SubprocessError) as exc:
            self.error = str(exc)


def safe_relative(path: Path, root: Path) -> str:
    try:
        relative = path.resolve(strict=True).relative_to(root.resolve(strict=True))
    except ValueError:
        fail(f"artifact escapes measurement output: {path}")
    value = PurePosixPath(relative.as_posix())
    if not value.parts or ".." in value.parts:
        fail(f"artifact path is not canonical: {path}")
    return value.as_posix()


def copy_artifact(source: Path, output: Path, relative: str) -> dict[str, Any]:
    destination = output / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    payload = destination.read_bytes()
    return {"bytes": len(payload), "path": relative, "sha256": sha256_bytes(payload)}


def validate_profile_report(
    path: Path,
    expected_class: Optional[str] = None,
    expected_gpu_profile: Optional[str] = None,
) -> dict[str, Any]:
    doc = load_json(path)
    if doc.get("kind") != PROFILE_KIND or doc.get("profile") != "release-full":
        fail("release run emitted the wrong profile report identity")
    if doc.get("ok") is not True:
        fail(f"release profile report is not successful: {doc.get('fail_reasons')!r}")
    elapsed = doc.get("elapsed_ms")
    artifact_bytes = doc.get("artifact_bytes")
    if not isinstance(elapsed, int) or elapsed <= 0:
        fail("release profile elapsed_ms is invalid")
    if not isinstance(artifact_bytes, int) or artifact_bytes < 0:
        fail("release profile artifact_bytes is invalid")
    if artifact_bytes > ARTIFACT_BUDGET_BYTES:
        fail("release profile artifact_bytes exceeds GB-4")
    if expected_class is not None and doc.get("measurement_run_class") != expected_class:
        fail("release profile measurement class mismatch")
    if expected_gpu_profile is not None and doc.get("agent_gpu_profile") != expected_gpu_profile:
        fail("release profile agent GPU identity mismatch")
    return doc


def terminate_group(proc: subprocess.Popen[Any]) -> None:
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        proc.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    proc.wait()


def diagnostic_tail(path: Path, root: Path) -> str:
    """Return a bounded, control-free, repository-relative diagnostic tail."""
    payload = path.read_bytes()[-(DIAGNOSTIC_TAIL_MAX_BYTES * 2) :]
    text = payload.decode("utf-8", errors="replace").replace(str(root), "<repo>")
    lines = []
    for raw in text.splitlines()[-DIAGNOSTIC_TAIL_MAX_LINES:]:
        clean = "".join(char if char == "\t" or 32 <= ord(char) <= 126 else "?" for char in raw)
        lines.append(clean)
    while lines and len("\n".join(lines).encode("utf-8")) > DIAGNOSTIC_TAIL_MAX_BYTES:
        if len(lines) > 1:
            lines.pop(0)
        else:
            encoded = lines[0].encode("utf-8")[-DIAGNOSTIC_TAIL_MAX_BYTES:]
            lines[0] = encoded.decode("utf-8", errors="ignore")
    return "\n".join(lines)


def measurement_environment(
    root: Path,
    cache_root: Path,
    environ: Optional[Mapping[str, str]] = None,
) -> dict[str, str]:
    env = dict(os.environ if environ is None else environ)
    if any(name in env for name in CARGO_CACHE_PROVENANCE_ENV):
        inherited_target = env.get("CARGO_TARGET_DIR", "")
        if not inherited_target:
            fail("inherited Cargo cache provenance is missing CARGO_TARGET_DIR")
        if env.get("GENESIS_CARGO_CACHE_RESOLVED") != "1":
            fail(f"arbitrary inherited CARGO_TARGET_DIR is forbidden: {inherited_target}")
        scope = env.get("GENESIS_CARGO_CACHE_SCOPE", "")
        inherited_key = env.get("GENESIS_CARGO_CACHE_KEY_SHA256", "")
        if not scope or not is_sha256(inherited_key):
            fail("inherited Cargo cache provenance is incomplete")
        try:
            resolved = cargo_cache.resolve(root, scope, env)
        except cargo_cache.CachePolicyError as exc:
            fail(f"cannot validate inherited Cargo cache provenance: {exc}")
        expected_target = Path(resolved["target_dir"]).resolve()
        target = Path(inherited_target)
        if (
            not target.is_absolute()
            or target.resolve() != expected_target
            or resolved["metadata"]["cacheKeySha256"] != inherited_key
        ):
            fail("inherited Cargo cache provenance does not match the canonical resolver")
        metadata_path = expected_target / str(resolved["metadata_file"])
        try:
            metadata = metadata_path.read_bytes()
        except OSError as exc:
            fail(f"cannot validate inherited Cargo cache metadata: {exc}")
        if metadata != cargo_cache.pretty_bytes(resolved["metadata"]):
            fail("inherited Cargo cache metadata does not match the canonical resolver")
    for name in CARGO_CACHE_ENV:
        env.pop(name, None)
    env["GENESIS_CARGO_CACHE_ROOT"] = str(cache_root)
    return env


def run_sample(
    root: Path,
    containment: Path,
    output: Path,
    pair_index: int,
    run_class: str,
    cache_root: Path,
    timeout_ms: int,
) -> dict[str, Any]:
    raw = containment / f"raw-{run_class}"
    raw.mkdir()
    cache_started_empty = not cache_root.exists() or allocated_bytes(cache_root) == 0
    if (run_class == "cold") != cache_started_empty:
        fail(f"{run_class} run cache precondition mismatch for pair {pair_index}")
    stdout_path = containment / f"{run_class}.stdout.log"
    stderr_path = containment / f"{run_class}.stderr.log"
    empty_history = containment / "empty-history.jsonl"
    empty_history.touch(exist_ok=True)
    env = measurement_environment(root, cache_root)
    gpu_profile = env.get("GENESIS_AGENT_GPU_PROFILE", "")
    if gpu_profile not in {"agent-gpu-strict", "agent-gpu-fallback"}:
        fail("release measurement requires an explicit agent GPU profile")
    env.update(
        {
            "CI": "true",
            "GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT": str(containment),
            "GENESIS_CHECK_HEALTH_OUTPUT_ROOT": str(raw),
            "GENESIS_CHECK_HEALTH_RELEASE_FULL_HISTORY_INPUT": str(empty_history),
            "GENESIS_GATE_TELEMETRY_DISABLE": "1",
            "GENESIS_HEALTH_CARGO_GATE_SHARDS": "2",
            "GENESIS_HEALTH_PROFILE": "release-full",
            "GENESIS_HEALTH_PROFILE_GATE_CACHE": "0",
            "GENESIS_HEALTH_WARM_CARGO_CACHE": "0",
            "GENESIS_RELEASE_MEASUREMENT_RUN_CLASS": run_class,
        }
    )
    roots = {
        "isolated-run": containment,
        "node-modules": root / "node_modules",
        "workspace-build": root / ".genesis/build",
        "workspace-target": root / "target",
    }
    started = time.monotonic_ns()
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        proc = subprocess.Popen(
            ["bash", "scripts/check_upgrade_plan_health.sh", "--profile", "release-full"],
            cwd=root,
            env=env,
            stdout=stdout,
            stderr=stderr,
            start_new_session=True,
        )
        sampler = ResourceSampler(proc.pid, roots)
        thread = threading.Thread(target=sampler.run, daemon=True)
        thread.start()
        previous_handlers = {}
        kill_timer: Optional[threading.Timer] = None

        def kill_group() -> None:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

        def forward(signum: int, _frame: Any) -> None:
            nonlocal kill_timer
            try:
                os.killpg(proc.pid, signum)
            except ProcessLookupError:
                return
            kill_timer = threading.Timer(5.0, kill_group)
            kill_timer.daemon = True
            kill_timer.start()

        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            previous_handlers[signum] = signal.signal(signum, forward)
        try:
            exit_code = proc.wait(timeout=timeout_ms / 1000)
        except subprocess.TimeoutExpired:
            terminate_group(proc)
            exit_code = 124
        finally:
            sampler.stop.set()
            thread.join(timeout=30)
            if kill_timer is not None:
                kill_timer.cancel()
            for signum, handler in previous_handlers.items():
                signal.signal(signum, handler)
    if thread.is_alive():
        fail("release resource sampler did not stop within 30 seconds")
    if sampler.error is not None:
        fail(f"release resource sampler failed: {sampler.error}")
    elapsed_ms = max(1, (time.monotonic_ns() - started) // 1_000_000)
    retained_prefix = f"runs/pair-{pair_index:02d}-{run_class}"
    stdout_artifact = copy_artifact(stdout_path, output, f"{retained_prefix}/stdout.log")
    stderr_artifact = copy_artifact(stderr_path, output, f"{retained_prefix}/stderr.log")
    if exit_code != 0:
        tail = diagnostic_tail(stderr_path, root)
        if tail:
            print("release-full-measurement: bounded child stderr tail:", file=sys.stderr)
            for line in tail.splitlines():
                print(f"release-full-measurement: | {line}", file=sys.stderr)
        fail(
            f"release {run_class} run {pair_index} failed with exit {exit_code}; "
            f"timeout_ms={timeout_ms}; see {stderr_artifact['path']}"
        )
    profile_path = raw / "profile-report.json"
    profile = validate_profile_report(profile_path, run_class, gpu_profile)
    retained_report = copy_artifact(
        profile_path,
        output,
        f"{retained_prefix}/profile-report.json",
    )
    for child in sorted(raw.iterdir()):
        if child.is_file() and child.name != "profile-report.json":
            copy_artifact(child, output, f"{retained_prefix}/{child.name}")
    if sampler.peak_rss_bytes <= 0:
        fail("process-tree peak RSS sampling produced no measurement")
    if sampler.peak_artifact_bytes > ARTIFACT_BUDGET_BYTES:
        fail(
            f"release {run_class} run {pair_index} exceeded artifact budget: "
            f"{sampler.peak_artifact_bytes} > {ARTIFACT_BUDGET_BYTES}"
        )
    if elapsed_ms > WALL_BUDGET_MS:
        fail(
            f"release {run_class} run {pair_index} exceeded wall budget: "
            f"{elapsed_ms} > {WALL_BUDGET_MS}"
        )
    return {
        "agentGpuProfile": gpu_profile,
        "artifactAttributionBytes": sampler.peak_attribution,
        "artifactPeakBytes": sampler.peak_artifact_bytes,
        "cacheRootStartedEmpty": cache_started_empty,
        "class": run_class,
        "exitCode": exit_code,
        "index": pair_index,
        "logArtifacts": [stdout_artifact["path"], stderr_artifact["path"]],
        "peakRssBytes": sampler.peak_rss_bytes,
        "profileElapsedMs": int(profile["elapsed_ms"]),
        "profileReportArtifact": retained_report["path"],
        "profileReportSha256": retained_report["sha256"],
        "telemetryElapsedMs": elapsed_ms,
    }


def reference_set(root: Path) -> tuple[dict[str, Any], str]:
    path = root / "policies/release_target_reference_set_v0.1.json"
    payload = path.read_bytes()
    doc = json.loads(payload)
    if doc.get("kind") != REFERENCE_KIND:
        fail("release target reference set identity mismatch")
    return doc, sha256_bytes(payload)


def is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(char in "0123456789abcdef" for char in value)
    )


def validate_target_report(
    root: Path,
    path: Path,
    policy: dict[str, Any],
    policy_sha: str,
    expected_commit: str,
) -> tuple[str, dict[str, Any]]:
    doc = exact_keys(
        load_json(path),
        {
            "ci_context", "errors", "expected_outcome", "generated_at_utc", "kind",
            "ok", "product_matrix", "qualification_statuses", "reference_set",
            "release_qualified", "require_non_synthetic", "require_reference_setup",
            "runner_evidence", "targets",
        },
        "named target report",
    )
    targets = doc["targets"]
    if not isinstance(targets, list) or len(targets) != 1:
        fail(f"named target report must contain exactly one target: {path}")
    target_row = exact_keys(
        targets[0],
        {
            "bundle_hash", "bundle_root", "checks", "errors", "ok", "product_id",
            "qualification", "reference_shard", "replay_artifacts", "runtime_evidence",
            "target",
        },
        "named target record",
    )
    target = target_row["target"]
    shards = {row["target"]: row for row in policy["shards"]}
    if target not in shards:
        fail(f"named target report has unknown target: {target!r}")
    shard = shards[target]
    runner = exact_keys(
        doc["runner_evidence"],
        {
            "architecture", "ci", "github_run_attempt", "github_run_id", "github_sha",
            "label", "matches_reference", "operating_system",
        },
        "target runner evidence",
    )
    expected_runner = shard["runner"]
    expected_os = "darwin" if expected_runner.startswith("macos-") else "linux"
    if (
        doc["kind"] != TARGET_KIND
        or doc["ok"] is not True
        or doc["errors"] != []
        or doc["release_qualified"] is not False
        or doc["require_non_synthetic"] is not True
        or doc["require_reference_setup"] is not True
        or doc["ci_context"] is not True
        or doc["expected_outcome"] != "unsupported-product"
        or doc["qualification_statuses"] != ["unsupported-product"]
        or doc["product_matrix"] != "docs/spec/PRODUCT_TARGET_MATRIX_v0.1.json"
        or doc["reference_set"]
        != {"path": "policies/release_target_reference_set_v0.1.json", "sha256": policy_sha}
        or runner["label"] != expected_runner
        or runner["matches_reference"] is not True
        or runner["ci"] is not True
        or runner["operating_system"] != expected_os
        or not isinstance(runner["architecture"], str)
        or not runner["architecture"]
        or not isinstance(runner["github_run_id"], str)
        or not runner["github_run_id"].isdigit()
        or runner["github_run_id"].startswith("0")
        or not isinstance(runner["github_run_attempt"], str)
        or not runner["github_run_attempt"].isdigit()
        or runner["github_run_attempt"].startswith("0")
        or runner["github_sha"] != expected_commit
    ):
        fail(f"named target report is not authentic reference readiness evidence: {target}")
    try:
        generated_at = dt.datetime.fromisoformat(doc["generated_at_utc"])
    except (TypeError, ValueError):
        fail(f"named target report has invalid generation time: {target}")
    if generated_at.tzinfo is None:
        fail(f"named target report generation time lacks a timezone: {target}")

    product_matrix = load_json(root / "docs/spec/PRODUCT_TARGET_MATRIX_v0.1.json")
    products = {row["id"]: row for row in product_matrix.get("entries", [])}
    product_id = TARGET_PRODUCT_IDS[target]
    product = products.get(product_id)
    if not isinstance(product, dict):
        fail(f"named target report references an unknown product: {product_id}")
    expected_qualification = {
        "product_matrix_maturity": product.get("maturity"),
        "product_matrix_release_eligible": product.get("release_eligible"),
        "product_matrix_release_status": product.get("release_status"),
        "reason": product.get("limitations"),
        "status": "unsupported-product",
    }
    if (
        target_row["ok"] is not True
        or target_row["errors"] != []
        or target_row["product_id"] != product_id
        or target_row["reference_shard"] != shard
        or target_row["bundle_root"] != target
        or target_row["qualification"] != expected_qualification
        or not is_sha256(target_row["bundle_hash"])
    ):
        fail(f"named target report policy or product binding mismatch: {target}")

    checks = exact_keys(
        target_row["checks"],
        {
            "artifact_signature", "boot_lane", "build_reproducible",
            "launch_adapter_contract", "manifest_pipeline_kind",
            "portable_provenance_paths", "required_artifacts", "smoke_lane",
        },
        "target checks",
    )
    if any(not isinstance(value, dict) or value.get("ok") is not True for value in checks.values()):
        fail(f"named target report contains a failed build or synthetic control: {target}")
    build_check = checks["build_reproducible"]
    signature_check = checks["artifact_signature"]
    if (
        build_check.get("hash_a") != target_row["bundle_hash"]
        or build_check.get("hash_b") != target_row["bundle_hash"]
        or not is_sha256(signature_check.get("actual"))
        or signature_check.get("actual") != signature_check.get("expected")
    ):
        fail(f"named target report build identities disagree: {target}")

    runtime = exact_keys(
        target_row["runtime_evidence"],
        {
            "class", "class_env", "command", "command_env", "command_sha256",
            "exit_code", "lifecycle", "mode", "ok", "replay_artifact_dir",
            "runtime_identity", "runtime_identity_env", "sdk_identity",
            "sdk_identity_env", "stderr_sha256", "stderr_tail", "stdout_sha256",
            "stdout_tail",
        },
        "target runtime evidence",
    )
    if (
        runtime["mode"] != "synthetic-adapter"
        or runtime["class"] != shard["runtimeClass"]
        or runtime["ok"] is not False
        or runtime["exit_code"] != 0
        or runtime["lifecycle"] is not None
        or runtime["replay_artifact_dir"] != target
        or runtime["command_env"] != shard["commandEnv"]
        or runtime["class_env"] != TARGET_CLASS_ENVS[target]
        or runtime["runtime_identity_env"] != shard["identityEnv"]
        or runtime["sdk_identity_env"] != shard["sdkIdentityEnv"]
        or not isinstance(runtime["command"], str)
        or not runtime["command"]
        or runtime["command_sha256"] != sha256_bytes(runtime["command"].encode())
        or not isinstance(runtime["runtime_identity"], str)
        or not runtime["runtime_identity"]
        or not isinstance(runtime["sdk_identity"], str)
        or not runtime["sdk_identity"]
        or not all(is_sha256(runtime[name]) for name in ("command_sha256", "stderr_sha256", "stdout_sha256"))
    ):
        fail(f"unsupported target report contains relabeled runtime evidence: {target}")

    replay = exact_keys(
        target_row["replay_artifacts"],
        {"file_count", "files", "root", "tree_sha256"},
        "target replay artifacts",
    )
    files = replay["files"]
    if (
        replay["root"] != target
        or not is_sha256(replay["tree_sha256"])
        or not isinstance(files, list)
        or replay["file_count"] != len(files)
        or len(files) < 3
    ):
        fail(f"named target replay inventory is incomplete: {target}")
    replay_by_path = {}
    for index, raw in enumerate(files):
        entry = exact_keys(raw, {"path", "sha256", "size_bytes"}, f"target replay artifact {index}")
        relative = PurePosixPath(entry["path"])
        if (
            relative.is_absolute()
            or ".." in relative.parts
            or relative.as_posix() != entry["path"]
            or entry["path"] in replay_by_path
            or not is_sha256(entry["sha256"])
            or not isinstance(entry["size_bytes"], int)
            or entry["size_bytes"] < 0
        ):
            fail(f"named target replay artifact is invalid: {target}:{entry.get('path')!r}")
        replay_by_path[entry["path"]] = entry
    for name, digest in (
        ("runtime_stdout.log", runtime["stdout_sha256"]),
        ("runtime_stderr.log", runtime["stderr_sha256"]),
        ("runtime_command.txt", None),
    ):
        entry = replay_by_path.get(name)
        if entry is None or (digest is not None and entry["sha256"] != digest):
            fail(f"named target report does not bind its runtime log: {target}:{name}")
    return target, runner


def validate_target_reports(root: Path, paths: Sequence[Path]) -> list[dict[str, Any]]:
    policy, policy_sha = reference_set(root)
    expected_commit = health_evidence.source_inventory(root)["gitCommit"]
    shards = {row["target"]: row for row in policy["shards"]}
    if sorted(shards) != TARGETS or len(paths) != len(TARGETS):
        fail("exactly one named target report is required for every reference shard")
    records = []
    seen = set()
    run_ids = set()
    run_attempts = set()
    for path in paths:
        target, runner = validate_target_report(root, path, policy, policy_sha, expected_commit)
        if target in seen:
            fail(f"named target report has duplicate or unknown target: {target!r}")
        seen.add(target)
        expected_runner = shards[target]["runner"]
        run_ids.add(runner["github_run_id"])
        run_attempts.add(runner.get("github_run_attempt"))
        records.append(
            {
                "expectedOutcome": "unsupported-product",
                "githubRunAttempt": runner.get("github_run_attempt"),
                "githubRunId": runner["github_run_id"],
                "githubSha": runner["github_sha"],
                "releaseQualified": False,
                "reportArtifact": f"target-readiness/{target}.json",
                "reportSha256": sha256_file(path),
                "runner": expected_runner,
                "target": target,
            }
        )
    if len(run_ids) != 1 or len(run_attempts) != 1 or None in run_attempts:
        fail("named target reports do not share one GitHub workflow run attempt")
    return sorted(records, key=lambda row: row["target"])


def artifact_inventory(output: Path) -> list[dict[str, Any]]:
    rows = []
    for path in sorted(output.rglob("*")):
        if not path.is_file() or path == output / "manifest.json":
            continue
        relative = safe_relative(path, output)
        payload = path.read_bytes()
        rows.append({"bytes": len(payload), "path": relative, "sha256": sha256_bytes(payload)})
    return rows


def github_provenance(
    source: Mapping[str, Any],
    environ: Optional[Mapping[str, str]] = None,
) -> dict[str, str]:
    env = os.environ if environ is None else environ
    values = {
        "job": env.get("GITHUB_JOB", ""),
        "runAttempt": env.get("GITHUB_RUN_ATTEMPT", ""),
        "runId": env.get("GITHUB_RUN_ID", ""),
        "sha": env.get("GITHUB_SHA", ""),
    }
    if values["job"] != "release_full_measurement_pair":
        fail("release pair worker lacks the canonical GitHub job identity")
    for field in ("runAttempt", "runId"):
        value = values[field]
        if not value.isdigit() or value.startswith("0"):
            fail(f"release pair worker GitHub {field} is invalid")
    if values["sha"] != source.get("gitCommit"):
        fail("release pair worker source commit does not match GitHub provenance")
    return values


def cache_isolation_record(
    pair_index: int,
    github: Mapping[str, str],
    nonce_sha256: Optional[str] = None,
) -> dict[str, Any]:
    nonce = nonce_sha256 or sha256_bytes(os.urandom(32))
    if not is_sha256(nonce):
        fail("release pair cache-isolation nonce is invalid")
    core = {
        "githubJob": github["job"],
        "githubRunAttempt": github["runAttempt"],
        "githubRunId": github["runId"],
        "method": "job-unique-external-owned-root",
        "nonceSha256": nonce,
        "pair": pair_index,
        "rootRemoved": True,
        "rootStartedEmpty": True,
    }
    return {**core, "identitySha256": sha256_bytes(canonical(core))}


def validate_pair_runs(runs: Sequence[dict[str, Any]], pair_index: int) -> None:
    if len(runs) != 2:
        fail("release pair worker requires one complete cold/warm pair")
    for row in runs:
        exact_keys(row, RUN_FIELDS, "run")
        run_class = row["class"]
        if run_class not in {"cold", "warm"}:
            fail("run class must be cold or warm")
        expected_empty = run_class == "cold"
        if row["cacheRootStartedEmpty"] is not expected_empty:
            fail(f"{run_class} run cache identity is false")
        for field in ("artifactPeakBytes", "peakRssBytes", "profileElapsedMs", "telemetryElapsedMs"):
            if not isinstance(row[field], int) or isinstance(row[field], bool) or row[field] <= 0:
                fail(f"run {field} must be a positive integer")
        if row["exitCode"] != 0:
            fail("release run did not exit successfully")
        if row["agentGpuProfile"] not in {"agent-gpu-strict", "agent-gpu-fallback"}:
            fail("release run lacks an explicit agent GPU profile")
        if row["index"] != pair_index:
            fail("release run pair index does not match its worker")
        prefix = f"runs/pair-{pair_index:02d}-{run_class}"
        if row["logArtifacts"] != [f"{prefix}/stdout.log", f"{prefix}/stderr.log"]:
            fail("release run log paths do not match its pair identity")
        if row["profileReportArtifact"] != f"{prefix}/profile-report.json":
            fail("release profile report path does not match its pair identity")
        if not is_sha256(row["profileReportSha256"]):
            fail("release profile report lacks a valid content identity")
        attribution = exact_keys(
            row["artifactAttributionBytes"],
            {"isolated-run", "node-modules", "workspace-build", "workspace-target"},
            "run artifact attribution",
        )
        if any(not isinstance(value, int) or value < 0 for value in attribution.values()):
            fail("run artifact attribution must contain non-negative integers")
        if sum(attribution.values()) != row["artifactPeakBytes"]:
            fail("run artifact peak does not equal its per-root attribution")
        if row["profileElapsedMs"] > row["telemetryElapsedMs"]:
            fail("profile elapsed time exceeds parent telemetry")
        logs = row["logArtifacts"]
        if not isinstance(logs, list) or len(logs) != 2 or len(set(logs)) != 2:
            fail("release run must bind distinct stdout and stderr logs")
        if row["artifactPeakBytes"] > ARTIFACT_BUDGET_BYTES:
            fail("release run artifact peak exceeds GB-4")
        if row["telemetryElapsedMs"] > WALL_BUDGET_MS:
            fail("release run duration exceeds GB-4")
    if [(row["index"], row["class"]) for row in runs] != [
        (pair_index, "cold"),
        (pair_index, "warm"),
    ]:
        fail("release pair worker run ordering mismatch")


def validate_run_set(runs: Sequence[dict[str, Any]], pairs: int) -> None:
    if pairs < MIN_PAIRS or pairs > MAX_PAIRS or len(runs) != pairs * 2:
        fail("release measurement requires two to five complete cold/warm pairs")
    for pair_index in range(1, pairs + 1):
        validate_pair_runs(runs[(pair_index - 1) * 2 : pair_index * 2], pair_index)


def validate_cleanup(row: Any, expected_pair: int) -> dict[str, Any]:
    cleanup = exact_keys(
        row,
        {"method", "ok", "pair", "recoveredBytes", "remainingBytes"},
        "cleanup recovery",
    )
    if (
        cleanup["method"] != "owned-ephemeral-root-removal"
        or cleanup["ok"] is not True
        or cleanup["pair"] != expected_pair
        or not isinstance(cleanup["recoveredBytes"], int)
        or isinstance(cleanup["recoveredBytes"], bool)
        or cleanup["recoveredBytes"] <= 0
        or cleanup["remainingBytes"] != 0
    ):
        fail("measurement cleanup recovery is incomplete")
    return cleanup


def validate_cache_isolation(row: Any, expected_pair: int, github: Mapping[str, str]) -> dict[str, Any]:
    isolation = exact_keys(
        row,
        {
            "githubJob", "githubRunAttempt", "githubRunId", "identitySha256", "method",
            "nonceSha256", "pair", "rootRemoved", "rootStartedEmpty",
        },
        "cache isolation",
    )
    core = dict(isolation)
    observed_identity = core.pop("identitySha256", None)
    if (
        isolation["method"] != "job-unique-external-owned-root"
        or isolation["pair"] != expected_pair
        or isolation["rootStartedEmpty"] is not True
        or isolation["rootRemoved"] is not True
        or isolation["githubJob"] != github["job"]
        or isolation["githubRunAttempt"] != github["runAttempt"]
        or isolation["githubRunId"] != github["runId"]
        or not is_sha256(isolation["nonceSha256"])
        or observed_identity != sha256_bytes(canonical(core))
    ):
        fail("release pair cache-isolation identity mismatch")
    return isolation


def validate_pair_report(report: dict[str, Any], expected_pair: Optional[int] = None) -> None:
    exact_keys(report, PAIR_REPORT_FIELDS, "pair measurement report")
    if report["kind"] != PAIR_KIND or report["version"] != PAIR_VERSION or report["ok"] is not True:
        fail("pair measurement report identity or status mismatch")
    pair_index = report["pair"]
    if (
        not isinstance(pair_index, int)
        or isinstance(pair_index, bool)
        or not 1 <= pair_index <= MAX_PAIRS
        or (expected_pair is not None and pair_index != expected_pair)
    ):
        fail("pair measurement index mismatch")
    if report["contentIdentitySha256"] != identity(report):
        fail("pair measurement content identity mismatch")
    try:
        generated_at = dt.datetime.fromisoformat(report["generatedAtUtc"])
    except (TypeError, ValueError):
        fail("pair measurement generation time is invalid")
    if generated_at.tzinfo is None:
        fail("pair measurement generation time lacks a timezone")
    if report["budgets"] != {
        "maxArtifactBytes": ARTIFACT_BUDGET_BYTES,
        "maxPairSessionMs": SESSION_BUDGET_MS,
        "maxWallMs": WALL_BUDGET_MS,
    }:
        fail("pair measurement budget contract mismatch")
    github = exact_keys(report["github"], {"job", "runAttempt", "runId", "sha"}, "pair GitHub provenance")
    if github["job"] != "release_full_measurement_pair":
        fail("pair measurement GitHub job identity mismatch")
    for field in ("runAttempt", "runId"):
        if not isinstance(github[field], str) or not github[field].isdigit() or github[field].startswith("0"):
            fail("pair measurement GitHub run identity mismatch")
    if github["sha"] != report["source"].get("gitCommit"):
        fail("pair measurement source and GitHub revision mismatch")
    validate_pair_runs(report["runs"], pair_index)
    validate_cleanup(report["cleanupRecovery"], pair_index)
    validate_cache_isolation(report["cacheIsolation"], pair_index, github)
def validate_artifacts(
    output: Path,
    entries: Any,
    *,
    require_complete_tree: bool,
    ignored_paths: Sequence[Path] = (),
) -> dict[str, dict[str, Any]]:
    if not isinstance(entries, list):
        fail("measurement artifact inventory must be an array")
    expected_paths = []
    by_path: dict[str, dict[str, Any]] = {}
    for index, entry in enumerate(entries):
        artifact = exact_keys(entry, {"bytes", "path", "sha256"}, f"artifact {index}")
        relative = PurePosixPath(artifact["path"])
        if (
            relative.is_absolute()
            or ".." in relative.parts
            or relative.as_posix() != artifact["path"]
            or artifact["path"] in by_path
        ):
            fail(f"measurement artifact path is not canonical or unique: {artifact['path']!r}")
        path = output / relative
        payload = path.read_bytes()
        if len(payload) != artifact["bytes"] or sha256_bytes(payload) != artifact["sha256"]:
            fail(f"measurement artifact identity mismatch: {artifact['path']}")
        expected_paths.append(artifact["path"])
        by_path[artifact["path"]] = artifact
    if require_complete_tree:
        ignored = {path.resolve() for path in ignored_paths}
        actual_paths = [
            safe_relative(path, output)
            for path in sorted(output.rglob("*"))
            if path.is_file() and path.resolve() not in ignored
        ]
        if expected_paths != actual_paths:
            fail("measurement artifact inventory is incomplete")
    return by_path


def verify_pair(root: Path, output: Path, expected_pair: Optional[int] = None) -> dict[str, Any]:
    report_path = output / "manifest.json"
    report = load_json(report_path)
    validate_pair_report(report, expected_pair)
    if report["source"] != health_evidence.source_inventory(root):
        fail("pair measurement source snapshot does not match the checkout")
    if report["executionEnvironment"] != health_evidence.execution_environment("release-full"):
        fail("pair measurement execution environment does not match the verifier")
    by_path = validate_artifacts(
        output,
        report["artifacts"],
        require_complete_tree=True,
        ignored_paths=(report_path,),
    )
    for run in report["runs"]:
        entry = by_path.get(run["profileReportArtifact"])
        if entry is None or entry["sha256"] != run["profileReportSha256"]:
            fail("pair run profile report is not bound to the artifact inventory")
        profile = validate_profile_report(
            output / run["profileReportArtifact"],
            run["class"],
            run["agentGpuProfile"],
        )
        if profile["elapsed_ms"] != run["profileElapsedMs"]:
            fail("pair run profile elapsed time disagrees with its retained report")
        if any(path not in by_path for path in run["logArtifacts"]):
            fail("pair run logs are not bound to the artifact inventory")
    return report


def build_report(
    root: Path,
    output: Path,
    runs: list[dict[str, Any]],
    pairs: int,
    cleanups: list[dict[str, Any]],
    targets: list[dict[str, Any]],
    pair_workers: list[dict[str, Any]],
) -> dict[str, Any]:
    validate_run_set(runs, pairs)
    if len(cleanups) != pairs or any(row.get("ok") is not True for row in cleanups):
        fail("every cold/warm pair must prove complete isolated-cache recovery")
    if [row["target"] for row in targets] != TARGETS:
        fail("target readiness evidence is incomplete")
    source = health_evidence.source_inventory(root)
    execution = health_evidence.execution_environment("release-full")
    cold = [row for row in runs if row["class"] == "cold"]
    warm = [row for row in runs if row["class"] == "warm"]
    report = {
        "artifacts": artifact_inventory(output),
        "budgets": {
            "maxArtifactBytes": ARTIFACT_BUDGET_BYTES,
            "maxWallMs": WALL_BUDGET_MS,
            "minimumPairs": MIN_PAIRS,
        },
        "cleanupRecovery": cleanups,
        "contentIdentitySha256": "",
        "executionEnvironment": execution,
        "generatedAtUtc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "history": {
            "coldP95ArtifactBytes": percentile95([row["artifactPeakBytes"] for row in cold]),
            "coldP95PeakRssBytes": percentile95([row["peakRssBytes"] for row in cold]),
            "coldP95WallMs": percentile95([row["telemetryElapsedMs"] for row in cold]),
            "samplesPerClass": pairs,
            "warmP95ArtifactBytes": percentile95([row["artifactPeakBytes"] for row in warm]),
            "warmP95PeakRssBytes": percentile95([row["peakRssBytes"] for row in warm]),
            "warmP95WallMs": percentile95([row["telemetryElapsedMs"] for row in warm]),
        },
        "kind": KIND,
        "ok": True,
        "pairWorkers": pair_workers,
        "pairs": pairs,
        "productReleaseQualified": False,
        "profileOperational": True,
        "readinessStatus": "unsupported-product",
        "runs": runs,
        "source": source,
        "targetReadiness": targets,
        "version": VERSION,
    }
    report["contentIdentitySha256"] = identity(report)
    return report


def validate_report(report: dict[str, Any]) -> None:
    exact_keys(
        report,
        {
            "artifacts", "budgets", "cleanupRecovery", "contentIdentitySha256",
            "executionEnvironment", "generatedAtUtc", "history", "kind", "ok",
            "pairs", "pairWorkers", "productReleaseQualified", "profileOperational",
            "readinessStatus", "runs", "source", "targetReadiness", "version",
        },
        "measurement report",
    )
    if report["kind"] != KIND or report["version"] != VERSION or report["ok"] is not True:
        fail("measurement report identity or status mismatch")
    if (
        report["profileOperational"] is not True
        or report["productReleaseQualified"] is not False
        or report["readinessStatus"] != "unsupported-product"
    ):
        fail("measurement confused profile operation with product qualification")
    if report["contentIdentitySha256"] != identity(report):
        fail("measurement report content identity mismatch")
    try:
        generated_at = dt.datetime.fromisoformat(report["generatedAtUtc"])
    except (TypeError, ValueError):
        fail("measurement report generation time is invalid")
    if generated_at.tzinfo is None:
        fail("measurement report generation time lacks a timezone")
    if report["budgets"] != {
        "maxArtifactBytes": ARTIFACT_BUDGET_BYTES,
        "maxWallMs": WALL_BUDGET_MS,
        "minimumPairs": MIN_PAIRS,
    }:
        fail("measurement budget contract mismatch")
    pairs = report["pairs"]
    if not isinstance(pairs, int) or isinstance(pairs, bool):
        fail("measurement pairs must be an integer")
    validate_run_set(report["runs"], pairs)
    cleanups = report["cleanupRecovery"]
    if not isinstance(cleanups, list) or len(cleanups) != pairs:
        fail("measurement cleanup recovery is incomplete")
    for expected_pair, row in enumerate(cleanups, 1):
        validate_cleanup(row, expected_pair)
    workers = report["pairWorkers"]
    if not isinstance(workers, list) or len(workers) != pairs:
        fail("measurement pair worker set is incomplete")
    expected_execution_sha = sha256_bytes(canonical(report["executionEnvironment"]))
    for expected_pair, row in enumerate(workers, 1):
        worker = exact_keys(row, PAIR_WORKER_FIELDS, "pair worker")
        expected_manifest = f"workers/pair-{expected_pair:02d}.json"
        if (
            worker["pair"] != expected_pair
            or worker["githubJob"] != "release_full_measurement_pair"
            or worker["githubSha"] != report["source"].get("gitCommit")
            or worker["workerManifestArtifact"] != expected_manifest
            or not all(
                is_sha256(worker[field])
                for field in (
                    "cacheIsolationIdentitySha256", "executionEnvironmentSha256",
                    "workerContentIdentitySha256", "workerManifestSha256",
                )
            )
            or worker["executionEnvironmentSha256"] != expected_execution_sha
        ):
            fail("measurement pair worker identity mismatch")
        for field in ("githubRunAttempt", "githubRunId"):
            if not isinstance(worker[field], str) or not worker[field].isdigit() or worker[field].startswith("0"):
                fail("measurement pair worker workflow identity mismatch")
    if len({row["cacheIsolationIdentitySha256"] for row in workers}) != pairs:
        fail("measurement pair workers reused a cache-isolation identity")
    if len({row["githubRunId"] for row in workers}) != 1 or len(
        {row["githubRunAttempt"] for row in workers}
    ) != 1:
        fail("measurement pair workers do not share one workflow run attempt")
    target_readiness = report["targetReadiness"]
    if not isinstance(target_readiness, list) or any(not isinstance(row, dict) for row in target_readiness):
        fail("measurement target readiness set must be an array of objects")
    if [row.get("target") for row in target_readiness] != TARGETS:
        fail("measurement target readiness set mismatch")
    for row in target_readiness:
        exact_keys(
            row,
            {
                "expectedOutcome", "githubRunAttempt", "githubRunId", "githubSha",
                "releaseQualified", "reportArtifact", "reportSha256", "runner", "target",
            },
            "target readiness",
        )
        if row["expectedOutcome"] != "unsupported-product" or row["releaseQualified"] is not False:
            fail("expected unsupported-product was relabeled as release qualification")
        if row["target"] not in TARGET_RUNNERS or row["runner"] != TARGET_RUNNERS[row["target"]]:
            fail("target readiness runner identity mismatch")
        if not is_sha256(row["reportSha256"]):
            fail("target readiness report identity is invalid")
        if (
            not isinstance(row["githubRunId"], str)
            or not row["githubRunId"].isdigit()
            or row["githubRunId"].startswith("0")
            or not isinstance(row["githubRunAttempt"], str)
            or not row["githubRunAttempt"].isdigit()
            or row["githubRunAttempt"].startswith("0")
        ):
            fail("target readiness workflow identity is invalid")
        if row["githubSha"] != report["source"].get("gitCommit"):
            fail("target readiness source commit mismatch")
    if len({row["githubRunId"] for row in target_readiness}) != 1 or len(
        {row["githubRunAttempt"] for row in target_readiness}
    ) != 1:
        fail("target readiness workflow provenance mismatch")
    expected_history = {
        "coldP95ArtifactBytes": percentile95([row["artifactPeakBytes"] for row in report["runs"] if row["class"] == "cold"]),
        "coldP95PeakRssBytes": percentile95([row["peakRssBytes"] for row in report["runs"] if row["class"] == "cold"]),
        "coldP95WallMs": percentile95([row["telemetryElapsedMs"] for row in report["runs"] if row["class"] == "cold"]),
        "samplesPerClass": pairs,
        "warmP95ArtifactBytes": percentile95([row["artifactPeakBytes"] for row in report["runs"] if row["class"] == "warm"]),
        "warmP95PeakRssBytes": percentile95([row["peakRssBytes"] for row in report["runs"] if row["class"] == "warm"]),
        "warmP95WallMs": percentile95([row["telemetryElapsedMs"] for row in report["runs"] if row["class"] == "warm"]),
    }
    if report["history"] != expected_history:
        fail("measurement p95 history is not derived from the retained runs")


def verify(root: Path, output: Path) -> None:
    report_path = output / "manifest.json"
    report = load_json(report_path)
    validate_report(report)
    if report["source"] != health_evidence.source_inventory(root):
        fail("measurement source snapshot does not match the checkout")
    if report["executionEnvironment"] != health_evidence.execution_environment("release-full"):
        fail("measurement execution environment does not match the verifier")
    by_path = validate_artifacts(
        output,
        report["artifacts"],
        require_complete_tree=True,
        ignored_paths=(report_path,),
    )
    for run in report["runs"]:
        entry = by_path.get(run["profileReportArtifact"])
        if entry is None or entry["sha256"] != run["profileReportSha256"]:
            fail("run profile report is not bound to the artifact inventory")
        profile = validate_profile_report(
            output / run["profileReportArtifact"],
            run["class"],
            run["agentGpuProfile"],
        )
        if profile["elapsed_ms"] != run["profileElapsedMs"]:
            fail("run profile elapsed time disagrees with its retained report")
        if any(path not in by_path for path in run["logArtifacts"]):
            fail("run logs are not bound to the artifact inventory")
    for target in report["targetReadiness"]:
        entry = by_path.get(target["reportArtifact"])
        if entry is None or entry["sha256"] != target["reportSha256"]:
            fail("target readiness report is not bound to the artifact inventory")
    target_paths = [output / row["reportArtifact"] for row in report["targetReadiness"]]
    if validate_target_reports(root, target_paths) != report["targetReadiness"]:
        fail("target readiness records disagree with retained reports")
    for worker in report["pairWorkers"]:
        manifest_entry = by_path.get(worker["workerManifestArtifact"])
        if manifest_entry is None or manifest_entry["sha256"] != worker["workerManifestSha256"]:
            fail("pair worker manifest is not bound to the artifact inventory")
        pair_report = load_json(output / worker["workerManifestArtifact"])
        validate_pair_report(pair_report, worker["pair"])
        if (
            pair_report["contentIdentitySha256"] != worker["workerContentIdentitySha256"]
            or pair_report["cacheIsolation"]["identitySha256"]
            != worker["cacheIsolationIdentitySha256"]
            or sha256_bytes(canonical(pair_report["executionEnvironment"]))
            != worker["executionEnvironmentSha256"]
            or pair_report["source"] != report["source"]
            or pair_report["executionEnvironment"] != report["executionEnvironment"]
            or pair_report["runs"]
            != report["runs"][(worker["pair"] - 1) * 2 : worker["pair"] * 2]
            or pair_report["cleanupRecovery"] != report["cleanupRecovery"][worker["pair"] - 1]
        ):
            fail("pair worker evidence disagrees with the aggregate")
        pair_artifacts = validate_artifacts(
            output,
            pair_report["artifacts"],
            require_complete_tree=False,
        )
        if any(path not in by_path or by_path[path] != entry for path, entry in pair_artifacts.items()):
            fail("pair worker artifact inventory disagrees with the aggregate")
    target_run_id = report["targetReadiness"][0]["githubRunId"]
    target_run_attempt = report["targetReadiness"][0]["githubRunAttempt"]
    if any(
        worker["githubRunId"] != target_run_id
        or worker["githubRunAttempt"] != target_run_attempt
        for worker in report["pairWorkers"]
    ):
        fail("pair workers and target readiness do not share one workflow run attempt")
    print(
        "release-full-measurement: verified "
        f"pairs={report['pairs']} identity={report['contentIdentitySha256']}"
    )


def run_pair(
    root: Path,
    output: Path,
    pair_index: int,
) -> None:
    root = root.resolve(strict=True)
    output = output.resolve()
    if output.exists() and any(output.iterdir()):
        fail("pair measurement output must start absent or empty")
    output.mkdir(parents=True, exist_ok=True)
    source_before = health_evidence.source_inventory(root)
    execution = health_evidence.execution_environment("release-full")
    github = github_provenance(source_before)
    session_started = time.monotonic_ns()
    runs: list[dict[str, Any]] = []
    cache_nonce = sha256_bytes(os.urandom(32))
    containment = Path(tempfile.gettempdir()) / (
        f"genesis-release-pair-{pair_index:02d}-{cache_nonce}"
    )
    containment.mkdir(mode=0o700, parents=False, exist_ok=False)
    cache_root = containment / "cargo-cache"
    before = 0
    try:
        for run_class in ("cold", "warm"):
            session_elapsed_ms = (time.monotonic_ns() - session_started) // 1_000_000
            remaining_ms = SESSION_BUDGET_MS - session_elapsed_ms
            if remaining_ms <= 0:
                fail("release pair measurement exceeded its 50-minute execution envelope")
            runs.append(
                run_sample(
                    root,
                    containment,
                    output,
                    pair_index,
                    run_class,
                    cache_root,
                    min(WALL_BUDGET_MS, remaining_ms),
                )
            )
        before = allocated_bytes(containment)
    finally:
        shutil.rmtree(containment, ignore_errors=False)
    remaining = allocated_bytes(containment)
    cleanup = {
        "method": "owned-ephemeral-root-removal",
        "ok": remaining == 0,
        "pair": pair_index,
        "recoveredBytes": before,
        "remainingBytes": remaining,
    }
    if source_before != health_evidence.source_inventory(root):
        fail("semantic source changed during release pair measurement")
    report = {
        "artifacts": artifact_inventory(output),
        "budgets": {
            "maxArtifactBytes": ARTIFACT_BUDGET_BYTES,
            "maxPairSessionMs": SESSION_BUDGET_MS,
            "maxWallMs": WALL_BUDGET_MS,
        },
        "cacheIsolation": cache_isolation_record(pair_index, github, cache_nonce),
        "cleanupRecovery": cleanup,
        "contentIdentitySha256": "",
        "executionEnvironment": execution,
        "generatedAtUtc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "github": github,
        "kind": PAIR_KIND,
        "ok": True,
        "pair": pair_index,
        "runs": runs,
        "source": source_before,
        "version": PAIR_VERSION,
    }
    report["contentIdentitySha256"] = identity(report)
    (output / "manifest.json").write_bytes(json.dumps(report, indent=2, sort_keys=True).encode() + b"\n")
    verify_pair(root, output, pair_index)
    print(
        "release-full-measurement: pair verified "
        f"pair={pair_index} identity={report['contentIdentitySha256']}"
    )


def aggregate(
    root: Path,
    output: Path,
    target_paths: Sequence[Path],
    pair_outputs: Sequence[Path],
) -> None:
    root = root.resolve(strict=True)
    output = output.resolve()
    if output.exists() and any(output.iterdir()):
        fail("measurement aggregate output must start absent or empty")
    if not MIN_PAIRS <= len(pair_outputs) <= MAX_PAIRS:
        fail("measurement aggregate requires two to five pair workers")
    output.mkdir(parents=True, exist_ok=True)
    targets = validate_target_reports(root, target_paths)
    target_dir = output / "target-readiness"
    target_dir.mkdir()
    for path in target_paths:
        doc = load_json(path)
        target = doc["targets"][0]["target"]
        copy_artifact(path, output, f"target-readiness/{target}.json")
    pair_reports: list[tuple[Path, dict[str, Any]]] = []
    for raw in pair_outputs:
        pair_output = raw.resolve(strict=True)
        report = verify_pair(root, pair_output)
        pair_reports.append((pair_output, report))
    pair_reports.sort(key=lambda item: item[1]["pair"])
    expected_pairs = list(range(1, len(pair_reports) + 1))
    if [report["pair"] for _, report in pair_reports] != expected_pairs:
        fail("measurement aggregate pair coverage is incomplete or duplicated")
    source = health_evidence.source_inventory(root)
    execution = health_evidence.execution_environment("release-full")
    runs: list[dict[str, Any]] = []
    cleanups: list[dict[str, Any]] = []
    pair_workers: list[dict[str, Any]] = []
    for pair_output, report in pair_reports:
        pair_index = report["pair"]
        if (
            report["source"] != source
            or report["executionEnvironment"] != execution
        ):
            fail("measurement pair worker source or environment mismatch")
        for artifact in report["artifacts"]:
            relative = artifact["path"]
            if (output / relative).exists():
                fail(f"measurement pair workers produced a duplicate artifact: {relative}")
            copied = copy_artifact(pair_output / relative, output, relative)
            if copied != artifact:
                fail("measurement pair worker artifact changed during aggregation")
        worker_relative = f"workers/pair-{pair_index:02d}.json"
        worker_artifact = copy_artifact(pair_output / "manifest.json", output, worker_relative)
        github = report["github"]
        pair_workers.append(
            {
                "cacheIsolationIdentitySha256": report["cacheIsolation"]["identitySha256"],
                "executionEnvironmentSha256": sha256_bytes(canonical(report["executionEnvironment"])),
                "githubJob": github["job"],
                "githubRunAttempt": github["runAttempt"],
                "githubRunId": github["runId"],
                "githubSha": github["sha"],
                "pair": pair_index,
                "workerContentIdentitySha256": report["contentIdentitySha256"],
                "workerManifestArtifact": worker_relative,
                "workerManifestSha256": worker_artifact["sha256"],
            }
        )
        runs.extend(report["runs"])
        cleanups.append(report["cleanupRecovery"])
    report = build_report(
        root,
        output,
        runs,
        len(pair_reports),
        cleanups,
        targets,
        pair_workers,
    )
    (output / "manifest.json").write_bytes(json.dumps(report, indent=2, sort_keys=True).encode() + b"\n")
    verify(root, output)


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    pair_parser = sub.add_parser("run-pair")
    pair_parser.add_argument("--root", type=Path, required=True)
    pair_parser.add_argument("--output", type=Path, required=True)
    pair_parser.add_argument("--pair-index", type=int, required=True)
    aggregate_parser = sub.add_parser("aggregate")
    aggregate_parser.add_argument("--root", type=Path, required=True)
    aggregate_parser.add_argument("--output", type=Path, required=True)
    aggregate_parser.add_argument("--pair-output", type=Path, action="append", required=True)
    aggregate_parser.add_argument("--target-report", type=Path, action="append", required=True)
    pair_verify_parser = sub.add_parser("verify-pair")
    pair_verify_parser.add_argument("--root", type=Path, required=True)
    pair_verify_parser.add_argument("--output", type=Path, required=True)
    pair_verify_parser.add_argument("--pair-index", type=int)
    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("--root", type=Path, required=True)
    verify_parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "run-pair":
            if not 1 <= args.pair_index <= MAX_PAIRS:
                fail(f"--pair-index must be between 1 and {MAX_PAIRS}")
            run_pair(args.root, args.output, args.pair_index)
        elif args.command == "aggregate":
            aggregate(args.root, args.output, args.target_report, args.pair_output)
        elif args.command == "verify-pair":
            verify_pair(args.root.resolve(strict=True), args.output.resolve(strict=True), args.pair_index)
        else:
            verify(args.root.resolve(strict=True), args.output.resolve(strict=True))
    except (MeasurementError, OSError, subprocess.SubprocessError) as exc:
        print(f"release-full-measurement: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
