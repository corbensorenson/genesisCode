#!/usr/bin/env python3
"""Run one governed gate and emit closed resource telemetry."""

from __future__ import annotations

import argparse
from collections import defaultdict, deque
import json
import os
from pathlib import Path, PurePosixPath
import platform
import signal
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any, Sequence

POLICY_REL = "policies/gate_telemetry_v0.1.json"
MANIFEST_REL = "genesis.gates.json"
POLICY_FIELDS = {
    "aggregateSampleIntervalMs",
    "diskRoots",
    "exactDiskEntrypoints",
    "eventKinds",
    "kind",
    "maxEventCount",
    "maxEventLineBytes",
    "sampleIntervalMs",
    "version",
}


class TelemetryError(ValueError):
    pass


def unique_pairs(pairs):
    out = {}
    for key, value in pairs:
        if key in out:
            raise TelemetryError(f"duplicate JSON key: {key}")
        out[key] = value
    return out


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs)
    except (OSError, json.JSONDecodeError) as exc:
        raise TelemetryError(f"cannot load {path}: {exc}") from exc


def repo_path(raw: str, field: str) -> str:
    path = PurePosixPath(raw)
    if (
        not raw
        or path.is_absolute()
        or path.as_posix() != raw
        or ".." in path.parts
        or "." in path.parts
        or "\\" in raw
    ):
        raise TelemetryError(f"{field} is not canonical repository-relative: {raw!r}")
    return path.as_posix()


def load_policy(root: Path) -> dict:
    policy = load_json(root / POLICY_REL)
    if not isinstance(policy, dict) or set(policy) != POLICY_FIELDS:
        raise TelemetryError("telemetry policy fields mismatch")
    if policy["kind"] != "genesis/gate-resource-telemetry-policy-v0.1" or policy["version"] != "0.1":
        raise TelemetryError("telemetry policy identity mismatch")
    for field in ("diskRoots", "exactDiskEntrypoints", "eventKinds"):
        values = policy[field]
        if not isinstance(values, list) or not values or values != sorted(set(values)):
            raise TelemetryError(f"{field} must be sorted and unique")
    for value in policy["diskRoots"]:
        repo_path(value, "disk root")
    for value in policy["exactDiskEntrypoints"]:
        repo_path(value, "exact-disk entrypoint")
        if not value.startswith("scripts/check_") or not value.endswith(".sh"):
            raise TelemetryError(f"exact-disk entrypoint is not a governed check: {value}")
    if policy["eventKinds"] != ["cache-hit", "network-attempt"]:
        raise TelemetryError("event kind contract drift")
    for field in (
        "aggregateSampleIntervalMs",
        "maxEventCount",
        "maxEventLineBytes",
        "sampleIntervalMs",
    ):
        if not isinstance(policy[field], int) or isinstance(policy[field], bool) or policy[field] < 1:
            raise TelemetryError(f"{field} must be positive")
    if policy["aggregateSampleIntervalMs"] < policy["sampleIntervalMs"]:
        raise TelemetryError("aggregate sampling interval must not exceed ordinary sampling frequency")
    return policy


def sample_interval_ms(policy: dict, gate: dict) -> int:
    if gate.get("boundaryClass") == "aggregate":
        return int(policy["aggregateSampleIntervalMs"])
    return int(policy["sampleIntervalMs"])


def exact_disk_enabled(policy: dict, entrypoint: str, override: str | None) -> bool:
    if override not in (None, "0", "1"):
        raise TelemetryError("GENESIS_GATE_TELEMETRY_EXACT_DISK must be 0 or 1")
    return override == "1" or entrypoint in policy["exactDiskEntrypoints"]


def logical_size(path: Path) -> int:
    try:
        if path.is_symlink():
            return path.lstat().st_size
        if path.is_file():
            return path.stat().st_size
        if not path.is_dir():
            return 0
    except OSError:
        return 0
    total = 0
    stack = [path]
    seen = set()
    while stack:
        directory = stack.pop()
        try:
            entries = os.scandir(directory)
        except OSError:
            continue
        with entries:
            for entry in entries:
                try:
                    stat = entry.stat(follow_symlinks=False)
                except OSError:
                    continue
                identity = (stat.st_dev, stat.st_ino)
                if identity in seen:
                    continue
                seen.add(identity)
                if entry.is_dir(follow_symlinks=False):
                    stack.append(Path(entry.path))
                else:
                    total += stat.st_size
    return total


def disk_size(root: Path, roots: Sequence[str]) -> int:
    return sum(logical_size(root / rel) for rel in roots)


def filesystem_free_bytes(root: Path) -> int:
    stats = os.statvfs(root)
    return int(stats.f_bavail) * int(stats.f_frsize)


def linux_process_tree(root_pid: int) -> set[int]:
    seen = set()
    queue = deque([root_pid])
    while queue:
        pid = queue.popleft()
        if pid in seen:
            continue
        seen.add(pid)
        children = Path(f"/proc/{pid}/task/{pid}/children")
        try:
            values = children.read_text(encoding="ascii").split()
        except OSError:
            continue
        queue.extend(int(value) for value in values if value.isdigit())
    return seen


class Sampler:
    def __init__(self, pid: int, interval_ms: int):
        self.pid = pid
        self.interval = interval_ms / 1000.0
        self.stop = threading.Event()
        self.peak_rss = 0
        self.io_read_by_pid = defaultdict(int)
        self.io_write_by_pid = defaultdict(int)
        self.platform = platform.system().lower()

    def run(self):
        while not self.stop.is_set():
            self.sample()
            self.stop.wait(self.interval)
        self.sample()

    def sample(self):
        if self.platform == "linux":
            pids = linux_process_tree(self.pid)
            rss_total = 0
            page_size = os.sysconf("SC_PAGE_SIZE")
            for pid in pids:
                try:
                    fields = Path(f"/proc/{pid}/statm").read_text(encoding="ascii").split()
                    rss_total += int(fields[1]) * page_size
                except (OSError, ValueError, IndexError):
                    pass
                try:
                    values = {}
                    for line in Path(f"/proc/{pid}/io").read_text(encoding="ascii").splitlines():
                        key, value = line.split(":", 1)
                        values[key] = int(value.strip())
                    self.io_read_by_pid[pid] = max(self.io_read_by_pid[pid], values.get("read_bytes", 0))
                    self.io_write_by_pid[pid] = max(self.io_write_by_pid[pid], values.get("write_bytes", 0))
                except (OSError, ValueError):
                    pass
            self.peak_rss = max(self.peak_rss, rss_total)
        elif self.platform == "darwin":
            proc = subprocess.run(["ps", "-axo", "pid=,ppid=,rss="], text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
            children = defaultdict(list)
            rss = {}
            for line in proc.stdout.splitlines():
                fields = line.split()
                if len(fields) == 3 and all(value.isdigit() for value in fields):
                    pid, ppid, rss_kib = map(int, fields)
                    children[ppid].append(pid)
                    rss[pid] = rss_kib * 1024
            pids, queue = set(), deque([self.pid])
            while queue:
                pid = queue.popleft()
                if pid not in pids:
                    pids.add(pid)
                    queue.extend(children.get(pid, ()))
            self.peak_rss = max(self.peak_rss, sum(rss.get(pid, 0) for pid in pids))


def metric(value: int, unit: str, method: str, completeness: str) -> dict:
    return {"completeness": completeness, "method": method, "unit": unit, "value": int(value)}


def event_counts(path: Path, policy: dict) -> dict[str, int]:
    counts = {kind: 0 for kind in policy["eventKinds"]}
    if not path.is_file():
        return counts
    try:
        with path.open("r", encoding="utf-8") as lines:
            for index, line in enumerate(lines, 1):
                if index > policy["maxEventCount"]:
                    raise TelemetryError("telemetry event count exceeds policy")
                if len(line.encode("utf-8")) > policy["maxEventLineBytes"]:
                    raise TelemetryError(f"telemetry event line {index} exceeds policy")
                try:
                    event = json.loads(line, object_pairs_hook=unique_pairs)
                except (json.JSONDecodeError, TelemetryError) as exc:
                    raise TelemetryError(f"invalid telemetry event line {index}: {exc}") from exc
                if not isinstance(event, dict) or set(event) != {"count", "kind"}:
                    raise TelemetryError(f"telemetry event fields mismatch at line {index}")
                kind, count = event["kind"], event["count"]
                if kind not in counts or not isinstance(count, int) or isinstance(count, bool) or count < 1:
                    raise TelemetryError(f"invalid telemetry event at line {index}")
                counts[kind] += count
                if counts[kind] > policy["maxEventCount"]:
                    raise TelemetryError(f"telemetry {kind} count exceeds policy")
    except (OSError, UnicodeError) as exc:
        raise TelemetryError(f"cannot read telemetry events: {exc}") from exc
    return counts


def normalize_platform() -> str:
    value = platform.system().lower()
    return value if value in {"darwin", "linux", "windows"} else "unsupported"


def run(root: Path, entrypoint: str, command: Sequence[str], output: Path | None, emit: str) -> int:
    root = root.resolve()
    entrypoint = repo_path(entrypoint, "entrypoint")
    policy = load_policy(root)
    manifest = load_json(root / MANIFEST_REL)
    gates = {gate["entrypoint"]: gate for gate in manifest.get("gates", [])}
    if entrypoint not in gates:
        raise TelemetryError(f"entrypoint is not governed by gate manifest: {entrypoint}")
    gate = gates[entrypoint]
    if not command:
        raise TelemetryError("gate command is empty")
    exact_disk = exact_disk_enabled(
        policy, entrypoint, os.environ.get("GENESIS_GATE_TELEMETRY_EXACT_DISK")
    )
    before_disk = disk_size(root, policy["diskRoots"]) if exact_disk else filesystem_free_bytes(root)
    event_fd, event_name = tempfile.mkstemp(prefix="genesis-gate-events.", suffix=".jsonl")
    os.close(event_fd)
    event_path = Path(event_name)
    env = dict(os.environ)
    env["GENESIS_GATE_TELEMETRY_ACTIVE_ENTRYPOINT"] = entrypoint
    env["GENESIS_GATE_TELEMETRY_EVENT_FILE"] = str(event_path)
    started = time.monotonic_ns()
    try:
        proc = subprocess.Popen(command, cwd=root, env=env, start_new_session=True)
    except OSError:
        event_path.unlink(missing_ok=True)
        raise
    sampler = Sampler(proc.pid, sample_interval_ms(policy, gate))
    thread = threading.Thread(target=sampler.run, daemon=True)
    thread.start()
    previous_handlers = {}

    def forward(signum, _frame):
        try:
            os.killpg(proc.pid, signum)
        except ProcessLookupError:
            pass

    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        previous_handlers[signum] = signal.signal(signum, forward)
    try:
        _, status, usage = os.wait4(proc.pid, 0)
        exit_code = os.waitstatus_to_exitcode(status)
        proc.returncode = exit_code
    finally:
        sampler.stop.set()
        thread.join(timeout=2)
        for signum, handler in previous_handlers.items():
            signal.signal(signum, handler)
    duration = time.monotonic_ns() - started
    after_disk = disk_size(root, policy["diskRoots"]) if exact_disk else filesystem_free_bytes(root)
    try:
        events = event_counts(event_path, policy)
    finally:
        event_path.unlink(missing_ok=True)
    rusage_rss = int(usage.ru_maxrss if normalize_platform() == "darwin" else usage.ru_maxrss * 1024)
    peak_rss = max(sampler.peak_rss, rusage_rss)
    if normalize_platform() == "linux":
        io_read = sum(sampler.io_read_by_pid.values())
        io_write = sum(sampler.io_write_by_pid.values())
        io_method, io_quality = "procfs-process-tree-io-sampling", "sampled"
    else:
        io_read = int(usage.ru_inblock) * 512
        io_write = int(usage.ru_oublock) * 512
        io_method, io_quality = "rusage-block-operations-x512", "estimated"
    effective_exit_code = exit_code if exit_code >= 0 else 128 + (-exit_code)
    budget_violations = []
    if effective_exit_code == 0 and os.environ.get("GENESIS_GATE_BUDGET_ENFORCE", "1") != "0":
        if duration > int(gate["expectedDurationSeconds"]) * 1_000_000_000:
            budget_violations.append(
                f"duration={duration}ns>{gate['expectedDurationSeconds']}s"
            )
        generated_delta = after_disk - before_disk if exact_disk else before_disk - after_disk
        disk_budget = int(gate["diskBudgetMiB"]) * 1024 * 1024
        if generated_delta > disk_budget:
            budget_violations.append(
                f"generated-disk={generated_delta}B>{disk_budget}B"
            )
        if gate["network"]["mode"] == "deny" and events["network-attempt"] > 0:
            budget_violations.append(
                f"network-attempts={events['network-attempt']}>0"
            )
        if budget_violations:
            effective_exit_code = 3
            print(
                "gate-telemetry: resource budget exceeded for "
                f"{entrypoint}: {', '.join(budget_violations)}",
                file=sys.stderr,
            )
    result = {
        "gate": {"entrypoint": entrypoint, "executionIdentitySha256": gate["executionIdentitySha256"], "id": gate["id"]},
        "kind": "genesis/gate-resource-telemetry-v0.1",
        "metrics": {
            "cacheHits": metric(events["cache-hit"], "count", "explicit-event-channel", "instrumented"),
            "durationNs": metric(duration, "nanoseconds", "monotonic-parent-clock", "exact"),
            "generatedDiskDeltaBytes": metric(
                after_disk - before_disk if exact_disk else before_disk - after_disk,
                "bytes",
                "logical-size-declared-generated-roots" if exact_disk else "filesystem-free-space-allocation-delta",
                "exact" if exact_disk else "sampled",
            ),
            "ioReadBytes": metric(io_read, "bytes", io_method, io_quality),
            "ioWriteBytes": metric(io_write, "bytes", io_method, io_quality),
            "networkAttempts": metric(events["network-attempt"], "count", "explicit-event-channel", "instrumented"),
            "peakRssBytes": metric(peak_rss, "bytes", "process-tree-sampling-plus-rusage", "sampled"),
        },
        "platform": normalize_platform(),
        "result": {
            "exitCode": effective_exit_code,
            "status": "failed" if budget_violations else ("passed" if exit_code == 0 else ("signaled" if exit_code < 0 else "failed")),
        },
        "version": "0.1",
    }
    rendered = json.dumps(result, sort_keys=True, separators=(",", ":"))
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if emit == "stdout":
        print(f"gate-telemetry: {rendered}")
    elif emit == "stderr":
        print(f"gate-telemetry: {rendered}", file=sys.stderr)
    return effective_exit_code


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--entrypoint", required=True)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--emit", choices=("stdout", "stderr", "none"), default="stderr")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    try:
        return run(args.root, args.entrypoint, command, args.out, args.emit)
    except (TelemetryError, OSError) as exc:
        print(f"gate-telemetry: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
