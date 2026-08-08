#!/usr/bin/env python3
"""Exercise host-bridge process cleanup through the public warm daemon."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import queue
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any

import host_handle_lifecycle_evidence as lifecycle_evidence


PROTOCOL = "genesis/warm-protocol-v0.2"
RESPONSE = "genesis/warm-response-v0.2"
REPORT_KIND = lifecycle_evidence.MACOS_REPORT_KIND
PROCESS_EXIT_TIMEOUT_SECONDS = 8.0
RESPONSE_TIMEOUT_SECONDS = 20.0


class ProbeError(RuntimeError):
    pass


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(64 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_executable(path: pathlib.Path, source: str) -> None:
    path.write_text(source, encoding="utf-8")
    path.chmod(0o755)


def bridge_source(mode: str) -> str:
    response = '{:ok true :status "daemon-probe-ok"}'
    behavior = {
        "success": f"printf '%s\\n%s' \"${{#response}}\" \"$response\"",
        "malformed": "printf 'not-a-length\\n'",
        "timeout": "sleep 300",
    }[mode]
    return f"""#!/bin/sh
set -eu
: "${{GENESIS_DAEMON_PROBE_LOG:?}}"
sleep 300 &
descendant=$!
printf '%s %s %s\\n' "$$" "$descendant" "${{GENESIS_HOST_BRIDGE_TRANSPORT:-unknown}}" >> "$GENESIS_DAEMON_PROBE_LOG"
response='{response}'
while IFS= read -r request_length; do
  case "$request_length" in
    ''|*[!0-9]*) exit 41 ;;
  esac
  dd bs=1 count="$request_length" status=none >/dev/null 2>/dev/null || exit 42
  {behavior}
done
"""


def write_fixture(workspace: pathlib.Path) -> dict[str, pathlib.Path]:
    log_path = workspace / "provider-processes.log"
    program = workspace / "provider.gc"
    program.write_text(
        '(def prog (((core/plugin::command "daemon-probe") "run") {:input "probe"}))\nprog\n',
        encoding="utf-8",
    )
    policies: dict[str, pathlib.Path] = {}
    for mode, timeout_ms in (("success", 5000), ("malformed", 5000), ("timeout", 1000)):
        bridge = workspace / f"bridge-{mode}.sh"
        write_executable(bridge, bridge_source(mode))
        policy = workspace / f"caps-{mode}.toml"
        policy.write_text(
            "\n".join(
                [
                    'allow = ["host/plugin::command"]',
                    '[op."host/plugin::command"]',
                    'allow_plugins = ["daemon-probe"]',
                    'allow_commands = ["run"]',
                    'base_dir = "."',
                    f'bridge_cmd = "{bridge.name}"',
                    f'bridge_cmd_sha256 = "sha256:{sha256_file(bridge)}"',
                    'bridge_transport = "persistent-stdio"',
                    f"timeout_ms = {timeout_ms}",
                    "max_bytes = 4096",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        policies[mode] = policy
    return {"log": log_path, "program": program, **policies}


def process_rows() -> list[tuple[int, int, str]]:
    proc = subprocess.run(
        ["ps", "-axo", "pid=,pgid=,stat="],
        capture_output=True,
        text=True,
        check=False,
        timeout=5,
    )
    if proc.returncode != 0:
        raise ProbeError("process table probe failed")
    rows = []
    for line in proc.stdout.splitlines():
        fields = line.split(None, 2)
        if len(fields) != 3:
            continue
        try:
            rows.append((int(fields[0]), int(fields[1]), fields[2]))
        except ValueError:
            continue
    return rows


def process_or_group_has_live_member(process_id: int) -> bool:
    return any(
        "Z" not in state and (pid == process_id or group == process_id)
        for pid, group, state in process_rows()
    )


def wait_for_cleanup(records: list[tuple[int, int, str]]) -> int:
    started = time.monotonic()
    deadline = started + PROCESS_EXIT_TIMEOUT_SECONDS
    while True:
        live = [
            process_id
            for provider, descendant, _ in records
            for process_id in (provider, descendant)
            if process_or_group_has_live_member(process_id)
        ]
        if not live:
            return round((time.monotonic() - started) * 1000)
        if time.monotonic() >= deadline:
            raise ProbeError(f"bridge process or process group survived cleanup: {sorted(set(live))}")
        time.sleep(0.025)


def force_cleanup(records: list[tuple[int, int, str]]) -> None:
    for provider, descendant, _ in records:
        for process_id in (descendant, -provider, provider):
            try:
                os.kill(process_id, signal.SIGKILL)
            except (OSError, ValueError):
                pass


def read_records(path: pathlib.Path) -> list[tuple[int, int, str]]:
    if not path.exists():
        return []
    records = []
    for line in path.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if len(fields) != 3:
            raise ProbeError(f"malformed provider process record: {line!r}")
        try:
            provider = int(fields[0])
            descendant = int(fields[1])
        except ValueError as exc:
            raise ProbeError(f"non-numeric provider process record: {line!r}") from exc
        if provider <= 1 or descendant <= 1 or provider == descendant:
            raise ProbeError(f"invalid provider process identities: {line!r}")
        if fields[2] != "persistent-stdio":
            raise ProbeError(f"unexpected provider transport: {fields[2]!r}")
        records.append((provider, descendant, fields[2]))
    return records


class WarmDaemon:
    def __init__(
        self,
        genesis: pathlib.Path,
        artifact: pathlib.Path,
        workspace: pathlib.Path,
        provider_log: pathlib.Path,
    ) -> None:
        environment = os.environ.copy()
        environment["GENESIS_DAEMON_PROBE_LOG"] = str(provider_log)
        self._process = subprocess.Popen(
            [
                str(genesis),
                "--selfhost-artifact",
                str(artifact),
                "warm",
                "--workspace-root",
                ".",
                "--max-processes",
                "8",
                "--max-wall-ms",
                "10000",
                "--drain-timeout-ms",
                "250",
                "--max-drain-requests",
                "1",
            ],
            cwd=workspace,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        if self._process.stdin is None or self._process.stdout is None or self._process.stderr is None:
            raise ProbeError("warm daemon pipes were unavailable")
        self._responses: queue.Queue[dict[str, Any] | BaseException | None] = queue.Queue()
        self._deferred: dict[str, list[dict[str, Any]]] = {}
        self._stderr: list[str] = []
        self._stdout_thread = threading.Thread(target=self._read_stdout, daemon=True)
        self._stderr_thread = threading.Thread(target=self._read_stderr, daemon=True)
        self._stdout_thread.start()
        self._stderr_thread.start()

    @property
    def pid(self) -> int:
        return self._process.pid

    def _read_stdout(self) -> None:
        assert self._process.stdout is not None
        try:
            for line in self._process.stdout:
                if line.strip():
                    self._responses.put(json.loads(line))
        except BaseException as exc:  # Forward reader failures to the controlling thread.
            self._responses.put(exc)
        finally:
            self._responses.put(None)

    def _read_stderr(self) -> None:
        assert self._process.stderr is not None
        self._stderr.extend(self._process.stderr)

    def send(self, frame: dict[str, Any]) -> None:
        if self._process.poll() is not None:
            raise ProbeError(f"warm daemon exited before request: {self.stderr_tail()}")
        assert self._process.stdin is not None
        self._process.stdin.write(json.dumps(frame, sort_keys=True, separators=(",", ":")) + "\n")
        self._process.stdin.flush()

    def terminal(self, request_id: str, *, accepted: bool = False) -> dict[str, Any]:
        deadline = time.monotonic() + RESPONSE_TIMEOUT_SECONDS
        saw_accepted = False
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ProbeError(f"warm response timed out for {request_id}: {self.stderr_tail()}")
            deferred = self._deferred.get(request_id)
            if deferred:
                item = deferred.pop(0)
            else:
                try:
                    item = self._responses.get(timeout=remaining)
                except queue.Empty as exc:
                    raise ProbeError(
                        f"warm response timed out for {request_id}: {self.stderr_tail()}"
                    ) from exc
            if isinstance(item, BaseException):
                raise ProbeError(f"warm response reader failed: {item}") from item
            if item is None:
                raise ProbeError(f"warm daemon closed before response {request_id}: {self.stderr_tail()}")
            if item.get("kind") != RESPONSE or item.get("protocol") != PROTOCOL:
                raise ProbeError(f"unexpected warm response shape: {item!r}")
            observed_id = item.get("id")
            if not isinstance(observed_id, str) or not observed_id:
                raise ProbeError(f"warm response has invalid request identity: {item!r}")
            if observed_id != request_id:
                self._deferred.setdefault(observed_id, []).append(item)
                continue
            if item.get("status") == "accepted":
                saw_accepted = True
                if accepted:
                    return item
                continue
            if accepted and not saw_accepted:
                raise ProbeError(f"request {request_id} terminalized before acceptance")
            return item

    def close_input(self) -> None:
        if self._process.stdin is not None and not self._process.stdin.closed:
            self._process.stdin.close()

    def wait(self) -> None:
        self.close_input()
        try:
            return_code = self._process.wait(timeout=RESPONSE_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired as exc:
            self._process.kill()
            self._process.wait(timeout=5)
            raise ProbeError("warm daemon did not terminate within its drain bound") from exc
        self._stdout_thread.join(timeout=2)
        self._stderr_thread.join(timeout=2)
        if self._stdout_thread.is_alive() or self._stderr_thread.is_alive():
            raise ProbeError("warm daemon response readers did not terminate")
        if return_code != 0:
            raise ProbeError(f"warm daemon failed with exit {return_code}: {self.stderr_tail()}")

    def terminate(self) -> None:
        if self._process.poll() is None:
            self._process.send_signal(signal.SIGTERM)
            try:
                self._process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait(timeout=2)

    def stderr_tail(self) -> str:
        return "".join(self._stderr)[-2000:].strip()


def initialize(daemon: WarmDaemon, request_id: str) -> dict[str, Any]:
    daemon.send(
        {
            "protocol": PROTOCOL,
            "id": request_id,
            "method": "initialize",
            "client": {"name": "host-bridge-daemon-lifecycle", "version": "0.1"},
        }
    )
    response = daemon.terminal(request_id)
    if not response.get("ok") or response.get("status") != "initialized":
        raise ProbeError(f"warm initialization failed: {response!r}")
    return response


def execute(daemon: WarmDaemon, request_id: str, policy: pathlib.Path) -> dict[str, Any]:
    daemon.send(
        {
            "protocol": PROTOCOL,
            "id": request_id,
            "method": "execute",
            "workspace": {"id": "probe", "root": "."},
            "argv": ["--json", "run", "provider.gc", "--caps", policy.name],
        }
    )
    response = daemon.terminal(request_id)
    data = response.get("data")
    error = response.get("error")
    data = data if isinstance(data, dict) else {}
    error = error if isinstance(error, dict) else {}
    details = error.get("details")
    details = details if isinstance(details, dict) else {}
    audit = data.get("audit") or details.get("audit")
    if not isinstance(audit, dict) or audit.get("worker_profile") != "native-isolated-v0.1":
        raise ProbeError(f"warm request did not retain a native lifecycle audit: {response!r}")
    return response


def new_records(
    path: pathlib.Path, previous_count: int, scenario: str
) -> list[tuple[int, int, str]]:
    deadline = time.monotonic() + RESPONSE_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        records = read_records(path)
        if len(records) > previous_count:
            return records[previous_count:]
        time.sleep(0.025)
    raise ProbeError(f"provider process identities were not recorded for {scenario}")


def run_probe(genesis: pathlib.Path, artifact: pathlib.Path, output: pathlib.Path) -> None:
    if os.name != "posix" or not shutil.which("ps"):
        raise ProbeError("daemon lifecycle evidence requires a POSIX host with ps")
    self_test(announce=False)
    genesis = genesis.resolve(strict=True)
    artifact = artifact.resolve(strict=True)
    if not os.access(genesis, os.X_OK):
        raise ProbeError(f"genesis executable is not runnable: {genesis}")

    started = time.monotonic()
    cleanup_samples: list[int] = []
    all_records: list[tuple[int, int, str]] = []
    daemon_pids: list[int] = []
    with tempfile.TemporaryDirectory(prefix="genesis-host-bridge-daemon-") as directory:
        workspace = pathlib.Path(directory)
        fixture = write_fixture(workspace)
        log_path = fixture["log"]

        daemon = WarmDaemon(genesis, artifact, workspace, log_path)
        daemon_pids.append(daemon.pid)
        try:
            initialize(daemon, "init-0")
            for index, mode in enumerate(("success", "malformed", "timeout")):
                before = len(read_records(log_path))
                response = execute(daemon, f"request-{mode}", fixture[mode])
                serialized = json.dumps(response, sort_keys=True)
                if mode == "success" and "daemon-probe-ok" not in serialized:
                    raise ProbeError(f"successful bridge response was not returned: {response!r}")
                if mode == "malformed" and "bridge-parse" not in serialized:
                    raise ProbeError(f"malformed bridge response was not sealed: {response!r}")
                if mode == "timeout" and "bridge-timeout" not in serialized:
                    raise ProbeError(f"bridge timeout was not sealed: {response!r}")
                observed = new_records(log_path, before, f"request-{mode}")
                all_records.extend(observed)
                cleanup_samples.append(wait_for_cleanup(observed))
                if index == 0:
                    daemon.send({"protocol": PROTOCOL, "id": "restart", "method": "restart"})
                    restarted = daemon.terminal("restart")
                    if not restarted.get("ok") or restarted.get("status") != "restarted":
                        raise ProbeError(f"warm restart failed: {restarted!r}")
                    if restarted.get("meta", {}).get("generation") != 1:
                        raise ProbeError(f"warm restart did not advance generation: {restarted!r}")
                    initialize(daemon, "init-1")
            daemon.send({"protocol": PROTOCOL, "id": "shutdown-idle", "method": "shutdown"})
            shutdown = daemon.terminal("shutdown-idle")
            if not shutdown.get("ok") or shutdown.get("status") != "draining":
                raise ProbeError(f"idle shutdown failed: {shutdown!r}")
            daemon.wait()
        finally:
            failed = sys.exc_info()[0] is not None
            daemon.terminate()
            if failed:
                force_cleanup(read_records(log_path))

        for method in ("shutdown", "eof"):
            daemon = WarmDaemon(genesis, artifact, workspace, log_path)
            daemon_pids.append(daemon.pid)
            try:
                initialize(daemon, f"init-{method}")
                before = len(read_records(log_path))
                daemon.send(
                    {
                        "protocol": PROTOCOL,
                        "id": f"active-{method}",
                        "method": "execute",
                        "workspace": {"id": "probe", "root": "."},
                        "argv": ["--json", "run", "provider.gc", "--caps", fixture["timeout"].name],
                    }
                )
                accepted = daemon.terminal(f"active-{method}", accepted=True)
                if accepted.get("status") != "accepted":
                    raise ProbeError(f"active {method} request was not accepted: {accepted!r}")
                observed = new_records(log_path, before, f"active-{method}")
                if method == "shutdown":
                    daemon.send({"protocol": PROTOCOL, "id": "shutdown-active", "method": "shutdown"})
                    draining = daemon.terminal("shutdown-active")
                    if not draining.get("ok") or draining.get("status") != "draining":
                        raise ProbeError(f"active shutdown did not enter draining: {draining!r}")
                daemon.close_input()
                daemon.wait()
                all_records.extend(observed)
                cleanup_samples.append(wait_for_cleanup(observed))
            finally:
                failed = sys.exc_info()[0] is not None
                daemon.terminate()
                if failed:
                    force_cleanup(read_records(log_path))

    provider_pids = [provider for provider, _, _ in all_records]
    descendant_pids = [descendant for _, descendant, _ in all_records]
    if len(provider_pids) != 5 or len(set(provider_pids)) != len(provider_pids):
        raise ProbeError(f"provider sessions were not isolated: {provider_pids!r}")
    if len(set(daemon_pids)) != len(daemon_pids):
        raise ProbeError(f"daemon process restart did not isolate process identity: {daemon_pids!r}")

    report = {
        "kind": REPORT_KIND,
        "version": "0.1",
        "ok": True,
        "platform": os.uname().sysname.lower(),
        "architecture": os.uname().machine.lower(),
        "daemon_processes": len(daemon_pids),
        "provider_processes": len(provider_pids),
        "descendant_processes": len(descendant_pids),
        "unique_provider_processes": len(set(provider_pids)),
        "scenarios": list(lifecycle_evidence.DAEMON_SCENARIOS),
        "cleanup": {
            "samples": len(cleanup_samples),
            "maximum_ms": max(cleanup_samples, default=0),
            "bound_ms": round(PROCESS_EXIT_TIMEOUT_SECONDS * 1000),
            "no_live_provider_or_descendant": True,
        },
        "transport": "persistent-stdio",
        "warm_protocol": PROTOCOL,
        "genesis_executable_sha256": sha256_file(genesis),
        "selfhost_artifact_sha256": sha256_file(artifact),
        "probe_source_sha256": sha256_file(pathlib.Path(__file__).resolve()),
        "negative_controls": list(lifecycle_evidence.NEGATIVE_CONTROLS),
        "elapsed_ms": round((time.monotonic() - started) * 1000),
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(
        "host-bridge-daemon-lifecycle: ok "
        f"platform={report['platform']} architecture={report['architecture']} "
        f"providers={report['provider_processes']} max_cleanup_ms={report['cleanup']['maximum_ms']}"
    )


def self_test(*, announce: bool = True) -> list[str]:
    controls: list[str] = []
    with tempfile.TemporaryDirectory(prefix="genesis-host-bridge-daemon-self-test-") as directory:
        record = pathlib.Path(directory) / "record.log"
        record.write_text("not-a-record\n", encoding="utf-8")
        try:
            read_records(record)
        except ProbeError:
            controls.append("reject-malformed-process-record")
        else:
            raise ProbeError("malformed process record negative control was accepted")

        record.write_text(f"{os.getpid()} {os.getpid()} persistent-stdio\n", encoding="utf-8")
        try:
            read_records(record)
        except ProbeError:
            controls.append("reject-duplicate-process-identity")
        else:
            raise ProbeError("duplicate process identity negative control was accepted")

        record.write_text("424242 424243 spawn-per-op\n", encoding="utf-8")
        try:
            read_records(record)
        except ProbeError:
            controls.append("reject-non-persistent-transport")
        else:
            raise ProbeError("non-persistent transport negative control was accepted")

        sleeper = subprocess.Popen(["sleep", "30"], start_new_session=True)
        try:
            if not process_or_group_has_live_member(sleeper.pid):
                raise ProbeError("live process-group negative control was not detected")
            controls.append("detect-live-process-group")
        finally:
            os.killpg(sleeper.pid, signal.SIGKILL)
            sleeper.wait(timeout=5)
        if process_or_group_has_live_member(sleeper.pid):
            raise ProbeError("reaped process-group control remained live")
        controls.append("accept-reaped-process-group")

    if (
        set(controls) != set(lifecycle_evidence.NEGATIVE_CONTROLS)
        or len(controls) != len(lifecycle_evidence.NEGATIVE_CONTROLS)
    ):
        raise ProbeError(f"negative-control coverage drifted: {controls!r}")
    if lifecycle_evidence.DAEMON_SCENARIOS != sorted(lifecycle_evidence.DAEMON_SCENARIOS):
        raise ProbeError("daemon lifecycle scenarios are not in canonical order")
    if announce:
        print(f"host-bridge-daemon-lifecycle: self-test ok (negative_controls={len(controls)})")
    return controls


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--genesis", type=pathlib.Path)
    parser.add_argument("--selfhost-artifact", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.self_test:
            if any(value is not None for value in (args.genesis, args.selfhost_artifact, args.output)):
                raise ProbeError("--self-test cannot be combined with probe inputs")
            self_test()
            return 0
        if args.genesis is None or args.selfhost_artifact is None or args.output is None:
            raise ProbeError("--genesis, --selfhost-artifact, and --output are required")
        run_probe(args.genesis, args.selfhost_artifact, args.output)
    except (OSError, ProbeError, subprocess.SubprocessError, ValueError) as exc:
        print(f"host-bridge-daemon-lifecycle: {exc}", file=os.sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
