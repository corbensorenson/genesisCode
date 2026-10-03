#!/usr/bin/env python3
"""Record and strictly verify the current canonical replay teaching pair."""
from __future__ import annotations

import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
from typing import Any

from supervisor_cancellation import cancellation_scope, deferred_cancellation

MAX_BYTES = 1024 * 1024
COMMAND_SECONDS = 45
DECISION_REASON = ":cap mismatch: deny decisions must carry nil cap"


def invoke(binary: Path, artifact: Path, workspace: Path, argv: list[str], error: type[ValueError]) -> tuple[int, dict[str, Any]]:
    import json

    # File-backed output avoids an unbounded PIPE allocation. The outer generated
    # authority owner accounts for these temporary files and total elapsed time.
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        process = None
        try:
            with deferred_cancellation():
                process = subprocess.Popen(
                    [str(binary), "--json", "--selfhost-artifact", str(artifact), *argv],
                    cwd=workspace, stdout=stdout, stderr=stderr,
                    start_new_session=os.name == "posix",
                )
            process.wait(timeout=COMMAND_SECONDS)
            stdout.seek(0)
            payload = stdout.read(MAX_BYTES + 1)
            if len(payload) > MAX_BYTES:
                raise error("replay producer output exceeds its bound")
            try:
                document = json.loads(payload)
            except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                raise error("replay producer returned invalid JSON") from exc
            if not isinstance(document, dict):
                raise error("replay producer envelope is not an object")
            return process.returncode, document
        except subprocess.TimeoutExpired as exc:
            raise error("replay producer exceeded its command deadline") from exc
        finally:
            with deferred_cancellation():
                if process is not None:
                    if os.name == "posix":
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                    elif process.poll() is None:
                        process.kill()
                    process.wait()


def assert_result(code: int, document: dict[str, Any], *, invalid: bool, error: type[ValueError]) -> None:
    if invalid:
        failure = document.get("error", {})
        if not isinstance(failure, dict) or not isinstance(failure.get("context", {}), dict):
            raise error("replay rejection envelope has invalid error context")
        facts = failure.get("context", {}).get("facts", {})
        if not isinstance(facts, dict):
            raise error("replay rejection envelope has invalid facts")
        if not (code == 40 and document.get("ok") is False
                and document.get("kind") == "genesis/error-v0.2"
                and failure.get("code") == "replay/mismatch"
                and facts.get("reason") == DECISION_REASON):
            raise error("replay invalid pair did not reject the declared decision mutation")
    elif not (code == 0 and document.get("ok") is True
              and document.get("kind") == "genesis/replay-v0.2"
              and isinstance(document.get("data"), dict)
              and document.get("data", {}).get("value") == "22"
              and document.get("data", {}).get("engine") == "selfhost"):
        raise error("replay valid pair did not strictly reproduce its declared value")


def produce_logs(root: Path, pair: dict[str, Any], binary: Path, error: type[ValueError]) -> dict[str, bytes]:
    artifact = root / "selfhost/toolchain.gc"
    if not binary.is_file() or not artifact.is_file():
        raise error("replay producer requires the fresh production CLI and pinned selfhost artifact")
    mutation = pair["mutation"]
    if mutation != {"kind": "replace-once", "path": "run.gclog", "before": ":decision :allow", "after": ":decision :deny"}:
        raise error("replay producer mutation contract drift")
    with cancellation_scope(), tempfile.TemporaryDirectory(prefix="genesis-canonical-replay-") as temporary:
        workspace = Path(temporary)
        for record in pair["valid"]["files"]:
            relative = record["path"]
            if relative == "run.gclog":
                continue
            target = workspace / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((root / pair["valid"]["root"] / relative).read_bytes())
        code, document = invoke(binary, artifact, workspace,
                                ["run", "program.gc", "--caps", "caps.toml", "--log", "run.gclog"], error)
        if not (code == 0 and document.get("ok") is True
                and document.get("kind") == "genesis/run-v0.2"
                and isinstance(document.get("data"), dict)
                and document.get("data", {}).get("value") == "22"
                and document.get("data", {}).get("entries") == 1
                and document.get("data", {}).get("denied") is False
                and document.get("data", {}).get("engine") == "selfhost"):
            raise error("replay recording did not produce the reviewed single allowed read")
        log = workspace / "run.gclog"
        if not log.is_file() or log.is_symlink() or log.stat().st_size > MAX_BYTES:
            raise error("replay producer log is missing or exceeds its bound")
        valid = log.read_bytes()
        before, after = mutation["before"].encode(), mutation["after"].encode()
        if valid.count(before) != 1 or b':version 3}' not in valid:
            raise error("replay producer log version or one-site mutation contract drift")
        replay = ["replay", "program.gc", "--log", "run.gclog"]
        assert_result(*invoke(binary, artifact, workspace, replay, error), invalid=False, error=error)
        invalid = valid.replace(before, after, 1)
        log.write_bytes(invalid)
        assert_result(*invoke(binary, artifact, workspace, replay, error), invalid=True, error=error)
        return {f"{pair['valid']['root']}/run.gclog": valid,
                f"{pair['invalid']['root']}/run.gclog": invalid}


def self_test(root: Path, pair: dict[str, Any], error: type[ValueError]) -> int:
    """Producer fault controls; these never stand in for real CLI conformance."""
    import hashlib
    import json

    paths = [root / pair[side]["root"] / "run.gclog" for side in ("valid", "invalid")]
    before = {path: hashlib.sha256(path.read_bytes()).digest() for path in paths}
    original = paths[0].read_bytes()
    recorded = {"kind": "genesis/run-v0.2", "ok": True,
                "data": {"value": "22", "entries": 1, "denied": False, "engine": "selfhost"}}
    accepted = {"kind": "genesis/replay-v0.2", "ok": True,
                "data": {"value": "22", "engine": "selfhost"}}
    rejected = {"ok": False, "kind": "genesis/error-v0.2",
                "error": {"code": "replay/mismatch", "context": {"facts": {"reason": DECISION_REASON}}}}
    stale = json.loads(json.dumps(rejected))
    stale["error"]["context"]["facts"]["reason"] = "continuation hash mismatch"
    wrong_code = json.loads(json.dumps(rejected))
    wrong_code["error"]["code"] = "core/internal"
    controls = [
        ("accepted-mutation", recorded, accepted, 0, original),
        ("earlier-mismatch", recorded, stale, 40, original),
        ("wrong-diagnostic", recorded, wrong_code, 40, original),
        ("wrong-version", recorded, rejected, 40, original.replace(b":version 3}", b":version 2}")),
        ("missing-log", recorded, rejected, 40, None),
        ("oversize-log", recorded, rejected, 40, b"x" * (MAX_BYTES + 1)),
        ("malformed-recording", [], rejected, 40, original),
    ]
    with tempfile.TemporaryDirectory(prefix="genesis-replay-producer-controls-") as temporary:
        binary = Path(temporary) / "producer"
        for name, run_result, invalid_result, invalid_code, log in controls:
            # The valid replay always succeeds. Rejection must be attributable to
            # the late injected fault, never an unrelated valid-side failure.
            source = (
                f"#!{sys.executable}\nimport pathlib,sys\n"
                "p=pathlib.Path('run.gclog')\n"
                "if 'run' in sys.argv:\n"
                + (f" p.write_bytes({log!r})\n" if log is not None else " pass\n")
                + f" print({json.dumps(run_result)!r})\n sys.exit(0)\n"
                "invalid=b':decision :deny' in p.read_bytes()\n"
                f"print({json.dumps(invalid_result)!r} if invalid else {json.dumps(accepted)!r})\n"
                f"sys.exit({invalid_code} if invalid else 0)\n"
            )
            binary.write_text(source, encoding="utf-8")
            binary.chmod(0o700)
            try:
                produce_logs(root, pair, binary, error)
            except error:
                pass
            else:
                raise error(f"replay producer accepted fault: {name}")
            if any(hashlib.sha256(path.read_bytes()).digest() != digest for path, digest in before.items()):
                raise error(f"replay producer fault changed a canonical log: {name}")
    return len(controls)
