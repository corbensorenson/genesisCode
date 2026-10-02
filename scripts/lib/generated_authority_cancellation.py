#!/usr/bin/env python3
"""Bounded process-group, staging, and atomic-publication cancellation controls."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time

import generated_authority as ga


def wait_for(predicate, label: str, seconds: float = 8) -> None:
    deadline = time.monotonic() + seconds
    while not predicate():
        ga.require(time.monotonic() < deadline, f"cancellation fixture did not reach {label}")
        time.sleep(0.01)


def group_alive(pid: int) -> bool:
    try:
        os.killpg(pid, 0)
        return True
    except ProcessLookupError:
        return False


def worker(root: Path, scope: Path, case: str) -> int:
    ready = scope / "ready.json"
    linger = (
        "import json,os,time; from pathlib import Path; "
        f"Path({str(ready)!r}).write_text(json.dumps({{'pid':os.getpid(),'pgid':os.getpgrp()}})); "
        "time.sleep(30)"
    )
    owner = None
    try:
        with ga.cancellation_scope():
            if case.startswith("publish"):
                live, stage = scope / "live", scope / "stage"
                for directory in (live, stage):
                    directory.mkdir()
                    subprocess.run(["git", "init", "-q", str(directory)], check=True)
                for name in ("a", "b"):
                    (live / name).write_text("old")
                    (stage / name).write_text("new")
                replace = os.replace
                fired = False
                def interrupted_replace(source, destination):
                    nonlocal fired
                    replace(source, destination)
                    if Path(destination) == live / "a" and not fired:
                        fired = True
                        os.kill(os.getpid(), signal.SIGTERM)
                ga.os.replace = interrupted_replace
                if case == "publish-rollback":
                    os.environ["GENESIS_GENERATED_AUTHORITY_FAIL_AFTER_PROMOTIONS"] = "1"
                try:
                    ga.promote(live, stage, ["a", "b"])
                finally:
                    ga.os.replace = replace
                    os.environ.pop("GENESIS_GENERATED_AUTHORITY_FAIL_AFTER_PROMOTIONS", None)
            elif case.startswith("stage"):
                live = scope / "live"
                live.mkdir()
                (live / "policies").mkdir()
                shutil.copy2(root / "policies/gate_telemetry_v0.1.json", live / "policies/gate_telemetry_v0.1.json")
                (live / "a").write_text("old")
                subprocess.run(["git", "init", "-q", str(live)], check=True)
                subprocess.run(["git", "add", "."], cwd=live, check=True)
                subprocess.run(["git", "-c", "user.name=control", "-c", "user.email=control@invalid", "commit", "-qm", "fixture"], cwd=live, check=True)
                mkdtemp = ga.tempfile.mkdtemp
                def owned_temporary(*args, **kwargs):
                    if kwargs.get("prefix") == "generated-authority-stage-":
                        kwargs["dir"] = scope
                    return mkdtemp(*args, **kwargs)
                ga.tempfile.mkdtemp = owned_temporary
                close = ga.AggregateResourceOwner.close
                def interrupted_cleanup(self, **kwargs):
                    # Repeated stop requests during unwind cannot bypass cleanup.
                    os.kill(os.getpid(), signal.SIGTERM)
                    os.kill(os.getpid(), signal.SIGINT)
                    return close(self, **kwargs)
                ga.AggregateResourceOwner.close = interrupted_cleanup
                try:
                    command = [sys.executable,"-c",linger]
                    if case == "stage-telemetry":
                        for _ in range(2):
                            command = [sys.executable, str(root / "scripts/lib/gate_telemetry.py"), "--root", str(root), "--entrypoint", "scripts/check_doc_hygiene.sh", "--emit", "none", "--", *command]
                    ga.stage_closure(live, [{"id":"fixture", "outputs":["a"], "command":command, "timeoutSeconds":20,"diskMiB":64,"checks":[]}], update=True, limits={"maxTimeoutSeconds":25,"maxDiskMiB":64})
                finally:
                    ga.tempfile.mkdtemp = mkdtemp
                    ga.AggregateResourceOwner.close = close
            else:
                scripts = scope / "scripts"
                scripts.mkdir()
                (scripts / "linger.sh").write_text("exec " + shlex.quote(sys.executable) + " -c " + shlex.quote(linger) + "\n")
                if case == "leader-exit":
                    # The shell exits successfully while its background child remains.
                    (scripts / "linger.sh").write_text(shlex.quote(sys.executable) + " -c " + shlex.quote(linger) + " &\nwhile [ ! -f " + shlex.quote(str(ready)) + " ]; do sleep .01; done\n")
                if case in ("registration", "spawn-failure"):
                    popen = ga.subprocess.Popen
                    calls = 0
                    def interrupted_spawn(*args, **kwargs):
                        nonlocal calls
                        calls += 1
                        if case == "spawn-failure" and calls == 2:
                            wait_for(ready.exists, "first validator readiness")
                            raise OSError("injected validator spawn failure")
                        process = popen(*args, **kwargs)
                        if case == "registration":
                            wait_for(ready.exists, "spawn registration")
                            os.kill(os.getpid(), signal.SIGTERM)
                        return process
                    ga.subprocess.Popen = interrupted_spawn
                owner = ga.AggregateResourceOwner(scope, scope, {"maxTimeoutSeconds":25,"maxDiskMiB":64}, sampling_root=root)
                if case in ("checks", "timeout", "spawn-failure", "leader-exit"):
                    checks = ["scripts/linger.sh"]
                    if case == "spawn-failure":
                        (scripts / "second.sh").write_text("exit 0\n")
                        checks.append("scripts/second.sh")
                    ga.run_checks(scope,[{"checks":checks,"timeoutSeconds":1 if case == "timeout" else 20}],owner)
                else:
                    ga.run_bounded([sys.executable,"-c",linger],cwd=scope,timeout=20,owner=owner)
    except ga.AuthorityCancelled as exc:
        return 128 + exc.signum
    except (ga.AuthorityError, OSError, subprocess.TimeoutExpired) as exc:
        print(str(exc), file=sys.stderr)
        return 1
    finally:
        if owner is not None:
            owner.close(validate=False)
    return 0


def cancellation_self_test(root: Path) -> int:
    if os.name == "nt":
        print("generated-authority-cancellation: unsupported process-group fixture on Windows")
        return 0
    cases = [(case, signum) for case in ("bounded", "checks", "stage", "stage-telemetry") for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)]
    cases += [(case, None) for case in ("registration", "timeout", "spawn-failure", "leader-exit", "publish", "publish-rollback")]
    controls = []
    with tempfile.TemporaryDirectory(prefix="generated-authority-cancellation-controls-") as temporary:
        for index, (case, signum) in enumerate(cases):
            scope = Path(temporary) / str(index)
            scope.mkdir()
            ready = scope / "ready.json"
            with (scope / "worker.log").open("wb") as log:
                process = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "--worker", case, "--root", str(root), "--scope", str(scope)], stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                child_pid = None
                try:
                    if signum is not None:
                        wait_for(lambda: ready.exists() or process.poll() is not None, "worker readiness")
                        ga.require(ready.exists(), (scope / "worker.log").read_text())
                        child_pid = json.loads(ready.read_text())["pgid"]
                        process.send_signal(signum)
                    result = process.wait(timeout=12)
                    expected = 128 + signum if signum is not None else {
                        "registration":143,"timeout":1,"spawn-failure":1,
                        "leader-exit":0,"publish":143,"publish-rollback":143,
                    }[case]
                    ga.require(result == expected, f"{case}: expected {expected}, got {result}: {(scope / 'worker.log').read_text()}")
                    if ready.exists():
                        child_pid = json.loads(ready.read_text())["pgid"]
                        wait_for(lambda: not group_alive(child_pid), "child group cleanup")
                    if case.startswith("stage"):
                        ga.require(not list(scope.glob("generated-authority-stage-*")), "staging directory survived termination")
                        worktrees = ga.git(scope / "live", "worktree", "list", "--porcelain")
                        ga.require(worktrees.count("worktree ") == 1, "staging Git registration survived termination")
                        ga.require((scope / "live/a").read_text() == "old", "cancelled staging published an output")
                    if case.startswith("publish"):
                        expected_bytes = "old" if case == "publish-rollback" else "new"
                        ga.require(all((scope / "live" / name).read_text() == expected_bytes for name in ("a", "b")), "signal left partial publication")
                        common = ga.common_git_dir(scope / "live")
                        ga.require(not (common / ga.LOCK_NAME).exists(), "publication lock survived termination")
                        ga.require(not list(common.glob("generated-authority-transaction-*")), "rollback journal survived termination")
                        ga.require(not list((scope / "live").glob(".*.generated-authority-*")), "publication temporary survived termination")
                    controls.append({"case":case,"signal":signum,"returncode":result})
                finally:
                    ga.kill_and_reap(process)
                    if ready.exists():
                        child_pid = json.loads(ready.read_text())["pgid"]
                    if child_pid is not None and group_alive(child_pid):
                        os.killpg(child_pid, signal.SIGKILL)
    ga.require(len(controls) == 18, "cancellation control inventory drift")
    print("generated-authority-cancellation: " + json.dumps(controls, sort_keys=True))
    return len(controls)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ga.ROOT)
    parser.add_argument("--scope", type=Path)
    parser.add_argument("--worker")
    args = parser.parse_args()
    if args.worker:
        raise SystemExit(worker(args.root, args.scope, args.worker))
    cancellation_self_test(args.root)
